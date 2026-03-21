use std::collections::BTreeMap;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use snesmaker_project::{
    Checkpoint, CompiledScene, GenreModule, PhysicsProfile, PointI16, SceneCompiler, SceneKind,
    SceneResource, SpawnPoint,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PlaytestState {
    pub frame: u32,
    pub x_fp: i32,
    pub y_fp: i32,
    pub vx_fp: i32,
    pub vy_fp: i32,
    pub grounded: bool,
    pub on_ladder: bool,
    pub touching_hazard: bool,
}

#[derive(Debug, Clone)]
pub struct PlaytestSession {
    scene: SceneResource,
    profile: PhysicsProfile,
    state: PlaytestState,
    coyote_left: u8,
    jump_buffer_left: u8,
    player_width_px: i16,
    player_height_px: i16,
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

const TILE_SIZE_PX: i32 = 8;
const DEFAULT_PLAYER_WIDTH_PX: i16 = 16;
const DEFAULT_PLAYER_HEIGHT_PX: i16 = 16;
const FIXED_POINT_ONE: i32 = 1 << snesmaker_project::FIXED_POINT_SHIFT;

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

impl PlaytestSession {
    pub fn new(scene: &SceneResource, profile: PhysicsProfile) -> Self {
        let mut session = Self {
            scene: scene.clone(),
            profile,
            state: PlaytestState::default(),
            coyote_left: 0,
            jump_buffer_left: 0,
            player_width_px: DEFAULT_PLAYER_WIDTH_PX,
            player_height_px: DEFAULT_PLAYER_HEIGHT_PX,
        };
        session.reset_to_default_start();
        session
    }

    pub fn state(&self) -> PlaytestState {
        self.state
    }

    pub fn reset_to_default_start(&mut self) -> PlaytestState {
        if let Some(spawn) = self.scene.spawns.first().cloned() {
            self.reset_to_spawn(&spawn)
        } else if let Some(checkpoint) = self.scene.checkpoints.first().cloned() {
            self.reset_to_checkpoint(&checkpoint)
        } else {
            self.reset_to_position(PointI16 { x: 0, y: 0 })
        }
    }

    pub fn reset_to_position(&mut self, position: PointI16) -> PlaytestState {
        self.state = PlaytestState {
            frame: 0,
            x_fp: fixed_point_from_pixels(position.x.into()),
            y_fp: fixed_point_from_pixels(position.y.into()),
            vx_fp: 0,
            vy_fp: 0,
            grounded: false,
            on_ladder: false,
            touching_hazard: false,
        };
        self.jump_buffer_left = 0;
        self.refresh_contacts();
        self.coyote_left = if self.state.grounded {
            self.profile.coyote_frames
        } else {
            0
        };
        self.state
    }

    pub fn reset_to_spawn(&mut self, spawn: &SpawnPoint) -> PlaytestState {
        self.reset_to_position(spawn.position)
    }

    pub fn reset_to_checkpoint(&mut self, checkpoint: &Checkpoint) -> PlaytestState {
        self.reset_to_position(checkpoint.position)
    }

    pub fn reset_to_spawn_id(&mut self, spawn_id: &str) -> Result<PlaytestState> {
        let spawn = self
            .scene
            .spawns
            .iter()
            .find(|spawn| spawn.id == spawn_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("spawn '{}' is missing", spawn_id))?;
        Ok(self.reset_to_spawn(&spawn))
    }

    pub fn reset_to_checkpoint_id(&mut self, checkpoint_id: &str) -> Result<PlaytestState> {
        let checkpoint = self
            .scene
            .checkpoints
            .iter()
            .find(|checkpoint| checkpoint.id == checkpoint_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("checkpoint '{}' is missing", checkpoint_id))?;
        Ok(self.reset_to_checkpoint(&checkpoint))
    }

    pub fn step(&mut self, input: InputFrame) -> PlaytestState {
        self.state.frame = self.state.frame.saturating_add(1);

        if input.jump_pressed {
            self.jump_buffer_left = self.profile.jump_buffer_frames;
        } else if self.jump_buffer_left > 0 {
            self.jump_buffer_left -= 1;
        }

        if self.state.grounded {
            self.coyote_left = self.profile.coyote_frames;
        } else if self.coyote_left > 0 {
            self.coyote_left -= 1;
        }

        let width_px = self.player_width_px as i32;
        let height_px = self.player_height_px as i32;
        let ladder_contact = self.overlaps_ladder(self.state.x_fp, self.state.y_fp);
        let horizontal = match (input.left, input.right) {
            (true, false) => -1,
            (false, true) => 1,
            _ => 0,
        };
        let accel = if self.state.grounded {
            self.profile.ground_accel_fp
        } else {
            self.profile.air_accel_fp
        };

        match horizontal {
            -1 => self.state.vx_fp = (self.state.vx_fp - accel).max(-self.profile.max_run_speed_fp),
            1 => self.state.vx_fp = (self.state.vx_fp + accel).min(self.profile.max_run_speed_fp),
            _ => self.state.vx_fp = apply_friction(self.state.vx_fp, accel),
        }

        if ladder_contact && (input.climb_up || input.climb_down) {
            let climb_direction = if input.climb_up {
                -1
            } else if input.climb_down {
                1
            } else {
                0
            };
            self.state.vy_fp = climb_direction * self.profile.ladder_speed_fp;
            self.state.grounded = false;
            self.state.on_ladder = true;
        } else if self.jump_buffer_left > 0
            && (self.state.grounded || self.coyote_left > 0 || ladder_contact)
        {
            self.state.vy_fp = self.profile.jump_velocity_fp;
            self.state.grounded = false;
            self.state.on_ladder = false;
            self.jump_buffer_left = 0;
            self.coyote_left = 0;
        } else {
            self.state.vy_fp =
                (self.state.vy_fp + self.profile.gravity_fp).min(self.profile.max_fall_speed_fp);
            self.state.on_ladder = ladder_contact;
        }

        let next_x_fp = self.state.x_fp + self.state.vx_fp;
        let (next_x_fp, collided_x) = resolve_horizontal_collision(
            &self.scene,
            next_x_fp,
            self.state.y_fp,
            width_px,
            height_px,
            self.state.vx_fp,
        );
        self.state.x_fp = next_x_fp;
        if collided_x {
            self.state.vx_fp = 0;
        }

        let next_y_fp = self.state.y_fp + self.state.vy_fp;
        let (next_y_fp, collided_y, grounded) = resolve_vertical_collision(
            &self.scene,
            self.state.x_fp,
            next_y_fp,
            width_px,
            height_px,
            self.state.vy_fp,
        );
        self.state.y_fp = next_y_fp;
        if collided_y {
            if self.state.vy_fp > 0 {
                self.state.vy_fp = 0;
            } else if self.state.vy_fp < 0 {
                self.state.vy_fp = 0;
            }
        }
        self.state.grounded = grounded;
        self.state.on_ladder = self.overlaps_ladder(self.state.x_fp, self.state.y_fp);
        self.state.touching_hazard = self.overlaps_hazard(self.state.x_fp, self.state.y_fp);
        if self.state.grounded {
            self.coyote_left = self.profile.coyote_frames;
        }

        self.state
    }

    fn refresh_contacts(&mut self) {
        self.state.on_ladder = self.overlaps_ladder(self.state.x_fp, self.state.y_fp);
        self.state.touching_hazard = self.overlaps_hazard(self.state.x_fp, self.state.y_fp);
        self.state.grounded = self.touching_solid_below(self.state.x_fp, self.state.y_fp);
    }

    fn overlaps_ladder(&self, x_fp: i32, y_fp: i32) -> bool {
        rect_overlaps_flag(
            &self.scene,
            x_fp,
            y_fp,
            i32::from(self.player_width_px),
            i32::from(self.player_height_px),
            |tile| tile.ladder,
        )
    }

    fn overlaps_hazard(&self, x_fp: i32, y_fp: i32) -> bool {
        rect_overlaps_flag(
            &self.scene,
            x_fp,
            y_fp,
            i32::from(self.player_width_px),
            i32::from(self.player_height_px),
            |tile| tile.hazard,
        )
    }

    fn touching_solid_below(&self, x_fp: i32, y_fp: i32) -> bool {
        let below_y_fp = y_fp + fixed_point_from_pixels(self.player_height_px as i32);
        rect_overlaps_flag(
            &self.scene,
            x_fp,
            below_y_fp,
            i32::from(self.player_width_px),
            1,
            |tile| tile.solid,
        )
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct CollisionTile {
    solid: bool,
    ladder: bool,
    hazard: bool,
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

fn fixed_point_from_pixels(pixels: i32) -> i32 {
    pixels * FIXED_POINT_ONE
}

fn fixed_point_to_pixels_floor(value_fp: i32) -> i32 {
    value_fp.div_euclid(FIXED_POINT_ONE)
}

fn scene_dimensions_pixels(scene: &SceneResource) -> (i32, i32) {
    (
        i32::from(scene.size_tiles.width) * TILE_SIZE_PX,
        i32::from(scene.size_tiles.height) * TILE_SIZE_PX,
    )
}

fn scene_collision_tile(scene: &SceneResource, tile_x: i32, tile_y: i32) -> CollisionTile {
    if tile_x < 0
        || tile_y < 0
        || tile_x >= i32::from(scene.size_tiles.width)
        || tile_y >= i32::from(scene.size_tiles.height)
    {
        return CollisionTile {
            solid: true,
            ladder: false,
            hazard: false,
        };
    }

    let tile_index = tile_y as usize * usize::from(scene.size_tiles.width) + tile_x as usize;
    CollisionTile {
        solid: scene
            .collision
            .solids
            .get(tile_index)
            .copied()
            .unwrap_or(false),
        ladder: scene
            .collision
            .ladders
            .get(tile_index)
            .copied()
            .unwrap_or(false),
        hazard: scene
            .collision
            .hazards
            .get(tile_index)
            .copied()
            .unwrap_or(false),
    }
}

fn rect_overlaps_flag(
    scene: &SceneResource,
    x_fp: i32,
    y_fp: i32,
    width_px: i32,
    height_px: i32,
    mut test: impl FnMut(CollisionTile) -> bool,
) -> bool {
    let width_fp = fixed_point_from_pixels(width_px);
    let height_fp = fixed_point_from_pixels(height_px);
    let left_px = fixed_point_to_pixels_floor(x_fp);
    let top_px = fixed_point_to_pixels_floor(y_fp);
    let right_px = fixed_point_to_pixels_floor(x_fp + width_fp - 1);
    let bottom_px = fixed_point_to_pixels_floor(y_fp + height_fp - 1);

    let left_tile = left_px.div_euclid(TILE_SIZE_PX);
    let right_tile = right_px.div_euclid(TILE_SIZE_PX);
    let top_tile = top_px.div_euclid(TILE_SIZE_PX);
    let bottom_tile = bottom_px.div_euclid(TILE_SIZE_PX);

    for tile_y in top_tile..=bottom_tile {
        for tile_x in left_tile..=right_tile {
            if test(scene_collision_tile(scene, tile_x, tile_y)) {
                return true;
            }
        }
    }

    false
}

fn resolve_horizontal_collision(
    scene: &SceneResource,
    x_fp: i32,
    y_fp: i32,
    width_px: i32,
    height_px: i32,
    delta_fp: i32,
) -> (i32, bool) {
    let (scene_width_px, _scene_height_px) = scene_dimensions_pixels(scene);
    let width_fp = fixed_point_from_pixels(width_px);
    let height_fp = fixed_point_from_pixels(height_px);
    let left_px = fixed_point_to_pixels_floor(x_fp);
    let top_px = fixed_point_to_pixels_floor(y_fp);
    let right_px = fixed_point_to_pixels_floor(x_fp + width_fp - 1);
    let bottom_px = fixed_point_to_pixels_floor(y_fp + height_fp - 1);
    let top_tile = top_px.div_euclid(TILE_SIZE_PX);
    let bottom_tile = bottom_px.div_euclid(TILE_SIZE_PX);

    if left_px < 0 {
        return (0, true);
    }
    if right_px >= scene_width_px {
        return (
            fixed_point_from_pixels(scene_width_px.saturating_sub(width_px)),
            true,
        );
    }

    let moving_right = delta_fp > 0;
    let moving_left = delta_fp < 0;
    if !moving_right && !moving_left {
        return (x_fp, false);
    }

    let mut clamped_x_fp = x_fp;
    let mut collided = false;

    if moving_right {
        let tile_x = right_px.div_euclid(TILE_SIZE_PX);
        for tile_y in top_tile..=bottom_tile {
            let tile = scene_collision_tile(scene, tile_x, tile_y);
            if tile.solid {
                let candidate = fixed_point_from_pixels(tile_x * TILE_SIZE_PX - width_px);
                if !collided || candidate < clamped_x_fp {
                    clamped_x_fp = candidate;
                }
                collided = true;
            }
        }
    } else {
        let tile_x = left_px.div_euclid(TILE_SIZE_PX);
        for tile_y in top_tile..=bottom_tile {
            let tile = scene_collision_tile(scene, tile_x, tile_y);
            if tile.solid {
                let candidate = fixed_point_from_pixels((tile_x + 1) * TILE_SIZE_PX);
                if !collided || candidate > clamped_x_fp {
                    clamped_x_fp = candidate;
                }
                collided = true;
            }
        }
    }

    (clamped_x_fp, collided)
}

fn resolve_vertical_collision(
    scene: &SceneResource,
    x_fp: i32,
    y_fp: i32,
    width_px: i32,
    height_px: i32,
    delta_fp: i32,
) -> (i32, bool, bool) {
    let (_scene_width_px, scene_height_px) = scene_dimensions_pixels(scene);
    let width_fp = fixed_point_from_pixels(width_px);
    let height_fp = fixed_point_from_pixels(height_px);
    let left_px = fixed_point_to_pixels_floor(x_fp);
    let top_px = fixed_point_to_pixels_floor(y_fp);
    let right_px = fixed_point_to_pixels_floor(x_fp + width_fp - 1);
    let bottom_px = fixed_point_to_pixels_floor(y_fp + height_fp - 1);
    let left_tile = left_px.div_euclid(TILE_SIZE_PX);
    let right_tile = right_px.div_euclid(TILE_SIZE_PX);

    if top_px < 0 {
        return (0, true, false);
    }
    if bottom_px >= scene_height_px {
        return (
            fixed_point_from_pixels(scene_height_px.saturating_sub(height_px)),
            true,
            true,
        );
    }

    let moving_down = delta_fp > 0;
    let moving_up = delta_fp < 0;
    if !moving_down && !moving_up {
        let grounded = rect_overlaps_flag(
            scene,
            x_fp,
            y_fp + fixed_point_from_pixels(height_px),
            width_px,
            1,
            |tile| tile.solid,
        );
        return (y_fp, false, grounded);
    }

    let mut clamped_y_fp = y_fp;
    let mut collided = false;
    let mut grounded = false;

    if moving_down {
        let tile_y = bottom_px.div_euclid(TILE_SIZE_PX);
        for tile_x in left_tile..=right_tile {
            let tile = scene_collision_tile(scene, tile_x, tile_y);
            if tile.solid {
                let candidate = fixed_point_from_pixels(tile_y * TILE_SIZE_PX - height_px);
                if !collided || candidate < clamped_y_fp {
                    clamped_y_fp = candidate;
                }
                collided = true;
                grounded = true;
            }
        }
    } else {
        let tile_y = top_px.div_euclid(TILE_SIZE_PX);
        for tile_x in left_tile..=right_tile {
            let tile = scene_collision_tile(scene, tile_x, tile_y);
            if tile.solid {
                let candidate = fixed_point_from_pixels((tile_y + 1) * TILE_SIZE_PX);
                if !collided || candidate > clamped_y_fp {
                    clamped_y_fp = candidate;
                }
                collided = true;
            }
        }
    }

    if !grounded {
        grounded = rect_overlaps_flag(
            scene,
            x_fp,
            clamped_y_fp + fixed_point_from_pixels(height_px),
            width_px,
            1,
            |tile| tile.solid,
        );
    }

    (clamped_y_fp, collided, grounded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use snesmaker_project::{CollisionLayer, GridSize, SceneKind, TileLayer};

    fn test_scene() -> SceneResource {
        let size_tiles = GridSize {
            width: 5,
            height: 5,
        };
        let tile_count = size_tiles.tile_count();
        let mut solids = vec![false; tile_count];
        let mut ladders = vec![false; tile_count];
        let mut hazards = vec![false; tile_count];

        for x in 0..size_tiles.width as usize {
            solids[(size_tiles.height as usize - 1) * size_tiles.width as usize + x] = true;
        }

        for y in 1..4 {
            ladders[y * size_tiles.width as usize + 1] = true;
        }

        hazards[2 * size_tiles.width as usize + 3] = true;
        for y in 0..size_tiles.height as usize {
            solids[y * size_tiles.width as usize + 2] = true;
        }

        SceneResource {
            id: "test".to_string(),
            kind: SceneKind::SideScroller,
            size_tiles,
            chunk_size_tiles: size_tiles,
            background_color_index: 0,
            layers: vec![TileLayer {
                id: "bg".to_string(),
                tileset_id: "tiles".to_string(),
                visible: true,
                parallax_x: 1,
                parallax_y: 1,
                tiles: vec![0; tile_count],
            }],
            collision: CollisionLayer {
                solids,
                ladders,
                hazards,
            },
            spawns: vec![SpawnPoint {
                id: "start".to_string(),
                position: PointI16 { x: 0, y: 8 },
            }],
            checkpoints: vec![Checkpoint {
                id: "hazard".to_string(),
                position: PointI16 { x: 24, y: 16 },
            }],
            entities: vec![],
            triggers: vec![],
            scripts: vec![],
        }
    }

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

    #[test]
    fn session_resets_from_spawn_and_checkpoint() {
        let scene = test_scene();
        let profile = snesmaker_project::default_megaman_like_physics();
        let mut session = PlaytestSession::new(&scene, profile);

        let state = session.state();
        assert_eq!(state.x_fp, fixed_point_from_pixels(0));
        assert_eq!(state.y_fp, fixed_point_from_pixels(8));
        assert!(state.on_ladder);
        assert!(!state.touching_hazard);

        let state = session
            .reset_to_checkpoint_id("hazard")
            .expect("checkpoint");
        assert_eq!(state.x_fp, fixed_point_from_pixels(24));
        assert_eq!(state.y_fp, fixed_point_from_pixels(16));
        assert!(state.touching_hazard);
    }

    #[test]
    fn session_steps_against_ladder_and_solid_tiles() {
        let scene = test_scene();
        let profile = snesmaker_project::default_megaman_like_physics();
        let mut session = PlaytestSession::new(&scene, profile);

        let climbed = session.step(InputFrame {
            climb_down: true,
            ..InputFrame::default()
        });
        assert!(climbed.on_ladder);
        assert!(climbed.y_fp > fixed_point_from_pixels(8));

        session.reset_to_position(PointI16 { x: 0, y: 0 });
        let blocked = session.step(InputFrame {
            right: true,
            ..InputFrame::default()
        });
        assert_eq!(blocked.x_fp, 0);
        assert_eq!(blocked.vx_fp, 0);
    }
}
