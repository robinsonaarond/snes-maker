use super::*;

impl EditorApp {
    pub(super) fn draw_scene_outliner_panel(&mut self, ui: &mut egui::Ui) {
        ui.add(
            egui::TextEdit::singleline(&mut self.outliner_filter)
                .hint_text("Filter scenes, layers, and objects"),
        );
        ui.add_space(6.0);

        ui.horizontal_wrapped(|ui| {
            if let Some((scene_index, layer_index)) = self.solo_layer {
                let label = self
                    .layer(scene_index, layer_index)
                    .map(|layer| format!("Solo layer: {}", layer.id))
                    .unwrap_or_else(|| "Solo layer".to_string());
                ui.label(label);
                if ui.small_button("Clear").clicked() {
                    self.solo_layer = None;
                    self.status = "Cleared layer solo".to_string();
                }
            }
            if let Some(group) = self.solo_group {
                ui.label(format!("Solo group: {}", group.label()));
                if ui.small_button("Clear").clicked() {
                    self.solo_group = None;
                    self.status = "Cleared object-group solo".to_string();
                }
            }
        });
        ui.add_space(4.0);

        let Some(bundle) = self.bundle.clone() else {
            ui.label("No project loaded.");
            return;
        };

        enum PendingOutlinerAction {
            Scene {
                scene_index: usize,
                label: String,
            },
            SceneFocus {
                scene_index: usize,
            },
            Layer {
                scene_index: usize,
                layer_index: usize,
                label: String,
            },
            LayerVisibility {
                scene_index: usize,
                layer_index: usize,
            },
            LayerLock {
                scene_index: usize,
                layer_index: usize,
            },
            LayerSolo {
                scene_index: usize,
                layer_index: usize,
            },
            LayerFocus {
                scene_index: usize,
                layer_index: usize,
            },
            LayerDuplicate {
                scene_index: usize,
                layer_index: usize,
            },
            GroupSolo(SceneObjectGroup),
            Spawn {
                scene_index: usize,
                index: usize,
                label: String,
            },
            SpawnFocus {
                scene_index: usize,
                index: usize,
            },
            SpawnDuplicate {
                scene_index: usize,
                index: usize,
            },
            SpawnIsolate {
                scene_index: usize,
                index: usize,
            },
            Checkpoint {
                scene_index: usize,
                index: usize,
                label: String,
            },
            CheckpointFocus {
                scene_index: usize,
                index: usize,
            },
            CheckpointDuplicate {
                scene_index: usize,
                index: usize,
            },
            CheckpointIsolate {
                scene_index: usize,
                index: usize,
            },
            Entity {
                scene_index: usize,
                index: usize,
                label: String,
            },
            EntityFocus {
                scene_index: usize,
                index: usize,
            },
            EntityDuplicate {
                scene_index: usize,
                index: usize,
            },
            EntityIsolate {
                scene_index: usize,
                index: usize,
            },
            Trigger {
                scene_index: usize,
                index: usize,
                label: String,
            },
            TriggerFocus {
                scene_index: usize,
                index: usize,
            },
            TriggerDuplicate {
                scene_index: usize,
                index: usize,
            },
            TriggerIsolate {
                scene_index: usize,
                index: usize,
            },
            Script {
                scene_index: usize,
                script_index: usize,
                label: String,
            },
            ScriptIsolate {
                scene_index: usize,
                script_index: usize,
                label: String,
            },
        }

        let filter = self.outliner_filter.trim().to_ascii_lowercase();
        let mut pending_action = None;

        egui::ScrollArea::vertical().show(ui, |ui| {
            for (scene_index, scene) in bundle.scenes.iter().enumerate() {
                let scene_matches = filter_matches(&filter, &scene.id);
                let child_matches = scene
                    .layers
                    .iter()
                    .any(|layer| filter_matches(&filter, &layer.id))
                    || scene
                        .spawns
                        .iter()
                        .any(|spawn| filter_matches(&filter, &spawn.id))
                    || scene
                        .checkpoints
                        .iter()
                        .any(|checkpoint| filter_matches(&filter, &checkpoint.id))
                    || scene
                        .entities
                        .iter()
                        .any(|entity| filter_matches(&filter, &entity.id))
                    || scene
                        .triggers
                        .iter()
                        .any(|trigger| filter_matches(&filter, &trigger.id))
                    || scene
                        .scripts
                        .iter()
                        .any(|script| filter_matches(&filter, &script.id));

                if !filter.is_empty() && !scene_matches && !child_matches {
                    continue;
                }

                egui::CollapsingHeader::new(format!("{} ({:?})", scene.id, scene.kind))
                    .default_open(self.selected_scene == scene_index)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui
                                .selectable_label(
                                    self.selected_scene == scene_index,
                                    format!(
                                        "Open scene ({}, {} entities, {} triggers)",
                                        scene.layers.len(),
                                        scene.entities.len(),
                                        scene.triggers.len()
                                    ),
                                )
                                .clicked()
                            {
                                pending_action = Some(PendingOutlinerAction::Scene {
                                    scene_index,
                                    label: scene.id.clone(),
                                });
                            }
                            if ui.small_button("Focus").clicked() {
                                pending_action =
                                    Some(PendingOutlinerAction::SceneFocus { scene_index });
                            }
                        });

                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label("Layers");
                            if ui
                                .small_button(
                                    if self.is_layer_soloed(scene_index, self.selected_layer) {
                                        "Clear Solo"
                                    } else {
                                        "Solo Active"
                                    },
                                )
                                .clicked()
                            {
                                pending_action = Some(PendingOutlinerAction::LayerSolo {
                                    scene_index,
                                    layer_index: self
                                        .selected_layer
                                        .min(scene.layers.len().saturating_sub(1)),
                                });
                            }
                        });
                        for (layer_index, layer) in scene.layers.iter().enumerate() {
                            if !filter_matches(&filter, &layer.id) && !filter.is_empty() {
                                continue;
                            }
                            ui.horizontal(|ui| {
                                if ui
                                    .selectable_label(
                                        self.selected_scene == scene_index
                                            && self.selected_layer == layer_index,
                                        format!(
                                            "{}  [{} | parallax {}:{}]",
                                            layer.id,
                                            if layer.visible { "visible" } else { "hidden" },
                                            layer.parallax_x,
                                            layer.parallax_y
                                        ),
                                    )
                                    .clicked()
                                {
                                    pending_action = Some(PendingOutlinerAction::Layer {
                                        scene_index,
                                        layer_index,
                                        label: layer.id.clone(),
                                    });
                                }
                                if ui
                                    .small_button(if layer.visible { "Hide" } else { "Show" })
                                    .clicked()
                                {
                                    pending_action = Some(PendingOutlinerAction::LayerVisibility {
                                        scene_index,
                                        layer_index,
                                    });
                                }
                                if ui
                                    .small_button(
                                        if self.is_layer_locked(scene_index, layer_index) {
                                            "Unlock"
                                        } else {
                                            "Lock"
                                        },
                                    )
                                    .clicked()
                                {
                                    pending_action = Some(PendingOutlinerAction::LayerLock {
                                        scene_index,
                                        layer_index,
                                    });
                                }
                                if ui
                                    .small_button(
                                        if self.is_layer_soloed(scene_index, layer_index) {
                                            "Unsolo"
                                        } else {
                                            "Solo"
                                        },
                                    )
                                    .clicked()
                                {
                                    pending_action = Some(PendingOutlinerAction::LayerSolo {
                                        scene_index,
                                        layer_index,
                                    });
                                }
                                if ui.small_button("Focus").clicked() {
                                    pending_action = Some(PendingOutlinerAction::LayerFocus {
                                        scene_index,
                                        layer_index,
                                    });
                                }
                                if ui.small_button("Dup").clicked() {
                                    pending_action = Some(PendingOutlinerAction::LayerDuplicate {
                                        scene_index,
                                        layer_index,
                                    });
                                }
                            });
                        }

                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(format!(
                                "{} ({})",
                                SceneObjectGroup::Spawns.label(),
                                scene.spawns.len()
                            ));
                            if ui
                                .small_button(if self.is_group_soloed(SceneObjectGroup::Spawns) {
                                    "Unsolo"
                                } else {
                                    "Solo"
                                })
                                .clicked()
                            {
                                pending_action = Some(PendingOutlinerAction::GroupSolo(
                                    SceneObjectGroup::Spawns,
                                ));
                            }
                        });
                        for (index, spawn) in scene.spawns.iter().enumerate() {
                            if !filter_matches(&filter, &spawn.id) && !filter.is_empty() {
                                continue;
                            }
                            ui.horizontal(|ui| {
                                if ui
                                    .selectable_label(
                                        self.selected_scene == scene_index
                                            && self.selected_spawn == Some(index),
                                        format!("Spawn: {}", spawn.id),
                                    )
                                    .clicked()
                                {
                                    pending_action = Some(PendingOutlinerAction::Spawn {
                                        scene_index,
                                        index,
                                        label: spawn.id.clone(),
                                    });
                                }
                                if ui.small_button("Focus").clicked() {
                                    pending_action = Some(PendingOutlinerAction::SpawnFocus {
                                        scene_index,
                                        index,
                                    });
                                }
                                if ui.small_button("Dup").clicked() {
                                    pending_action = Some(PendingOutlinerAction::SpawnDuplicate {
                                        scene_index,
                                        index,
                                    });
                                }
                                if ui.small_button("Isolate").clicked() {
                                    pending_action = Some(PendingOutlinerAction::SpawnIsolate {
                                        scene_index,
                                        index,
                                    });
                                }
                            });
                        }
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(format!(
                                "{} ({})",
                                SceneObjectGroup::Checkpoints.label(),
                                scene.checkpoints.len()
                            ));
                            if ui
                                .small_button(
                                    if self.is_group_soloed(SceneObjectGroup::Checkpoints) {
                                        "Unsolo"
                                    } else {
                                        "Solo"
                                    },
                                )
                                .clicked()
                            {
                                pending_action = Some(PendingOutlinerAction::GroupSolo(
                                    SceneObjectGroup::Checkpoints,
                                ));
                            }
                        });
                        for (index, checkpoint) in scene.checkpoints.iter().enumerate() {
                            if !filter_matches(&filter, &checkpoint.id) && !filter.is_empty() {
                                continue;
                            }
                            ui.horizontal(|ui| {
                                if ui
                                    .selectable_label(
                                        self.selected_scene == scene_index
                                            && self.selected_checkpoint == Some(index),
                                        format!("Checkpoint: {}", checkpoint.id),
                                    )
                                    .clicked()
                                {
                                    pending_action = Some(PendingOutlinerAction::Checkpoint {
                                        scene_index,
                                        index,
                                        label: checkpoint.id.clone(),
                                    });
                                }
                                if ui.small_button("Focus").clicked() {
                                    pending_action = Some(PendingOutlinerAction::CheckpointFocus {
                                        scene_index,
                                        index,
                                    });
                                }
                                if ui.small_button("Dup").clicked() {
                                    pending_action =
                                        Some(PendingOutlinerAction::CheckpointDuplicate {
                                            scene_index,
                                            index,
                                        });
                                }
                                if ui.small_button("Isolate").clicked() {
                                    pending_action =
                                        Some(PendingOutlinerAction::CheckpointIsolate {
                                            scene_index,
                                            index,
                                        });
                                }
                            });
                        }
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(format!(
                                "{} ({})",
                                SceneObjectGroup::Entities.label(),
                                scene.entities.len()
                            ));
                            if ui
                                .small_button(if self.is_group_soloed(SceneObjectGroup::Entities) {
                                    "Unsolo"
                                } else {
                                    "Solo"
                                })
                                .clicked()
                            {
                                pending_action = Some(PendingOutlinerAction::GroupSolo(
                                    SceneObjectGroup::Entities,
                                ));
                            }
                        });
                        for (index, entity) in scene.entities.iter().enumerate() {
                            if !filter_matches(&filter, &entity.id) && !filter.is_empty() {
                                continue;
                            }
                            ui.horizontal(|ui| {
                                if ui
                                    .selectable_label(
                                        self.selected_scene == scene_index
                                            && self.selected_entity == Some(index),
                                        format!("Entity: {} ({})", entity.id, entity.archetype),
                                    )
                                    .clicked()
                                {
                                    pending_action = Some(PendingOutlinerAction::Entity {
                                        scene_index,
                                        index,
                                        label: entity.id.clone(),
                                    });
                                }
                                if ui.small_button("Focus").clicked() {
                                    pending_action = Some(PendingOutlinerAction::EntityFocus {
                                        scene_index,
                                        index,
                                    });
                                }
                                if ui.small_button("Dup").clicked() {
                                    pending_action = Some(PendingOutlinerAction::EntityDuplicate {
                                        scene_index,
                                        index,
                                    });
                                }
                                if ui.small_button("Isolate").clicked() {
                                    pending_action = Some(PendingOutlinerAction::EntityIsolate {
                                        scene_index,
                                        index,
                                    });
                                }
                            });
                        }
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(format!(
                                "{} ({})",
                                SceneObjectGroup::Triggers.label(),
                                scene.triggers.len()
                            ));
                            if ui
                                .small_button(if self.is_group_soloed(SceneObjectGroup::Triggers) {
                                    "Unsolo"
                                } else {
                                    "Solo"
                                })
                                .clicked()
                            {
                                pending_action = Some(PendingOutlinerAction::GroupSolo(
                                    SceneObjectGroup::Triggers,
                                ));
                            }
                        });
                        for (index, trigger) in scene.triggers.iter().enumerate() {
                            if !filter_matches(&filter, &trigger.id) && !filter.is_empty() {
                                continue;
                            }
                            ui.horizontal(|ui| {
                                if ui
                                    .selectable_label(
                                        self.selected_scene == scene_index
                                            && self.selected_trigger == Some(index),
                                        format!("Trigger: {} ({:?})", trigger.id, trigger.kind),
                                    )
                                    .clicked()
                                {
                                    pending_action = Some(PendingOutlinerAction::Trigger {
                                        scene_index,
                                        index,
                                        label: trigger.id.clone(),
                                    });
                                }
                                if ui.small_button("Focus").clicked() {
                                    pending_action = Some(PendingOutlinerAction::TriggerFocus {
                                        scene_index,
                                        index,
                                    });
                                }
                                if ui.small_button("Dup").clicked() {
                                    pending_action =
                                        Some(PendingOutlinerAction::TriggerDuplicate {
                                            scene_index,
                                            index,
                                        });
                                }
                                if ui.small_button("Isolate").clicked() {
                                    pending_action = Some(PendingOutlinerAction::TriggerIsolate {
                                        scene_index,
                                        index,
                                    });
                                }
                            });
                        }

                        if !scene.scripts.is_empty() {
                            ui.separator();
                            ui.horizontal(|ui| {
                                ui.label(format!(
                                    "{} ({})",
                                    SceneObjectGroup::Scripts.label(),
                                    scene.scripts.len()
                                ));
                                if ui
                                    .small_button(
                                        if self.is_group_soloed(SceneObjectGroup::Scripts) {
                                            "Unsolo"
                                        } else {
                                            "Solo"
                                        },
                                    )
                                    .clicked()
                                {
                                    pending_action = Some(PendingOutlinerAction::GroupSolo(
                                        SceneObjectGroup::Scripts,
                                    ));
                                }
                            });
                            for (script_index, script) in scene.scripts.iter().enumerate() {
                                if !filter_matches(&filter, &script.id) && !filter.is_empty() {
                                    continue;
                                }
                                ui.horizontal(|ui| {
                                    if ui
                                        .selectable_label(false, format!("Script: {}", script.id))
                                        .clicked()
                                    {
                                        pending_action = Some(PendingOutlinerAction::Script {
                                            scene_index,
                                            script_index,
                                            label: script.id.clone(),
                                        });
                                    }
                                    if ui.small_button("Isolate").clicked() {
                                        pending_action =
                                            Some(PendingOutlinerAction::ScriptIsolate {
                                                scene_index,
                                                script_index,
                                                label: script.id.clone(),
                                            });
                                    }
                                });
                            }
                        }
                    });
            }
        });

        if let Some(action) = pending_action {
            match action {
                PendingOutlinerAction::Scene { scene_index, label } => {
                    self.selected_scene = scene_index;
                    self.scene_scroll_offset = Vec2::ZERO;
                    self.clear_selection();
                    self.preview_focus = PreviewFocus::None;
                    self.sync_selection();
                    self.status = format!("Selected scene '{}'", label);
                }
                PendingOutlinerAction::SceneFocus { scene_index } => {
                    self.selected_scene = scene_index;
                    self.sync_selection();
                    self.focus_scene(scene_index);
                }
                PendingOutlinerAction::Layer {
                    scene_index,
                    layer_index,
                    label,
                } => {
                    self.select_layer(scene_index, layer_index);
                    self.status = format!("Active layer: '{}'", label);
                }
                PendingOutlinerAction::LayerVisibility {
                    scene_index,
                    layer_index,
                } => {
                    self.toggle_layer_visibility(scene_index, layer_index);
                }
                PendingOutlinerAction::LayerLock {
                    scene_index,
                    layer_index,
                } => {
                    self.toggle_layer_lock(scene_index, layer_index);
                }
                PendingOutlinerAction::LayerSolo {
                    scene_index,
                    layer_index,
                } => {
                    self.toggle_layer_solo(scene_index, layer_index);
                }
                PendingOutlinerAction::LayerFocus {
                    scene_index,
                    layer_index,
                } => {
                    self.select_layer(scene_index, layer_index);
                    self.focus_layer(scene_index, layer_index);
                }
                PendingOutlinerAction::LayerDuplicate {
                    scene_index,
                    layer_index,
                } => {
                    self.duplicate_layer(scene_index, layer_index);
                }
                PendingOutlinerAction::GroupSolo(group) => {
                    self.toggle_group_solo(group);
                }
                PendingOutlinerAction::Spawn {
                    scene_index,
                    index,
                    label,
                } => {
                    self.selected_scene = scene_index;
                    self.scene_scroll_offset = Vec2::ZERO;
                    self.sync_selection();
                    self.selected_spawn = Some(index);
                    self.selected_checkpoint = None;
                    self.selected_entity = None;
                    self.selected_trigger = None;
                    self.tool = EditorTool::Spawn;
                    self.preview_focus = PreviewFocus::None;
                    self.status = format!("Selected spawn '{}'", label);
                }
                PendingOutlinerAction::SpawnFocus { scene_index, index } => {
                    let spawn = self
                        .bundle
                        .as_ref()
                        .and_then(|bundle| bundle.scenes.get(scene_index))
                        .and_then(|scene| scene.spawns.get(index))
                        .map(|spawn| (spawn.id.clone(), spawn.position));
                    if let Some((label, position)) = spawn {
                        self.selected_scene = scene_index;
                        self.sync_selection();
                        self.selected_spawn = Some(index);
                        self.tool = EditorTool::Spawn;
                        self.focus_point(&format!("spawn '{}'", label), position);
                    }
                }
                PendingOutlinerAction::SpawnDuplicate { scene_index, index } => {
                    let duplicate = self
                        .bundle
                        .as_ref()
                        .and_then(|bundle| bundle.scenes.get(scene_index))
                        .and_then(|scene| scene.spawns.get(index).cloned())
                        .map(|mut spawn| {
                            let existing = self
                                .bundle
                                .as_ref()
                                .and_then(|bundle| bundle.scenes.get(scene_index))
                                .map(|scene| {
                                    scene
                                        .spawns
                                        .iter()
                                        .map(|entry| entry.id.clone())
                                        .collect::<BTreeSet<_>>()
                                })
                                .unwrap_or_default();
                            spawn.id = next_unique_layer_id(&existing, &spawn.id);
                            spawn.position.x += 16;
                            spawn
                        });
                    if let Some(duplicate) = duplicate {
                        self.capture_history();
                        if let Some(scene) = self
                            .bundle
                            .as_mut()
                            .and_then(|bundle| bundle.scenes.get_mut(scene_index))
                        {
                            let insert_at = (index + 1).min(scene.spawns.len());
                            let label = duplicate.id.clone();
                            scene.spawns.insert(insert_at, duplicate);
                            self.selected_scene = scene_index;
                            self.selected_spawn = Some(insert_at);
                            self.tool = EditorTool::Spawn;
                            self.mark_edited(format!("Duplicated spawn '{}'", label));
                        }
                    }
                }
                PendingOutlinerAction::SpawnIsolate { scene_index, index } => {
                    self.solo_group = Some(SceneObjectGroup::Spawns);
                    let spawn = self
                        .bundle
                        .as_ref()
                        .and_then(|bundle| bundle.scenes.get(scene_index))
                        .and_then(|scene| scene.spawns.get(index))
                        .map(|spawn| (spawn.id.clone(), spawn.position));
                    if let Some((label, position)) = spawn {
                        self.selected_scene = scene_index;
                        self.selected_spawn = Some(index);
                        self.tool = EditorTool::Spawn;
                        self.focus_point(&format!("spawn '{}'", label), position);
                    }
                }
                PendingOutlinerAction::Checkpoint {
                    scene_index,
                    index,
                    label,
                } => {
                    self.selected_scene = scene_index;
                    self.scene_scroll_offset = Vec2::ZERO;
                    self.sync_selection();
                    self.selected_spawn = None;
                    self.selected_checkpoint = Some(index);
                    self.selected_entity = None;
                    self.selected_trigger = None;
                    self.tool = EditorTool::Checkpoint;
                    self.preview_focus = PreviewFocus::None;
                    self.status = format!("Selected checkpoint '{}'", label);
                }
                PendingOutlinerAction::CheckpointFocus { scene_index, index } => {
                    let checkpoint = self
                        .bundle
                        .as_ref()
                        .and_then(|bundle| bundle.scenes.get(scene_index))
                        .and_then(|scene| scene.checkpoints.get(index))
                        .map(|checkpoint| (checkpoint.id.clone(), checkpoint.position));
                    if let Some((label, position)) = checkpoint {
                        self.selected_scene = scene_index;
                        self.sync_selection();
                        self.selected_checkpoint = Some(index);
                        self.tool = EditorTool::Checkpoint;
                        self.focus_point(&format!("checkpoint '{}'", label), position);
                    }
                }
                PendingOutlinerAction::CheckpointDuplicate { scene_index, index } => {
                    let duplicate = self
                        .bundle
                        .as_ref()
                        .and_then(|bundle| bundle.scenes.get(scene_index))
                        .and_then(|scene| scene.checkpoints.get(index).cloned())
                        .map(|mut checkpoint| {
                            let existing = self
                                .bundle
                                .as_ref()
                                .and_then(|bundle| bundle.scenes.get(scene_index))
                                .map(|scene| {
                                    scene
                                        .checkpoints
                                        .iter()
                                        .map(|entry| entry.id.clone())
                                        .collect::<BTreeSet<_>>()
                                })
                                .unwrap_or_default();
                            checkpoint.id = next_unique_layer_id(&existing, &checkpoint.id);
                            checkpoint.position.x += 16;
                            checkpoint
                        });
                    if let Some(duplicate) = duplicate {
                        self.capture_history();
                        if let Some(scene) = self
                            .bundle
                            .as_mut()
                            .and_then(|bundle| bundle.scenes.get_mut(scene_index))
                        {
                            let insert_at = (index + 1).min(scene.checkpoints.len());
                            let label = duplicate.id.clone();
                            scene.checkpoints.insert(insert_at, duplicate);
                            self.selected_scene = scene_index;
                            self.selected_checkpoint = Some(insert_at);
                            self.tool = EditorTool::Checkpoint;
                            self.mark_edited(format!("Duplicated checkpoint '{}'", label));
                        }
                    }
                }
                PendingOutlinerAction::CheckpointIsolate { scene_index, index } => {
                    self.solo_group = Some(SceneObjectGroup::Checkpoints);
                    let checkpoint = self
                        .bundle
                        .as_ref()
                        .and_then(|bundle| bundle.scenes.get(scene_index))
                        .and_then(|scene| scene.checkpoints.get(index))
                        .map(|checkpoint| (checkpoint.id.clone(), checkpoint.position));
                    if let Some((label, position)) = checkpoint {
                        self.selected_scene = scene_index;
                        self.selected_checkpoint = Some(index);
                        self.tool = EditorTool::Checkpoint;
                        self.focus_point(&format!("checkpoint '{}'", label), position);
                    }
                }
                PendingOutlinerAction::Entity {
                    scene_index,
                    index,
                    label,
                } => {
                    self.selected_scene = scene_index;
                    self.scene_scroll_offset = Vec2::ZERO;
                    self.sync_selection();
                    self.selected_spawn = None;
                    self.selected_checkpoint = None;
                    self.selected_entity = Some(index);
                    self.selected_trigger = None;
                    self.tool = EditorTool::Entity;
                    self.preview_focus = PreviewFocus::Entity;
                    self.status = format!("Selected entity '{}'", label);
                }
                PendingOutlinerAction::EntityFocus { scene_index, index } => {
                    let entity = self
                        .bundle
                        .as_ref()
                        .and_then(|bundle| bundle.scenes.get(scene_index))
                        .and_then(|scene| scene.entities.get(index))
                        .map(|entity| (entity.id.clone(), entity.position));
                    if let Some((label, position)) = entity {
                        self.selected_scene = scene_index;
                        self.sync_selection();
                        self.selected_entity = Some(index);
                        self.tool = EditorTool::Entity;
                        self.preview_focus = PreviewFocus::Entity;
                        self.focus_point(&format!("entity '{}'", label), position);
                    }
                }
                PendingOutlinerAction::EntityDuplicate { scene_index, index } => {
                    let duplicate = self
                        .bundle
                        .as_ref()
                        .and_then(|bundle| bundle.scenes.get(scene_index))
                        .and_then(|scene| scene.entities.get(index).cloned())
                        .map(|mut entity| {
                            let existing = self
                                .bundle
                                .as_ref()
                                .and_then(|bundle| bundle.scenes.get(scene_index))
                                .map(|scene| {
                                    scene
                                        .entities
                                        .iter()
                                        .map(|entry| entry.id.clone())
                                        .collect::<BTreeSet<_>>()
                                })
                                .unwrap_or_default();
                            entity.id = next_unique_layer_id(&existing, &entity.id);
                            entity.position.x += 16;
                            entity
                        });
                    if let Some(duplicate) = duplicate {
                        self.capture_history();
                        if let Some(scene) = self
                            .bundle
                            .as_mut()
                            .and_then(|bundle| bundle.scenes.get_mut(scene_index))
                        {
                            let insert_at = (index + 1).min(scene.entities.len());
                            let label = duplicate.id.clone();
                            scene.entities.insert(insert_at, duplicate);
                            self.selected_scene = scene_index;
                            self.selected_entity = Some(insert_at);
                            self.tool = EditorTool::Entity;
                            self.preview_focus = PreviewFocus::Entity;
                            self.mark_edited(format!("Duplicated entity '{}'", label));
                        }
                    }
                }
                PendingOutlinerAction::EntityIsolate { scene_index, index } => {
                    self.solo_group = Some(SceneObjectGroup::Entities);
                    let entity = self
                        .bundle
                        .as_ref()
                        .and_then(|bundle| bundle.scenes.get(scene_index))
                        .and_then(|scene| scene.entities.get(index))
                        .map(|entity| (entity.id.clone(), entity.position));
                    if let Some((label, position)) = entity {
                        self.selected_scene = scene_index;
                        self.selected_entity = Some(index);
                        self.tool = EditorTool::Entity;
                        self.preview_focus = PreviewFocus::Entity;
                        self.focus_point(&format!("entity '{}'", label), position);
                    }
                }
                PendingOutlinerAction::Trigger {
                    scene_index,
                    index,
                    label,
                } => {
                    self.selected_scene = scene_index;
                    self.scene_scroll_offset = Vec2::ZERO;
                    self.sync_selection();
                    self.selected_spawn = None;
                    self.selected_checkpoint = None;
                    self.selected_entity = None;
                    self.selected_trigger = Some(index);
                    self.tool = EditorTool::Trigger;
                    self.preview_focus = PreviewFocus::None;
                    self.status = format!("Selected trigger '{}'", label);
                }
                PendingOutlinerAction::TriggerFocus { scene_index, index } => {
                    let trigger = self
                        .bundle
                        .as_ref()
                        .and_then(|bundle| bundle.scenes.get(scene_index))
                        .and_then(|scene| scene.triggers.get(index))
                        .map(|trigger| (trigger.id.clone(), trigger.rect));
                    if let Some((label, rect)) = trigger {
                        self.selected_scene = scene_index;
                        self.sync_selection();
                        self.selected_trigger = Some(index);
                        self.tool = EditorTool::Trigger;
                        self.focus_trigger_rect(&format!("trigger '{}'", label), rect);
                    }
                }
                PendingOutlinerAction::TriggerDuplicate { scene_index, index } => {
                    let duplicate = self
                        .bundle
                        .as_ref()
                        .and_then(|bundle| bundle.scenes.get(scene_index))
                        .and_then(|scene| scene.triggers.get(index).cloned())
                        .map(|mut trigger| {
                            let existing = self
                                .bundle
                                .as_ref()
                                .and_then(|bundle| bundle.scenes.get(scene_index))
                                .map(|scene| {
                                    scene
                                        .triggers
                                        .iter()
                                        .map(|entry| entry.id.clone())
                                        .collect::<BTreeSet<_>>()
                                })
                                .unwrap_or_default();
                            trigger.id = next_unique_layer_id(&existing, &trigger.id);
                            trigger.rect.x += 16;
                            trigger
                        });
                    if let Some(duplicate) = duplicate {
                        self.capture_history();
                        if let Some(scene) = self
                            .bundle
                            .as_mut()
                            .and_then(|bundle| bundle.scenes.get_mut(scene_index))
                        {
                            let insert_at = (index + 1).min(scene.triggers.len());
                            let label = duplicate.id.clone();
                            scene.triggers.insert(insert_at, duplicate);
                            self.selected_scene = scene_index;
                            self.selected_trigger = Some(insert_at);
                            self.tool = EditorTool::Trigger;
                            self.mark_edited(format!("Duplicated trigger '{}'", label));
                        }
                    }
                }
                PendingOutlinerAction::TriggerIsolate { scene_index, index } => {
                    self.solo_group = Some(SceneObjectGroup::Triggers);
                    let trigger = self
                        .bundle
                        .as_ref()
                        .and_then(|bundle| bundle.scenes.get(scene_index))
                        .and_then(|scene| scene.triggers.get(index))
                        .map(|trigger| (trigger.id.clone(), trigger.rect));
                    if let Some((label, rect)) = trigger {
                        self.selected_scene = scene_index;
                        self.selected_trigger = Some(index);
                        self.tool = EditorTool::Trigger;
                        self.focus_trigger_rect(&format!("trigger '{}'", label), rect);
                    }
                }
                PendingOutlinerAction::Script {
                    scene_index,
                    script_index,
                    label,
                } => {
                    self.select_script(scene_index, script_index);
                    self.status = format!("Selected script '{}'", label);
                }
                PendingOutlinerAction::ScriptIsolate {
                    scene_index,
                    script_index,
                    label,
                } => {
                    self.selected_scene = scene_index;
                    self.solo_group = Some(SceneObjectGroup::Scripts);
                    self.sync_selection();
                    self.select_script(scene_index, script_index);
                    self.status = format!("Isolated script group around '{}'", label);
                }
            }
        }
    }
}
