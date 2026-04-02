use super::*;

impl EditorApp {
    pub(super) fn draw_asset_browser_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.add(
            egui::TextEdit::singleline(&mut self.asset_browser_filter)
                .hint_text("Filter assets by id or file name"),
        );
        ui.add_space(6.0);

        let Some(bundle) = self.bundle.clone() else {
            ui.label("No project loaded.");
            return;
        };

        enum PendingAssetAction {
            Scene {
                scene_index: usize,
                label: String,
            },
            LoadPrefab(String),
            Metasprite {
                metasprite_index: usize,
                label: String,
            },
            Animation {
                animation_index: usize,
                label: String,
            },
            Dialogue {
                dialogue_index: usize,
                label: String,
            },
            Script {
                scene_index: usize,
                script_index: usize,
                label: String,
            },
            Status(String),
            SpriteSource {
                path: Utf8PathBuf,
                label: String,
            },
            ToggleFavorite {
                kind: &'static str,
                id: String,
            },
            LoadBrush(String),
            LoadSnippet(String),
            PromoteLegacySnippet(String),
        }

        let filter = self.asset_browser_filter.trim().to_ascii_lowercase();
        let sprite_sources = self.list_project_sprite_sources().unwrap_or_default();
        let time_seconds = ctx.input(|input| input.time) as f32;
        let mut pending_action = None;

        ui.label(format!(
            "{} scene(s) | {} prefab(s) | {} tileset(s) | {} palette(s) | {} metasprite(s) | {} animation(s) | {} dialogue(s)",
            bundle.scenes.len(),
            bundle.prefabs.len(),
            bundle.tilesets.len(),
            bundle.palettes.len(),
            bundle.metasprites.len(),
            bundle.animations.len(),
            bundle.dialogues.len()
        ));
        ui.label(format!(
            "{} favorite(s) | {} snippet(s) | {} brush(es)",
            self.workspace_addons.editor_favorites.scenes.len()
                + self.workspace_addons.editor_favorites.prefabs.len()
                + self.workspace_addons.editor_favorites.palettes.len()
                + self.workspace_addons.editor_favorites.tilesets.len()
                + self.workspace_addons.editor_favorites.metasprites.len()
                + self.workspace_addons.editor_favorites.animations.len()
                + self.workspace_addons.editor_favorites.dialogues.len()
                + self.workspace_addons.editor_favorites.sprite_sources.len(),
            self.workspace_addons.scene_library.snippets.len(),
            self.workspace_addons.scene_library.brushes.len(),
        ));
        ui.add_space(4.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.collapsing(format!("Scenes ({})", bundle.scenes.len()), |ui| {
                for (scene_index, scene) in bundle.scenes.iter().enumerate() {
                    if !filter_matches(&filter, &scene.id) && !filter.is_empty() {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        draw_scene_thumbnail(ui, &bundle, scene);
                        ui.vertical(|ui| {
                            if ui
                                .selectable_label(self.selected_scene == scene_index, &scene.id)
                                .clicked()
                            {
                                pending_action = Some(PendingAssetAction::Scene {
                                    scene_index,
                                    label: scene.id.clone(),
                                });
                            }
                            ui.small(format!("{}", self.asset_usage_summary("scene", &scene.id)));
                            ui.horizontal(|ui| {
                                if ui
                                    .small_button(if self.is_asset_favorite("scene", &scene.id) {
                                        "Unstar"
                                    } else {
                                        "Star"
                                    })
                                    .clicked()
                                {
                                    pending_action = Some(PendingAssetAction::ToggleFavorite {
                                        kind: "scene",
                                        id: scene.id.clone(),
                                    });
                                }
                            });
                        });
                    });
                }
            });

