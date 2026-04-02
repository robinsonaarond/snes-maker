use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PlaytestStartMode {
    SceneStart,
    SelectedSpawn,
    SelectedCheckpoint,
}

impl PlaytestStartMode {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::SceneStart => "Scene Start",
            Self::SelectedSpawn => "Selected Spawn",
            Self::SelectedCheckpoint => "Selected Checkpoint",
        }
    }
}

pub(super) struct PlaytestState {
    pub(super) last_status: String,
    pub(super) session: Option<PlaytestSession>,
    pub(super) playing: bool,
    pub(super) speed_multiplier: f32,
    pub(super) accumulated_seconds: f32,
    pub(super) selected_physics_id: String,
    pub(super) start_mode: PlaytestStartMode,
    pub(super) show_camera_bounds: bool,
    pub(super) show_spawns: bool,
    pub(super) show_checkpoints: bool,
    pub(super) show_triggers: bool,
    pub(super) show_entities: bool,
}

impl Default for PlaytestState {
    fn default() -> Self {
        Self {
            last_status: String::new(),
            session: None,
            playing: false,
            speed_multiplier: 1.0,
            accumulated_seconds: 0.0,
            selected_physics_id: String::new(),
            start_mode: PlaytestStartMode::SceneStart,
            show_camera_bounds: true,
            show_spawns: true,
            show_checkpoints: true,
            show_triggers: true,
            show_entities: true,
        }
    }
}

impl EditorApp {
    pub(super) fn reset_playtest_session_impl(&mut self) {
        let Some(scene) = self.resolved_current_scene() else {
            self.playtest_state.session = None;
            return;
        };
        let Some(profile) = self.selected_physics_profile() else {
            self.playtest_state.session = None;
            return;
        };

        let profile_id = profile.id.clone();
        let mut session = PlaytestSession::new(&scene, profile);
        match self.playtest_state.start_mode {
            PlaytestStartMode::SceneStart => {
                session.reset_to_default_start();
                self.playtest_state.last_status =
                    format!("Started '{}' from its default start", scene.id);
            }
            PlaytestStartMode::SelectedSpawn => {
                if let Some(index) = self
                    .selected_spawn
                    .and_then(|index| scene.spawns.get(index))
                {
                    let _ = session.reset_to_spawn_id(&index.id);
                    self.playtest_state.last_status =
                        format!("Started '{}' from spawn '{}'", scene.id, index.id);
                } else {
                    session.reset_to_default_start();
                    self.playtest_state.last_status =
                        format!("Started '{}' from its default start", scene.id);
                }
            }
            PlaytestStartMode::SelectedCheckpoint => {
                if let Some(index) = self
                    .selected_checkpoint
                    .and_then(|index| scene.checkpoints.get(index))
                {
                    let _ = session.reset_to_checkpoint_id(&index.id);
                    self.playtest_state.last_status =
                        format!("Started '{}' from checkpoint '{}'", scene.id, index.id);
                } else {
                    session.reset_to_default_start();
                    self.playtest_state.last_status =
                        format!("Started '{}' from its default start", scene.id);
                }
            }
        }

        self.playtest_state.session = Some(session);
        self.playtest_state.playing = false;
        self.playtest_state.accumulated_seconds = 0.0;
        self.playtest_state.selected_physics_id = profile_id;
    }

    pub(super) fn step_playtest_session_impl(&mut self, input: InputFrame) {
        if self.playtest_state.session.is_none() {
            self.reset_playtest_session_impl();
        }
        if let Some(session) = &mut self.playtest_state.session {
            let state = session.step(input);
            self.playtest_state.last_status = format!(
                "Frame {}  x={} y={}  grounded={} ladder={} hazard={}",
                state.frame,
                state.x_fp >> snesmaker_project::FIXED_POINT_SHIFT,
                state.y_fp >> snesmaker_project::FIXED_POINT_SHIFT,
                state.grounded,
                state.on_ladder,
                state.touching_hazard
            );
        }
    }

    pub(super) fn draw_playtest_tab_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(bundle_snapshot) = self.bundle.clone() else {
            ui.heading("Playtest");
            ui.label("Load a project to use the in-editor playtest sandbox.");
            return;
        };

        let emulator = bundle_snapshot
            .manifest
            .editor
            .preferred_emulator
            .clone()
            .unwrap_or_else(|| "ares".to_string());
        let mut build = false;
        let mut build_and_launch = false;
        let mut refresh_report = false;
        let mut restart_session = false;
        let mut step_session = false;
        let mut duplicate_preset = false;
        let mut reset_preset = false;

