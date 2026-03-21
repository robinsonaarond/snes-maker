use std::collections::BTreeMap;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use snesmaker_project::{
    CompiledScene, GenreModule, PhysicsProfile, SceneCompiler, SceneKind, SceneResource,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct InputFrame {
    pub left: bool,
    pub right: bool,
    pub jump_pressed: bool,
    pub jump_held: bool,
    pub climb_up: bool,
    pub climb_down: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraceFrame {
    pub frame: u32,
    pub x_fp: i32,
    pub y_fp: i32,
    pub vx_fp: i32,
    pub vy_fp: i32,
    pub grounded: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SimState {
    x_fp: i32,
    y_fp: i32,
    vx_fp: i32,
    vy_fp: i32,
    grounded: bool,
    coyote_left: u8,
    jump_buffer_left: u8,
}

pub fn simulate_trace(profile: &PhysicsProfile, inputs: &[InputFrame]) -> Vec<TraceFrame> {
    let mut state = SimState {
        x_fp: 0,
        y_fp: 0,
        vx_fp: 0,
        vy_fp: 0,
        grounded: true,
        coyote_left: profile.coyote_frames,
        jump_buffer_left: 0,
    };

    let mut output = Vec::with_capacity(inputs.len());

    for (frame, input) in inputs.iter().enumerate() {
        if input.jump_pressed {
            state.jump_buffer_left = profile.jump_buffer_frames;
        }

        let horizontal = match (input.left, input.right) {
            (true, false) => -1,
            (false, true) => 1,
            _ => 0,
        };

        let accel = if state.grounded {
            profile.ground_accel_fp
        } else {
            profile.air_accel_fp
        };

        match horizontal {
            -1 => state.vx_fp = (state.vx_fp - accel).max(-profile.max_run_speed_fp),
            1 => state.vx_fp = (state.vx_fp + accel).min(profile.max_run_speed_fp),
            _ => state.vx_fp = apply_friction(state.vx_fp, accel),
        }

        if state.grounded {
            state.coyote_left = profile.coyote_frames;
        } else if state.coyote_left > 0 {
            state.coyote_left -= 1;
        }

        if state.jump_buffer_left > 0 {
            state.jump_buffer_left -= 1;
        }

        if state.jump_buffer_left > 0 && (state.grounded || state.coyote_left > 0) {
            state.vy_fp = profile.jump_velocity_fp;
            state.grounded = false;
            state.coyote_left = 0;
            state.jump_buffer_left = 0;
        }

        if input.climb_up || input.climb_down {
            let climb_direction = if input.climb_up {
                -1
            } else if input.climb_down {
                1
            } else {
                0
            };
            state.vy_fp = climb_direction * profile.ladder_speed_fp;
        } else {
            state.vy_fp = (state.vy_fp + profile.gravity_fp).min(profile.max_fall_speed_fp);
        }

        state.x_fp += state.vx_fp;
        state.y_fp += state.vy_fp;

        if state.y_fp >= 0 {
            state.y_fp = 0;
            state.vy_fp = 0;
            state.grounded = true;
        } else {
            state.grounded = false;
        }

        output.push(TraceFrame {
            frame: frame as u32,
            x_fp: state.x_fp,
            y_fp: state.y_fp,
            vx_fp: state.vx_fp,
            vy_fp: state.vy_fp,
            grounded: state.grounded,
        });
    }

    output
}

pub struct PlatformerSceneCompiler;

impl SceneCompiler for PlatformerSceneCompiler {
    fn genre(&self) -> SceneKind {
        SceneKind::SideScroller
    }

    fn compile_scene(
        &self,
        _bundle: &snesmaker_project::ProjectBundle,
        scene: &SceneResource,
    ) -> Result<CompiledScene> {
        let layer = scene
            .layers
            .first()
            .ok_or_else(|| anyhow::anyhow!("scene '{}' has no layers to compile", scene.id))?;

        let mut data_bytes = Vec::with_capacity(layer.tiles.len() * 2 + scene.entities.len() * 6);
        for tile in &layer.tiles {
            data_bytes.extend(tile.to_le_bytes());
        }

        for entity in &scene.entities {
            data_bytes.extend(entity.position.x.to_le_bytes());
            data_bytes.extend(entity.position.y.to_le_bytes());
            data_bytes.push(match entity.facing {
                snesmaker_project::Facing::Left => 0,
                snesmaker_project::Facing::Right => 1,
            });
            data_bytes.push(entity.archetype.len() as u8);
        }

        let mut metadata = BTreeMap::new();
        metadata.insert(
            "chunk_width".to_string(),
            scene.chunk_size_tiles.width.to_string(),
        );
        metadata.insert(
            "chunk_height".to_string(),
            scene.chunk_size_tiles.height.to_string(),
        );
        metadata.insert("entity_count".to_string(), scene.entities.len().to_string());

        Ok(CompiledScene {
            scene_id: scene.id.clone(),
            genre: SceneKind::SideScroller,
            data_bytes,
            metadata,
        })
    }
}

pub struct PlatformerGenreModule {
    compiler: PlatformerSceneCompiler,
}

impl Default for PlatformerGenreModule {
    fn default() -> Self {
        Self {
            compiler: PlatformerSceneCompiler,
        }
    }
}

impl GenreModule for PlatformerGenreModule {
    fn id(&self) -> &'static str {
        "side_scroller"
    }

    fn supports(&self, scene: &SceneResource) -> bool {
        scene.kind == SceneKind::SideScroller
    }

    fn scene_compiler(&self) -> &dyn SceneCompiler {
        &self.compiler
    }
}

fn apply_friction(value: i32, amount: i32) -> i32 {
    if value > 0 {
        (value - amount).max(0)
    } else if value < 0 {
        (value + amount).min(0)
    } else {
        0
    }
}

pub fn compile_scene(scene: &SceneResource) -> Result<CompiledScene> {
    if scene.kind != SceneKind::SideScroller {
        bail!("platformer compiler only supports side-scroller scenes");
    }

    PlatformerSceneCompiler.compile_scene(&snesmaker_project::ProjectBundle::default(), scene)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn megaman_like_trace_is_deterministic() {
        let profile = snesmaker_project::default_megaman_like_physics();
        let inputs = vec![
            InputFrame {
                right: true,
                ..InputFrame::default()
            },
            InputFrame {
                right: true,
                jump_pressed: true,
                jump_held: true,
                ..InputFrame::default()
            },
            InputFrame {
                right: true,
                jump_held: true,
                ..InputFrame::default()
            },
            InputFrame {
                right: true,
                ..InputFrame::default()
            },
            InputFrame {
                ..InputFrame::default()
            },
        ];

        let trace = simulate_trace(&profile, &inputs);
        let expected_x: Vec<i32> = trace.iter().map(|frame| frame.x_fp).collect();
        assert_eq!(expected_x, vec![90, 270, 506, 798, 1034]);
        assert!(!trace[1].grounded);
    }
}