            ui.collapsing(format!("Prefabs ({})", bundle.prefabs.len()), |ui| {
                if bundle.prefabs.is_empty() {
                    ui.label("Save a selection as a prefab from the Scene tab.");
                } else {
                    for prefab in &bundle.prefabs {
                        let matches_name = filter_matches(&filter, &prefab.name);
                        let matches_id = filter_matches(&filter, &prefab.id);
                        if !filter.is_empty() && !matches_name && !matches_id {
                            continue;
                        }
                        ui.horizontal(|ui| {
                            draw_prefab_thumbnail(ui, &bundle, prefab);
                            ui.vertical(|ui| {
                                let label =
                                    if prefab.name.trim().is_empty() || prefab.name == prefab.id {
                                        format!(
                                            "{} ({}x{} tiles, {} object(s))",
                                            prefab.id,
                                            prefab.size_tiles.width,
                                            prefab.size_tiles.height,
                                            prefab.spawns.len()
                                                + prefab.checkpoints.len()
                                                + prefab.entities.len()
                                                + prefab.triggers.len()
                                        )
                                    } else {
                                        format!(
                                            "{} [{}] ({}x{} tiles, {} object(s))",
                                            prefab.name,
                                            prefab.id,
                                            prefab.size_tiles.width,
                                            prefab.size_tiles.height,
                                            prefab.spawns.len()
                                                + prefab.checkpoints.len()
                                                + prefab.entities.len()
                                                + prefab.triggers.len()
                                        )
                                    };
                                let response = ui.selectable_label(false, label);
                                if response.drag_started() {
                                    self.asset_drag_payload = Some(AssetDragPayload::Prefab {
                                        id: prefab.id.clone(),
                                        label: if prefab.name.trim().is_empty() {
                                            prefab.id.clone()
                                        } else {
                                            prefab.name.clone()
                                        },
                                    });
                                }
                                if response.clicked() {
                                    pending_action =
                                        Some(PendingAssetAction::LoadPrefab(prefab.id.clone()));
                                }
                                ui.small(self.asset_usage_summary("prefab", &prefab.id));
                                ui.horizontal(|ui| {
                                    if ui.small_button("Load").clicked() {
                                        pending_action =
                                            Some(PendingAssetAction::LoadPrefab(prefab.id.clone()));
                                    }
                                    if ui
                                        .small_button(
                                            if self.is_asset_favorite("prefab", &prefab.id) {
                                                "Unstar"
                                            } else {
                                                "Star"
                                            },
                                        )
                                        .clicked()
                                    {
                                        pending_action = Some(PendingAssetAction::ToggleFavorite {
                                            kind: "prefab",
                                            id: prefab.id.clone(),
                                        });
                                    }
                                });
                            });
                        });
                    }
                }
            });