        ui.heading("Playtest");
        ui.label(format!("Configured emulator: {}", emulator));
        ui.horizontal(|ui| {
            if ui.button("Build ROM").clicked() {
                build = true;
            }
            if ui.button("Build && Launch").clicked() {
                build_and_launch = true;
            }
            if ui.button("Refresh Build Report").clicked() {
                refresh_report = true;
            }
        });

        if build {
            self.build_current_rom();
        }
        if build_and_launch {
            self.build_and_launch_playtest();
        }
        if refresh_report {
            self.refresh_last_build_report();
        }

        if self.playtest_state.selected_physics_id.is_empty() {
            if let Some(preset) = bundle_snapshot.manifest.gameplay.physics_presets.first() {
                self.playtest_state.selected_physics_id = preset.id.clone();
            }
        }

        ui.separator();
        ui.horizontal(|ui| {
            egui::ComboBox::from_label("Physics Preset")
                .selected_text(if self.playtest_state.selected_physics_id.is_empty() {
                    "Choose preset"
                } else {
                    self.playtest_state.selected_physics_id.as_str()
                })
                .show_ui(ui, |ui| {
                    for preset in &bundle_snapshot.manifest.gameplay.physics_presets {
                        if ui
                            .selectable_value(
                                &mut self.playtest_state.selected_physics_id,
                                preset.id.clone(),
                                &preset.id,
                            )
                            .changed()
                        {
                            restart_session = true;
                        }
                    }
                });
            egui::ComboBox::from_label("Start")
                .selected_text(self.playtest_state.start_mode.label())
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_value(
                            &mut self.playtest_state.start_mode,
                            PlaytestStartMode::SceneStart,
                            PlaytestStartMode::SceneStart.label(),
                        )
                        .changed()
                    {
                        restart_session = true;
                    }
                    if ui
                        .selectable_value(
                            &mut self.playtest_state.start_mode,
                            PlaytestStartMode::SelectedSpawn,
                            PlaytestStartMode::SelectedSpawn.label(),
                        )
                        .changed()
                    {
                        restart_session = true;
                    }
                    if ui
                        .selectable_value(
                            &mut self.playtest_state.start_mode,
                            PlaytestStartMode::SelectedCheckpoint,
                            PlaytestStartMode::SelectedCheckpoint.label(),
                        )
                        .changed()
                    {
                        restart_session = true;
                    }
                });
            if ui
                .button(if self.playtest_state.playing {
                    "Pause"
                } else {
                    "Play"
                })
                .clicked()
            {
                self.playtest_state.playing = !self.playtest_state.playing;
            }
            if ui.button("Step").clicked() {
                step_session = true;
            }
            if ui.button("Restart").clicked() {
                restart_session = true;
            }
        });

        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::Slider::new(&mut self.playtest_state.speed_multiplier, 0.1..=2.0)
                    .text("Slow Motion"),
            );
            ui.checkbox(&mut self.show_collision, "Collision");
            ui.checkbox(&mut self.playtest_state.show_spawns, "Spawns");
            ui.checkbox(&mut self.playtest_state.show_checkpoints, "Checkpoints");
            ui.checkbox(&mut self.playtest_state.show_triggers, "Triggers");
            ui.checkbox(&mut self.playtest_state.show_entities, "Entities");
            ui.checkbox(&mut self.playtest_state.show_camera_bounds, "Camera Bounds");
        });

        let selected_profile_index = bundle_snapshot
            .manifest
            .gameplay
            .physics_presets
            .iter()
            .position(|preset| preset.id == self.playtest_state.selected_physics_id);
        if let Some(index) = selected_profile_index {
            let preset_snapshot = bundle_snapshot.manifest.gameplay.physics_presets[index].clone();
            let mut edited = preset_snapshot.clone();
            let mut changed = false;

            ui.separator();
            ui.collapsing("Physics Sandbox", |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Duplicate Preset").clicked() {
                        duplicate_preset = true;
                    }
                    if ui.button("Reset To Template").clicked() {
                        reset_preset = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Family");
                    egui::ComboBox::from_id_salt("physics_family")
                        .selected_text(format!("{:?}", edited.family))
                        .show_ui(ui, |ui| {
                            changed |= ui
                                .selectable_value(
                                    &mut edited.family,
                                    snesmaker_project::PhysicsFamily::MegaManLike,
                                    "Mega Man-like",
                                )
                                .changed();
                            changed |= ui
                                .selectable_value(
                                    &mut edited.family,
                                    snesmaker_project::PhysicsFamily::MarioLike,
                                    "Mario-like",
                                )
                                .changed();
                            changed |= ui
                                .selectable_value(
                                    &mut edited.family,
                                    snesmaker_project::PhysicsFamily::Custom,
                                    "Custom",
                                )
                                .changed();
                        });
                });
                for (label, value) in [
                    ("Gravity", &mut edited.gravity_fp),
                    ("Max Fall", &mut edited.max_fall_speed_fp),
                    ("Ground Accel", &mut edited.ground_accel_fp),
                    ("Air Accel", &mut edited.air_accel_fp),
                    ("Run Speed", &mut edited.max_run_speed_fp),
                    ("Jump Velocity", &mut edited.jump_velocity_fp),
                    ("Ladder Speed", &mut edited.ladder_speed_fp),
                ] {
                    ui.horizontal(|ui| {
                        ui.label(label);
                        changed |= ui.add(egui::DragValue::new(value).speed(4)).changed();
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("Coyote Frames");
                    changed |= ui
                        .add(egui::DragValue::new(&mut edited.coyote_frames).range(0..=16))
                        .changed();
                    ui.label("Jump Buffer");
                    changed |= ui
                        .add(egui::DragValue::new(&mut edited.jump_buffer_frames).range(0..=16))
                        .changed();
                });
            });

            if changed {
                self.capture_history();
                if let Some(bundle) = &mut self.bundle {
                    if let Some(target) = bundle
                        .manifest
                        .gameplay
                        .physics_presets
                        .iter_mut()
                        .find(|preset| preset.id == preset_snapshot.id)
                    {
                        *target = edited.clone();
                    }
                }
                self.playtest_state.selected_physics_id = edited.id.clone();
                self.mark_edited(format!("Updated physics preset '{}'", edited.id));
                restart_session = true;
            }

            if duplicate_preset {
                self.capture_history();
                let mut duplicated_label = None;
                if let Some(bundle) = &mut self.bundle {
                    let existing = bundle
                        .manifest
                        .gameplay
                        .physics_presets
                        .iter()
                        .map(|preset| preset.id.clone())
                        .collect::<BTreeSet<_>>();
                    let mut duplicate = preset_snapshot.clone();
                    duplicate.id = next_unique_layer_id(&existing, &preset_snapshot.id);
                    self.playtest_state.selected_physics_id = duplicate.id.clone();
                    bundle
                        .manifest
                        .gameplay
                        .physics_presets
                        .push(duplicate.clone());
                    duplicated_label = Some(duplicate.id);
                    restart_session = true;
                }
                if let Some(label) = duplicated_label {
                    self.mark_edited(format!("Duplicated physics preset '{}'", label));
                }
            }

            if reset_preset {
                self.capture_history();
                let mut reset_label = None;
                if let Some(bundle) = &mut self.bundle {
                    if let Some(target) = bundle
                        .manifest
                        .gameplay
                        .physics_presets
                        .iter_mut()
                        .find(|preset| preset.id == preset_snapshot.id)
                    {
                        *target = template_physics_profile(target.family, target.id.clone());
                        reset_label = Some(target.id.clone());
                        restart_session = true;
                    }
                }
                if let Some(label) = reset_label {
                    self.mark_edited(format!("Reset physics preset '{}'", label));
                }
            }
        }

        if restart_session {
            self.reset_playtest_session_impl();
        }

        let input = input_frame_from_context(ctx);
        if self.playtest_state.playing {
            self.playtest_state.accumulated_seconds +=
                ctx.input(|input| input.stable_dt) * self.playtest_state.speed_multiplier;
            while self.playtest_state.accumulated_seconds >= (1.0 / 60.0) {
                self.step_playtest_session_impl(input);
                self.playtest_state.accumulated_seconds -= 1.0 / 60.0;
            }
            ctx.request_repaint();
        } else if step_session {
            self.step_playtest_session_impl(input);
        }

        if !self.playtest_state.last_status.is_empty() {
            ui.separator();
            ui.label(&self.playtest_state.last_status);
        }

        if let Some(profile) = self.selected_physics_profile() {
            ui.separator();
            ui.collapsing("Movement Trace", |ui| {
                let trace = simulate_trace(&profile, &sample_platformer_trace_input());
                draw_trace_chart(ui, &trace, "Y Position", |frame| frame.y_fp);
                draw_trace_chart(ui, &trace, "Horizontal Speed", |frame| frame.vx_fp);
                ui.label(format!(
                    "Preset '{}'  |  coyote={}  jump buffer={}  ladder={}",
                    profile.id,
                    profile.coyote_frames,
                    profile.jump_buffer_frames,
                    profile.ladder_speed_fp
                ));
            });
        }

        ui.separator();
        ui.heading("In-Editor Sandbox");
        let playtest_zoom = 6.0;
        let playtest_viewport = Vec2::new(ui.available_width(), 280.0);
        let mut camera_offset = Vec2::ZERO;
        if let (Some(scene), Some(state)) = (
            bundle_snapshot.resolved_scene_by_index(self.selected_scene),
            self.playtest_state
                .session
                .as_ref()
                .map(|session| session.state()),
        ) {
            let focus_rect = RectI16 {
                x: (state.x_fp >> snesmaker_project::FIXED_POINT_SHIFT) as i16,
                y: (state.y_fp >> snesmaker_project::FIXED_POINT_SHIFT) as i16,
                width: 16,
                height: 16,
            };
            camera_offset = focus_offset_for_rect(
                focus_rect,
                playtest_zoom,
                playtest_viewport,
                Vec2::new(
                    scene.size_tiles.width as f32 * 8.0 * playtest_zoom,
                    scene.size_tiles.height as f32 * 8.0 * playtest_zoom,
                ),
            );
        }

        let outcome = draw_scene_canvas(
            ui,
            &bundle_snapshot,
            self.selected_scene,
            playtest_zoom,
            camera_offset,
            playtest_viewport,
            self.show_grid,
            self.show_collision,
            self.selected_layer,
            self.selected_tile,
            self.selected_spawn,
            self.selected_checkpoint,
            self.selected_entity,
            self.selected_trigger,
            self.selected_prefab_instance,
            None,
            None,
            false,
            None,
            None,
            self.playtest_state.show_spawns,
            self.playtest_state.show_checkpoints,
            self.playtest_state.show_entities,
            self.playtest_state.show_triggers,
            ctx.input(|input| input.time) as f32,
        );

        if let Some(session) = &self.playtest_state.session {
            let state = session.state();
            let painter = ui.painter_at(outcome.viewport_rect);
            let origin = outcome.viewport_rect.min - camera_offset;
            let player_center = origin
                + Vec2::new(
                    (state.x_fp >> snesmaker_project::FIXED_POINT_SHIFT) as f32 * playtest_zoom
                        + 8.0 * playtest_zoom,
                    (state.y_fp >> snesmaker_project::FIXED_POINT_SHIFT) as f32 * playtest_zoom
                        + 8.0 * playtest_zoom,
                );
            painter.circle_filled(player_center, 8.0, Color32::from_rgb(255, 112, 96));
            painter.circle_stroke(player_center, 10.0, (2.0, Color32::WHITE));

            if self.playtest_state.show_camera_bounds {
                painter.rect_stroke(
                    outcome.viewport_rect,
                    6.0,
                    (2.0, Color32::from_rgb(255, 255, 255)),
                    StrokeKind::Inside,
                );
            }

            if let Some(scene) = bundle_snapshot.resolved_scene_by_index(self.selected_scene) {
                let player_rect = RectI16 {
                    x: (state.x_fp >> snesmaker_project::FIXED_POINT_SHIFT) as i16,
                    y: (state.y_fp >> snesmaker_project::FIXED_POINT_SHIFT) as i16,
                    width: 16,
                    height: 16,
                };
                let active_triggers = scene
                    .triggers
                    .iter()
                    .filter(|trigger| rects_overlap_pixels(player_rect, trigger.rect))
                    .map(|trigger| trigger.id.clone())
                    .collect::<Vec<_>>();
                ui.label(format!(
                    "Player: x={} y={} grounded={} ladder={} hazard={}  |  Active triggers: {}",
                    state.x_fp >> snesmaker_project::FIXED_POINT_SHIFT,
                    state.y_fp >> snesmaker_project::FIXED_POINT_SHIFT,
                    state.grounded,
                    state.on_ladder,
                    state.touching_hazard,
                    if active_triggers.is_empty() {
                        "none".to_string()
                    } else {
                        active_triggers.join(", ")
                    }
                ));
                ui.label(format!(
                    "Entity state: {} active / {} inactive",
                    scene.entities.iter().filter(|entity| entity.active).count(),
                    scene
                        .entities
                        .iter()
                        .filter(|entity| !entity.active)
                        .count()
                ));
            }
        } else {
            ui.label("Restart the session to begin simulating the current scene.");
        }

        if let Some(outcome) = &self.last_build_outcome {
            ui.separator();
            ui.label(format!("Last ROM path: {}", outcome.rom_path));
            if !outcome.rom_built {
                ui.label(
                    "A playable ROM was not produced yet. Build assets are still available for inspection.",
                );
            }
        } else {
            ui.separator();
            ui.label("Build the project to generate a ROM and launch a playtest session.");
        }
    }
}