            ui.collapsing(format!("Animations ({})", bundle.animations.len()), |ui| {
                for (animation_index, animation) in bundle.animations.iter().enumerate() {
                    if !filter_matches(&filter, &animation.id) && !filter.is_empty() {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        draw_animation_thumbnail(ui, &bundle, animation_index, time_seconds);
                        ui.vertical(|ui| {
                            let label =
                                format!("{} ({} frame(s))", animation.id, animation.frames.len());
                            let response = ui.selectable_label(
                                self.selected_animation == animation_index,
                                label,
                            );
                            if response.drag_started() {
                                self.asset_drag_payload = Some(AssetDragPayload::Visual {
                                    id: animation.id.clone(),
                                    label: animation.id.clone(),
                                });
                            }
                            if response.clicked() {
                                pending_action = Some(PendingAssetAction::Animation {
                                    animation_index,
                                    label: animation.id.clone(),
                                });
                            }
                            ui.small(self.asset_usage_summary("animation", &animation.id));
                            if ui
                                .small_button(
                                    if self.is_asset_favorite("animation", &animation.id) {
                                        "Unstar"
                                    } else {
                                        "Star"
                                    },
                                )
                                .clicked()
                            {
                                pending_action = Some(PendingAssetAction::ToggleFavorite {
                                    kind: "animation",
                                    id: animation.id.clone(),
                                });
                            }
                        });
                    });
                }
            });

            ui.collapsing(
                format!("Metasprites ({})", bundle.metasprites.len()),
                |ui| {
                    for (metasprite_index, metasprite) in bundle.metasprites.iter().enumerate() {
                        if !filter_matches(&filter, &metasprite.id) && !filter.is_empty() {
                            continue;
                        }
                        ui.horizontal(|ui| {
                            draw_metasprite_thumbnail(ui, &bundle, metasprite);
                            ui.vertical(|ui| {
                                let label = format!(
                                    "{} ({} piece(s))",
                                    metasprite.id,
                                    metasprite.pieces.len()
                                );
                                let response = ui.selectable_label(
                                    self.selected_metasprite == Some(metasprite_index),
                                    label,
                                );
                                if response.drag_started() {
                                    self.asset_drag_payload = Some(AssetDragPayload::Visual {
                                        id: metasprite.id.clone(),
                                        label: metasprite.id.clone(),
                                    });
                                }
                                if response.clicked() {
                                    pending_action = Some(PendingAssetAction::Metasprite {
                                        metasprite_index,
                                        label: metasprite.id.clone(),
                                    });
                                }
                                ui.small(self.asset_usage_summary("metasprite", &metasprite.id));
                                if ui
                                    .small_button(
                                        if self.is_asset_favorite("metasprite", &metasprite.id) {
                                            "Unstar"
                                        } else {
                                            "Star"
                                        },
                                    )
                                    .clicked()
                                {
                                    pending_action = Some(PendingAssetAction::ToggleFavorite {
                                        kind: "metasprite",
                                        id: metasprite.id.clone(),
                                    });
                                }
                            });
                        });
                    }
                },
            );

            ui.collapsing(format!("Tilesets ({})", bundle.tilesets.len()), |ui| {
                for tileset in &bundle.tilesets {
                    if !filter_matches(&filter, &tileset.id) && !filter.is_empty() {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        if let Some(palette) = bundle.palette(&tileset.palette_id) {
                            draw_tileset_thumbnail(ui, tileset, palette);
                        } else {
                            let (response, painter) =
                                draw_asset_thumbnail_frame(ui, Vec2::new(92.0, 54.0));
                            painter.text(
                                response.rect.center(),
                                Align2::CENTER_CENTER,
                                "No palette",
                                FontId::proportional(12.0),
                                Color32::from_rgb(180, 160, 160),
                            );
                        }
                        ui.vertical(|ui| {
                            if ui
                                .selectable_label(
                                    false,
                                    format!("{} ({} tile(s))", tileset.id, tileset.tiles.len()),
                                )
                                .clicked()
                            {
                                pending_action = Some(PendingAssetAction::Status(format!(
                                    "Tileset '{}' selected with {} tile(s).",
                                    tileset.id,
                                    tileset.tiles.len()
                                )));
                            }
                            ui.small(self.asset_usage_summary("tileset", &tileset.id));
                            if ui
                                .small_button(if self.is_asset_favorite("tileset", &tileset.id) {
                                    "Unstar"
                                } else {
                                    "Star"
                                })
                                .clicked()
                            {
                                pending_action = Some(PendingAssetAction::ToggleFavorite {
                                    kind: "tileset",
                                    id: tileset.id.clone(),
                                });
                            }
                        });
                    });
                }
            });

            ui.collapsing(format!("Palettes ({})", bundle.palettes.len()), |ui| {
                for palette in &bundle.palettes {
                    if !filter_matches(&filter, &palette.id) && !filter.is_empty() {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        draw_palette_thumbnail(ui, palette);
                        ui.vertical(|ui| {
                            if ui
                                .selectable_label(
                                    false,
                                    format!("{} ({} color(s))", palette.id, palette.colors.len()),
                                )
                                .clicked()
                            {
                                pending_action = Some(PendingAssetAction::Status(format!(
                                    "Palette '{}' selected with {} color(s).",
                                    palette.id,
                                    palette.colors.len()
                                )));
                            }
                            ui.small(self.asset_usage_summary("palette", &palette.id));
                            if ui
                                .small_button(if self.is_asset_favorite("palette", &palette.id) {
                                    "Unstar"
                                } else {
                                    "Star"
                                })
                                .clicked()
                            {
                                pending_action = Some(PendingAssetAction::ToggleFavorite {
                                    kind: "palette",
                                    id: palette.id.clone(),
                                });
                            }
                        });
                    });
                }
            });

            ui.collapsing(format!("Dialogues ({})", bundle.dialogues.len()), |ui| {
                for (dialogue_index, dialogue) in bundle.dialogues.iter().enumerate() {
                    if !filter_matches(&filter, &dialogue.id) && !filter.is_empty() {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        draw_text_thumbnail(
                            ui,
                            "Dialogue",
                            &dialogue_preview_text(dialogue),
                            Color32::from_rgb(214, 132, 84),
                        );
                        ui.vertical(|ui| {
                            if ui
                                .selectable_label(
                                    self.selected_dialogue == Some(dialogue_index),
                                    format!("{} ({} node(s))", dialogue.id, dialogue.nodes.len()),
                                )
                                .clicked()
                            {
                                pending_action = Some(PendingAssetAction::Dialogue {
                                    dialogue_index,
                                    label: dialogue.id.clone(),
                                });
                            }
                            ui.small(self.asset_usage_summary("dialogue", &dialogue.id));
                            if ui
                                .small_button(if self.is_asset_favorite("dialogue", &dialogue.id) {
                                    "Unstar"
                                } else {
                                    "Star"
                                })
                                .clicked()
                            {
                                pending_action = Some(PendingAssetAction::ToggleFavorite {
                                    kind: "dialogue",
                                    id: dialogue.id.clone(),
                                });
                            }
                        });
                    });
                }
            });

            if let Some(scene) = bundle.scenes.get(self.selected_scene) {
                ui.collapsing(
                    format!("Scripts in '{}' ({})", scene.id, scene.scripts.len()),
                    |ui| {
                        for (script_index, script) in scene.scripts.iter().enumerate() {
                            if !filter_matches(&filter, &script.id) && !filter.is_empty() {
                                continue;
                            }
                            ui.horizontal(|ui| {
                                draw_text_thumbnail(
                                    ui,
                                    "Script",
                                    &script_preview_text(script),
                                    Color32::from_rgb(96, 208, 255),
                                );
                                ui.vertical(|ui| {
                                    let label = format!(
                                        "{} ({} command(s))",
                                        script.id,
                                        script.commands.len()
                                    );
                                    let response = ui.selectable_label(
                                        self.selected_script == Some(script_index),
                                        label,
                                    );
                                    if response.drag_started() {
                                        self.asset_drag_payload = Some(AssetDragPayload::Script {
                                            id: script.id.clone(),
                                            label: script.id.clone(),
                                        });
                                    }
                                    if response.clicked() {
                                        pending_action = Some(PendingAssetAction::Script {
                                            scene_index: self.selected_scene,
                                            script_index,
                                            label: script.id.clone(),
                                        });
                                    }
                                    ui.small(format!("Scene: {}", scene.id));
                                });
                            });
                        }
                    },
                );
            }

            ui.collapsing(format!("Sprite Sources ({})", sprite_sources.len()), |ui| {
                match sprite_sources.is_empty() {
                    true => {
                        ui.label("No project-local sprite sheets yet.");
                    }
                    false => {
                        for path in &sprite_sources {
                            let label = self.project_sprite_source_relative_path(path);
                            if !filter_matches(&filter, &label) && !filter.is_empty() {
                                continue;
                            }
                            ui.horizontal(|ui| {
                                if let Some(texture) = self.sprite_source_preview_texture(ctx, path)
                                {
                                    draw_sprite_source_thumbnail(ui, &texture, &label);
                                } else {
                                    let (response, painter) =
                                        draw_asset_thumbnail_frame(ui, Vec2::new(92.0, 54.0));
                                    painter.text(
                                        response.rect.center(),
                                        Align2::CENTER_CENTER,
                                        "Sheet",
                                        FontId::proportional(12.0),
                                        Color32::from_rgb(180, 180, 190),
                                    );
                                }
                                ui.vertical(|ui| {
                                    if ui.selectable_label(false, &label).clicked() {
                                        pending_action = Some(PendingAssetAction::SpriteSource {
                                            path: path.clone(),
                                            label: label.clone(),
                                        });
                                    }
                                });
                                if ui
                                    .small_button(
                                        if self.is_asset_favorite("sprite_source", &label) {
                                            "Unstar"
                                        } else {
                                            "Star"
                                        },
                                    )
                                    .clicked()
                                {
                                    pending_action = Some(PendingAssetAction::ToggleFavorite {
                                        kind: "sprite_source",
                                        id: label.clone(),
                                    });
                                }
                            });
                        }
                    }
                }
            });

            ui.collapsing(
                format!(
                    "Legacy Scene Snippets ({})",
                    self.workspace_addons.scene_library.snippets.len()
                ),
                |ui| {
                    if self.workspace_addons.scene_library.snippets.is_empty() {
                        ui.label("No legacy workspace snippets are stored for this project.");
                    } else {
                        for snippet in &self.workspace_addons.scene_library.snippets {
                            if !filter_matches(&filter, &snippet.name) && !filter.is_empty() {
                                continue;
                            }
                            ui.horizontal(|ui| {
                                draw_scene_snippet_thumbnail(ui, &bundle, snippet);
                                ui.vertical(|ui| {
                                    let label = format!(
                                        "{} ({}x{} tiles, {} object(s))",
                                        snippet.name,
                                        snippet.size_tiles.width,
                                        snippet.size_tiles.height,
                                        snippet.spawns.len()
                                            + snippet.checkpoints.len()
                                            + snippet.entities.len()
                                            + snippet.triggers.len()
                                    );
                                    let response = ui.selectable_label(false, label);
                                    if response.drag_started() {
                                        self.asset_drag_payload =
                                            Some(AssetDragPayload::LegacySnippet {
                                                name: snippet.name.clone(),
                                                label: snippet.name.clone(),
                                            });
                                    }
                                    if response.clicked() {
                                        pending_action = Some(PendingAssetAction::LoadSnippet(
                                            snippet.name.clone(),
                                        ));
                                    }
                                    if ui.small_button("Load").clicked() {
                                        pending_action = Some(PendingAssetAction::LoadSnippet(
                                            snippet.name.clone(),
                                        ));
                                    }
                                    if ui.small_button("Promote").clicked() {
                                        pending_action =
                                            Some(PendingAssetAction::PromoteLegacySnippet(
                                                snippet.name.clone(),
                                            ));
                                    }
                                });
                            });
                        }
                    }
                },
            );

            ui.collapsing(
                format!(
                    "Tile Brushes ({})",
                    self.workspace_addons.scene_library.brushes.len()
                ),
                |ui| {
                    if self.workspace_addons.scene_library.brushes.is_empty() {
                        ui.label("Save a selection as a brush from the Scene tab.");
                    } else {
                        for brush in &self.workspace_addons.scene_library.brushes {
                            if !filter_matches(&filter, &brush.name) && !filter.is_empty() {
                                continue;
                            }
                            ui.horizontal(|ui| {
                                draw_tile_brush_thumbnail(ui, brush);
                                ui.vertical(|ui| {
                                    let label = format!(
                                        "{} ({}x{} tiles)",
                                        brush.name, brush.size_tiles.width, brush.size_tiles.height
                                    );
                                    let response = ui.selectable_label(false, label);
                                    if response.drag_started() {
                                        self.asset_drag_payload = Some(AssetDragPayload::Brush {
                                            name: brush.name.clone(),
                                            label: brush.name.clone(),
                                        });
                                    }
                                    if response.clicked() {
                                        pending_action =
                                            Some(PendingAssetAction::LoadBrush(brush.name.clone()));
                                    }
                                    if ui.small_button("Load").clicked() {
                                        pending_action =
                                            Some(PendingAssetAction::LoadBrush(brush.name.clone()));
                                    }
                                });
                            });
                        }
                    }
                },
            );
        });

        if let Some(action) = pending_action {
            match action {
                PendingAssetAction::Scene { scene_index, label } => {
                    self.selected_scene = scene_index;
                    self.scene_scroll_offset = Vec2::ZERO;
                    self.clear_selection();
                    self.preview_focus = PreviewFocus::None;
                    self.sync_selection();
                    self.status = format!("Selected scene '{}'", label);
                }
                PendingAssetAction::LoadPrefab(id) => {
                    self.load_prefab_into_clipboard(&id);
                }
                PendingAssetAction::Metasprite {
                    metasprite_index,
                    label,
                } => {
                    self.selected_metasprite = Some(metasprite_index);
                    self.selected_metasprite_piece = self
                        .bundle
                        .as_ref()
                        .and_then(|bundle| bundle.metasprites.get(metasprite_index))
                        .and_then(|metasprite| {
                            sanitize_optional_index(Some(0), metasprite.pieces.len())
                        });
                    self.preview_focus = PreviewFocus::Metasprite;
                    self.metasprite_drag_state = None;
                    self.metasprite_place_mode = false;
                    self.workspace.layout.show_tab(DockTab::Animation);
                    self.status = format!("Selected metasprite '{}'", label);
                }
                PendingAssetAction::Animation {
                    animation_index,
                    label,
                } => {
                    self.selected_animation = animation_index;
                    self.preview_focus = PreviewFocus::Animation;
                    self.animation_preview_scrub_frame = 0;
                    self.animation_preview_play_anchor_tick = 0;
                    self.animation_preview_play_anchor_seconds = 0.0;
                    self.metasprite_drag_state = None;
                    self.metasprite_place_mode = false;
                    self.status = format!("Selected animation '{}'", label);
                }
                PendingAssetAction::Dialogue {
                    dialogue_index,
                    label,
                } => {
                    self.select_dialogue(dialogue_index);
                    self.status = format!("Selected dialogue '{}'", label);
                }
                PendingAssetAction::Script {
                    scene_index,
                    script_index,
                    label,
                } => {
                    self.select_script(scene_index, script_index);
                    self.status = format!("Selected script '{}'", label);
                }
                PendingAssetAction::Status(message) => {
                    self.status = message;
                }
                PendingAssetAction::SpriteSource { path, label } => {
                    self.import_state.open = true;
                    self.load_import_preview_from_path(ctx, path.as_std_path(), label);
                }
                PendingAssetAction::ToggleFavorite { kind, id } => {
                    self.toggle_asset_favorite(kind, &id);
                }
                PendingAssetAction::LoadBrush(name) => {
                    self.load_brush_into_clipboard(&name);
                }
                PendingAssetAction::LoadSnippet(name) => {
                    self.load_snippet_into_clipboard(&name);
                }
                PendingAssetAction::PromoteLegacySnippet(name) => {
                    self.promote_legacy_snippet_to_prefab(&name);
                }
            }
        }
    }
}
