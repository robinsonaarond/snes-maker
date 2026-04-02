use super::*;

pub(super) fn draw_animation_tab(app: &mut EditorApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    let Some(bundle) = app.bundle.as_ref() else {
        ui.heading("Animation");
        ui.label("Load a project to preview animations.");
        return;
    };

    let metasprite_list = bundle
        .metasprites
        .iter()
        .enumerate()
        .map(|(index, metasprite)| (index, metasprite.id.clone(), metasprite.pieces.len()))
        .collect::<Vec<_>>();
    let animation_list = bundle
        .animations
        .iter()
        .enumerate()
        .map(|(index, animation)| (index, animation.id.clone(), animation.frames.len()))
        .collect::<Vec<_>>();
    let metasprite_ids = bundle
        .metasprites
        .iter()
        .map(|metasprite| metasprite.id.clone())
        .collect::<Vec<_>>();
    let animation_snapshot = bundle.animations.get(app.selected_animation).cloned();
    let current_time = ctx.input(|input| input.time) as f32;
    let preview_tick_before_ui = animation_snapshot
        .as_ref()
        .map(|animation| animation_preview_tick(app, animation, current_time));

    ui.heading("Animation");
    ui.label("Select a metasprite or animation to edit it visually.");
    ui.horizontal_wrapped(|ui| {
        ui.label("Preview Facing");
        ui.selectable_value(
            &mut app.animation_preview_facing,
            Facing::Right,
            facing_label(Facing::Right),
        );
        ui.selectable_value(
            &mut app.animation_preview_facing,
            Facing::Left,
            facing_label(Facing::Left),
        );
        let speed_response = ui
            .add(egui::Slider::new(&mut app.animation_preview_speed, 0.25..=2.0).text("Playback"));
        if speed_response.changed() && app.animation_preview_playing {
            if let Some(tick) = preview_tick_before_ui {
                set_animation_preview_anchor(app, current_time, tick);
            }
        }
        ui.checkbox(&mut app.show_metasprite_anchor, "Anchor");
        ui.checkbox(&mut app.show_metasprite_bounds, "Piece Bounds");
        if matches!(app.preview_focus, PreviewFocus::Entity) {
            ui.checkbox(&mut app.show_entity_hitbox_preview, "Entity Hitbox");
        }
    });

    if let Some(animation) = animation_snapshot.as_ref() {
        let total_frames = animation_total_frames(animation);
        let current_time = ctx.input(|input| input.time) as f32;
        let max_tick = total_frames.saturating_sub(1);
        let mut current_tick = animation_preview_tick(app, animation, current_time);
        if app.animation_preview_playing && !app.animation_preview_loop && current_tick >= max_tick
        {
            app.animation_preview_playing = false;
            app.animation_preview_scrub_frame = max_tick;
            set_animation_preview_anchor(app, current_time, max_tick);
            current_tick = max_tick;
        }
        let current_frame = animation_frame_index_for_tick(animation, current_tick) + 1;
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(if app.animation_preview_playing {
                    "Pause"
                } else {
                    "Play"
                })
                .clicked()
            {
                if app.animation_preview_playing {
                    app.animation_preview_scrub_frame = current_tick;
                    app.animation_preview_play_anchor_tick = current_tick;
                }
                set_animation_preview_anchor(app, current_time, app.animation_preview_scrub_frame);
                app.animation_preview_playing = !app.animation_preview_playing;
            }
            ui.checkbox(&mut app.animation_preview_loop, "Loop");
            let mut scrub = app.animation_preview_scrub_frame.min(max_tick);
            let response = ui.add(egui::Slider::new(&mut scrub, 0..=max_tick).text("Scrub"));
            if response.changed() {
                app.animation_preview_scrub_frame = scrub;
                app.animation_preview_playing = false;
            }
            ui.label(format!(
                "Frame {} / {}  |  Tick {} / {}",
                current_frame,
                animation.frames.len().max(1),
                current_tick.saturating_add(1),
                total_frames.max(1),
            ));
        });
    }

    ui.separator();
    ui.columns(2, |columns| {
        columns[0].label("Metasprites");
        egui::ScrollArea::vertical()
            .max_height(160.0)
            .show(&mut columns[0], |ui| {
                for (index, id, piece_count) in &metasprite_list {
                    if ui
                        .selectable_label(
                            app.selected_metasprite == Some(*index),
                            format!("{id} ({piece_count} piece(s))"),
                        )
                        .clicked()
                    {
                        let selected_piece = app
                            .bundle
                            .as_ref()
                            .and_then(|bundle| bundle.metasprites.get(*index))
                            .and_then(|metasprite| {
                                sanitize_optional_index(Some(0), metasprite.pieces.len())
                            });
                        app.selected_metasprite = Some(*index);
                        app.selected_metasprite_piece = selected_piece;
                        app.preview_focus = PreviewFocus::Metasprite;
                        app.metasprite_drag_state = None;
                        app.metasprite_place_mode = false;
                    }
                }
            });

        columns[1].label("Animations");
        egui::ScrollArea::vertical()
            .max_height(160.0)
            .show(&mut columns[1], |ui| {
                for (index, id, frame_count) in &animation_list {
                    if ui
                        .selectable_label(
                            app.selected_animation == *index,
                            format!("{id} ({frame_count} frame(s))"),
                        )
                        .clicked()
                    {
                        app.selected_animation = *index;
                        app.preview_focus = PreviewFocus::Animation;
                        app.animation_preview_scrub_frame = 0;
                        set_animation_preview_anchor(app, current_time, 0);
                        app.metasprite_drag_state = None;
                        app.metasprite_place_mode = false;
                    }
                }
            });
    });

    ui.separator();
    if app.has_context_preview() {
        app.draw_context_preview(ui, current_time);
    } else if animation_snapshot.is_some() {
        app.preview_focus = PreviewFocus::Animation;
        app.draw_context_preview(ui, current_time);
    } else if app.selected_metasprite.is_some() {
        app.preview_focus = PreviewFocus::Metasprite;
        app.draw_context_preview(ui, current_time);
    } else {
        ui.label("Select a metasprite or animation to preview it here.");
    }

    ui.separator();
    draw_animation_inspector(app, ui, animation_snapshot.as_ref(), &metasprite_ids);
}

pub(super) fn draw_animation_inspector(
    app: &mut EditorApp,
    ui: &mut egui::Ui,
    animation_snapshot: Option<&AnimationResource>,
    metasprite_ids: &[String],
) {
    if matches!(app.preview_focus, PreviewFocus::Metasprite) && app.selected_metasprite.is_some() {
        draw_metasprite_editor(app, ui);
        return;
    }

    ui.collapsing("Animation Frames", |ui| {
        let Some(animation_snapshot) = animation_snapshot else {
            ui.label("No animation selected.");
            return;
        };

        ui.label(format!("Selected animation: {}", animation_snapshot.id));
        if metasprite_ids.is_empty() {
            ui.label("Import or create a metasprite to start building frames.");
            return;
        }

        #[derive(Clone, Copy)]
        enum FrameAction {
            Remove(usize),
            Duplicate(usize),
            MoveUp(usize),
            MoveDown(usize),
        }

        let mut edited = animation_snapshot.clone();
        let mut changed = false;
        let mut action = None;
        let frame_count = edited.frames.len();

        for (index, frame) in edited.frames.iter_mut().enumerate() {
            let can_move_up = index > 0;
            let can_move_down = index + 1 < frame_count;
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("Frame {}", index + 1));
                    egui::ComboBox::from_id_salt((
                        "animation_frame",
                        app.selected_animation,
                        index,
                    ))
                    .selected_text(&frame.metasprite_id)
                    .show_ui(ui, |ui| {
                        for metasprite_id in metasprite_ids {
                            changed |= ui
                                .selectable_value(
                                    &mut frame.metasprite_id,
                                    metasprite_id.clone(),
                                    metasprite_id,
                                )
                                .changed();
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label("Duration");
                    changed |= ui
                        .add(egui::DragValue::new(&mut frame.duration_frames).range(1..=120))
                        .changed();
                    if ui.small_button("Up").clicked() && can_move_up {
                        action = Some(FrameAction::MoveUp(index));
                    }
                    if ui.small_button("Down").clicked() && can_move_down {
                        action = Some(FrameAction::MoveDown(index));
                    }
                    if ui.small_button("Duplicate").clicked() {
                        action = Some(FrameAction::Duplicate(index));
                    }
                    if ui.small_button("Remove").clicked() {
                        action = Some(FrameAction::Remove(index));
                    }
                });
            });
        }

        if let Some(action) = action {
            match action {
                FrameAction::Remove(index) if index < edited.frames.len() => {
                    edited.frames.remove(index);
                    changed = true;
                }
                FrameAction::Duplicate(index) if index < edited.frames.len() => {
                    let duplicate = edited.frames[index].clone();
                    edited.frames.insert(index + 1, duplicate);
                    changed = true;
                }
                FrameAction::MoveUp(index) if index > 0 && index < edited.frames.len() => {
                    edited.frames.swap(index, index - 1);
                    changed = true;
                }
                FrameAction::MoveDown(index) if index + 1 < edited.frames.len() => {
                    edited.frames.swap(index, index + 1);
                    changed = true;
                }
                _ => {}
            }
        }

        ui.horizontal(|ui| {
            if ui.button("+ Frame").clicked() {
                let next = edited.frames.last().cloned().unwrap_or(AnimationFrame {
                    metasprite_id: metasprite_ids[0].clone(),
                    duration_frames: 8,
                });
                edited.frames.push(next);
                changed = true;
            }
            if ui.button("Preview Animation").clicked() {
                app.preview_focus = PreviewFocus::Animation;
            }
        });

        if changed {
            app.capture_history();
            if let Some(bundle) = &mut app.bundle {
                if let Some(animation) = bundle.animations.get_mut(app.selected_animation) {
                    *animation = edited;
                }
            }
            app.mark_edited(format!("Updated animation '{}'", animation_snapshot.id));
        }
    });
}

pub(super) fn draw_context_preview(app: &mut EditorApp, ui: &mut egui::Ui, time_seconds: f32) {
    match app.preview_focus {
        PreviewFocus::Metasprite => {
            let Some((metasprite, tileset, palette)) = app.bundle.as_ref().and_then(|bundle| {
                let index = app.selected_metasprite?;
                let metasprite = bundle.metasprites.get(index)?.clone();
                let tileset = super::find_tileset_for_metasprite(bundle, &metasprite)?.clone();
                let palette = bundle.palette(&metasprite.palette_id)?.clone();
                Some((metasprite, tileset, palette))
            }) else {
                return;
            };
            ui.collapsing("Metasprite Preview", |ui| {
                ui.label(format!("Selected metasprite: {}", metasprite.id));
                draw_metasprite_preview_canvas(
                    app,
                    ui,
                    &metasprite,
                    &tileset,
                    &palette,
                    app.animation_preview_facing,
                    None,
                    false,
                );
            });
        }
        PreviewFocus::Animation => {
            let Some((animation, metasprite, tileset, palette, tick)) =
                app.bundle.as_ref().and_then(|bundle| {
                    let animation = bundle.animations.get(app.selected_animation)?.clone();
                    let tick = animation_preview_tick(app, &animation, time_seconds);
                    let metasprite =
                        metasprite_for_animation_tick(bundle, &animation, tick)?.clone();
                    let tileset = super::find_tileset_for_metasprite(bundle, &metasprite)?.clone();
                    let palette = bundle.palette(&metasprite.palette_id)?.clone();
                    Some((animation, metasprite, tileset, palette, tick))
                })
            else {
                return;
            };
            ui.collapsing("Animation Preview", |ui| {
                ui.label(format!("Selected animation: {}", animation.id));
                ui.label(format!(
                    "Preview tick {}  |  frame {}",
                    tick.saturating_add(1),
                    animation_frame_index_for_tick(&animation, tick) + 1
                ));
                draw_metasprite_preview_canvas(
                    app,
                    ui,
                    &metasprite,
                    &tileset,
                    &palette,
                    app.animation_preview_facing,
                    None,
                    false,
                );
            });
        }
        PreviewFocus::Entity => {
            let Some((entity, metasprite, tileset, palette)) =
                app.bundle.as_ref().and_then(|bundle| {
                    let scene = bundle.scenes.get(app.selected_scene)?;
                    let entity = scene.entities.get(app.selected_entity?)?.clone();
                    let metasprite =
                        metasprite_for_entity_preview(bundle, &entity, app, time_seconds)?.clone();
                    let tileset = super::find_tileset_for_metasprite(bundle, &metasprite)?.clone();
                    let palette = bundle.palette(&metasprite.palette_id)?.clone();
                    Some((entity, metasprite, tileset, palette))
                })
            else {
                return;
            };
            ui.collapsing("Animation Preview", |ui| {
                ui.label(format!(
                    "Selected entity: {} ({})",
                    entity.id, entity.archetype
                ));
                let hitbox = app.show_entity_hitbox_preview.then_some(entity.hitbox);
                draw_metasprite_preview_canvas(
                    app,
                    ui,
                    &metasprite,
                    &tileset,
                    &palette,
                    entity.facing,
                    hitbox,
                    false,
                );
            });
        }
        PreviewFocus::None => {}
    }
}

pub(super) fn has_context_preview(app: &EditorApp) -> bool {
    let Some(bundle) = &app.bundle else {
        return false;
    };

    match app.preview_focus {
        PreviewFocus::Metasprite => app
            .selected_metasprite
            .and_then(|index| bundle.metasprites.get(index))
            .is_some(),
        PreviewFocus::Animation => bundle.animations.get(app.selected_animation).is_some(),
        PreviewFocus::Entity => bundle
            .scenes
            .get(app.selected_scene)
            .and_then(|scene| {
                app.selected_entity
                    .and_then(|index| scene.entities.get(index))
            })
            .is_some_and(|entity| entity_has_animation(bundle, entity)),
        PreviewFocus::None => false,
    }
}

pub(super) fn needs_animation_repaint(app: &EditorApp) -> bool {
    let Some(bundle) = &app.bundle else {
        return false;
    };

    let preview_animates = app.animation_preview_playing
        && match app.preview_focus {
            PreviewFocus::Animation => bundle.animations.get(app.selected_animation).is_some(),
            PreviewFocus::Entity => bundle
                .scenes
                .get(app.selected_scene)
                .and_then(|scene| {
                    app.selected_entity
                        .and_then(|index| scene.entities.get(index))
                })
                .is_some_and(|entity| entity_has_animation(bundle, entity)),
            PreviewFocus::Metasprite | PreviewFocus::None => false,
        };

    preview_animates
        || bundle.scenes.get(app.selected_scene).is_some_and(|scene| {
            scene
                .entities
                .iter()
                .any(|entity| entity_has_animation(bundle, entity))
        })
}

fn draw_metasprite_editor(app: &mut EditorApp, ui: &mut egui::Ui) {
    ui.collapsing("Metasprite Editor", |ui| {
        let Some(metasprite_index) = app.selected_metasprite else {
            ui.label("No metasprite selected.");
            return;
        };

        let Some((metasprite_snapshot, tileset_snapshot, palette_snapshot)) =
            app.bundle.as_ref().and_then(|bundle| {
                bundle
                    .metasprites
                    .get(metasprite_index)
                    .cloned()
                    .map(|metasprite| {
                        let tileset =
                            super::find_tileset_for_metasprite(bundle, &metasprite).cloned();
                        let palette = bundle.palette(&metasprite.palette_id).cloned();
                        (metasprite, tileset, palette)
                    })
            })
        else {
            ui.label("Selected metasprite is missing.");
            return;
        };

        let mut add_piece = false;
        let mut remove_piece = None;
        let mut flip_horizontal = false;
        let mut flip_vertical = false;

        ui.label(format!(
            "{}  |  {} piece(s)",
            metasprite_snapshot.id,
            metasprite_snapshot.pieces.len()
        ));
        ui.horizontal_wrapped(|ui| {
            if ui.button("+ Piece").clicked() {
                add_piece = true;
            }
            if ui
                .selectable_label(app.metasprite_place_mode, "Place On Canvas")
                .clicked()
            {
                app.metasprite_place_mode = !app.metasprite_place_mode;
                app.metasprite_drag_state = None;
            }
            if ui
                .add_enabled(
                    app.selected_metasprite_piece.is_some(),
                    egui::Button::new("Flip H"),
                )
                .clicked()
            {
                flip_horizontal = true;
            }
            if ui
                .add_enabled(
                    app.selected_metasprite_piece.is_some(),
                    egui::Button::new("Flip V"),
                )
                .clicked()
            {
                flip_vertical = true;
            }
            if ui
                .add_enabled(
                    app.selected_metasprite_piece.is_some(),
                    egui::Button::new("Remove Selected"),
                )
                .clicked()
            {
                remove_piece = app.selected_metasprite_piece;
            }
        });
        if app.metasprite_place_mode {
            ui.small("Place mode is on: click the canvas to stamp a piece using the selected piece or active tile as the template.");
        }

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label("Canvas");
                match (&tileset_snapshot, &palette_snapshot) {
                    (Some(tileset), Some(palette)) => {
                        draw_metasprite_preview_canvas(
                            app,
                            ui,
                            &metasprite_snapshot,
                            tileset,
                            palette,
                            app.animation_preview_facing,
                            None,
                            true,
                        );
                    }
                    (None, _) => {
                        ui.label("No compatible tileset found for this metasprite.");
                    }
                    (_, None) => {
                        ui.label("No compatible palette found for this metasprite.");
                    }
                }
            });

            ui.separator();

            ui.vertical(|ui| {
                ui.label("Pieces");
                egui::ScrollArea::vertical()
                    .max_height(180.0)
                    .show(ui, |ui| {
                        for (index, piece) in metasprite_snapshot.pieces.iter().enumerate() {
                            if ui
                                .selectable_label(
                                    app.selected_metasprite_piece == Some(index),
                                    format!(
                                        "Piece {}  |  tile {}  |  ({}, {})  |  prio {}",
                                        index + 1,
                                        piece.tile_index,
                                        piece.x,
                                        piece.y,
                                        piece.priority.min(3)
                                    ),
                                )
                                .clicked()
                            {
                                app.selected_metasprite_piece = Some(index);
                            }
                        }
                    });

                if let Some(piece_index) = app.selected_metasprite_piece {
                    if let Some(piece_snapshot) =
                        metasprite_snapshot.pieces.get(piece_index).cloned()
                    {
                        ui.separator();
                        ui.label(format!("Selected piece {}", piece_index + 1));

                        let mut edited_piece = piece_snapshot;
                        let mut changed = false;
                        let max_tile_index = tileset_snapshot
                            .as_ref()
                            .map(|tileset| tileset.tiles.len().saturating_sub(1) as u16)
                            .unwrap_or(u16::MAX);

                        ui.horizontal(|ui| {
                            ui.label("Tile");
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut edited_piece.tile_index)
                                        .range(0..=max_tile_index),
                                )
                                .changed();
                        });
                        ui.horizontal(|ui| {
                            ui.label("X");
                            changed |= ui.add(egui::DragValue::new(&mut edited_piece.x)).changed();
                            ui.label("Y");
                            changed |= ui.add(egui::DragValue::new(&mut edited_piece.y)).changed();
                        });
                        ui.horizontal(|ui| {
                            ui.label("Palette Slot");
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut edited_piece.palette_slot)
                                        .range(0..=7),
                                )
                                .changed();
                            ui.label("Priority");
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut edited_piece.priority)
                                        .range(0..=3),
                                )
                                .changed();
                        });
                        changed |= ui
                            .checkbox(&mut edited_piece.h_flip, "Flip Horizontally")
                            .changed();
                        changed |= ui
                            .checkbox(&mut edited_piece.v_flip, "Flip Vertically")
                            .changed();

                        ui.horizontal_wrapped(|ui| {
                            if ui.small_button("Left").clicked() {
                                edited_piece.x -= 1;
                                changed = true;
                            }
                            if ui.small_button("Right").clicked() {
                                edited_piece.x += 1;
                                changed = true;
                            }
                            if ui.small_button("Up").clicked() {
                                edited_piece.y -= 1;
                                changed = true;
                            }
                            if ui.small_button("Down").clicked() {
                                edited_piece.y += 1;
                                changed = true;
                            }
                            if ui.small_button("Snap +8 X").clicked() {
                                edited_piece.x += 8;
                                changed = true;
                            }
                            if ui.small_button("Snap +8 Y").clicked() {
                                edited_piece.y += 8;
                                changed = true;
                            }
                        });

                        if changed {
                            app.capture_history();
                            if let Some(bundle) = &mut app.bundle {
                                if let Some(metasprite) =
                                    bundle.metasprites.get_mut(metasprite_index)
                                {
                                    if let Some(piece) = metasprite.pieces.get_mut(piece_index) {
                                        *piece = edited_piece;
                                    }
                                }
                            }
                            app.mark_edited(format!(
                                "Updated metasprite '{}'",
                                metasprite_snapshot.id
                            ));
                        }
                    }
                } else if metasprite_snapshot.pieces.is_empty() {
                    ui.separator();
                    ui.label("This metasprite has no pieces yet.");
                } else {
                    ui.separator();
                    ui.label("Select a piece to edit it.");
                }
            });
        });

        if add_piece {
            let mut piece = selected_piece_template(app, &metasprite_snapshot);
            piece.x = 0;
            piece.y = 0;
            add_metasprite_piece(app, metasprite_index, piece, &metasprite_snapshot.id);
        }

        if flip_horizontal || flip_vertical {
            if let Some(piece_index) = app.selected_metasprite_piece {
                app.capture_history();
                if let Some(bundle) = &mut app.bundle {
                    if let Some(piece) = bundle
                        .metasprites
                        .get_mut(metasprite_index)
                        .and_then(|metasprite| metasprite.pieces.get_mut(piece_index))
                    {
                        if flip_horizontal {
                            piece.h_flip = !piece.h_flip;
                        }
                        if flip_vertical {
                            piece.v_flip = !piece.v_flip;
                        }
                    }
                }
                app.mark_edited(format!(
                    "Updated metasprite '{}'",
                    metasprite_snapshot.id
                ));
            }
        }

        if let Some(piece_index) = remove_piece {
            if piece_index < metasprite_snapshot.pieces.len() {
                remove_metasprite_piece(
                    app,
                    metasprite_index,
                    piece_index,
                    &metasprite_snapshot.id,
                );
            }
        }
    });
}

fn selected_piece_template(app: &EditorApp, metasprite: &MetaspriteResource) -> SpriteTileRef {
    app.selected_metasprite_piece
        .and_then(|piece_index| metasprite.pieces.get(piece_index))
        .cloned()
        .or_else(|| metasprite.pieces.last().cloned())
        .unwrap_or(SpriteTileRef {
            tile_index: app.selected_tile as u16,
            x: 0,
            y: 0,
            palette_slot: 0,
            priority: 3,
            h_flip: false,
            v_flip: false,
        })
}

fn add_metasprite_piece(
    app: &mut EditorApp,
    metasprite_index: usize,
    piece: SpriteTileRef,
    metasprite_id: &str,
) {
    app.capture_history();
    if let Some(bundle) = &mut app.bundle {
        if let Some(metasprite) = bundle.metasprites.get_mut(metasprite_index) {
            metasprite.pieces.push(piece);
            app.selected_metasprite_piece =
                sanitize_optional_index(Some(metasprite.pieces.len() - 1), metasprite.pieces.len());
        }
    }
    app.mark_edited(format!("Added piece to metasprite '{}'", metasprite_id));
}

fn remove_metasprite_piece(
    app: &mut EditorApp,
    metasprite_index: usize,
    piece_index: usize,
    metasprite_id: &str,
) {
    app.capture_history();
    if let Some(bundle) = &mut app.bundle {
        if let Some(metasprite) = bundle.metasprites.get_mut(metasprite_index) {
            metasprite.pieces.remove(piece_index);
            app.selected_metasprite_piece =
                sanitize_optional_index(Some(piece_index), metasprite.pieces.len());
        }
    }
    app.mark_edited(format!("Removed piece from metasprite '{}'", metasprite_id));
}

fn draw_metasprite_preview_canvas(
    app: &mut EditorApp,
    ui: &mut egui::Ui,
    metasprite: &MetaspriteResource,
    tileset: &TilesetResource,
    palette: &PaletteResource,
    facing: Facing,
    hitbox: Option<RectI16>,
    interactive: bool,
) {
    let desired = if interactive {
        Vec2::new(280.0, 280.0)
    } else {
        Vec2::new(192.0, 192.0)
    };
    let sense = if interactive {
        Sense::click_and_drag()
    } else {
        Sense::hover()
    };
    let (response, painter) = ui.allocate_painter(desired, sense);
    painter.rect_filled(response.rect, 6.0, Color32::from_rgb(18, 26, 34));

    let layout = MetaspriteCanvasLayout::for_rect(response.rect.shrink(12.0), metasprite);
    let anchor = layout.anchor_point(facing);
    if app.show_metasprite_anchor {
        painter.line_segment(
            [
                anchor + Vec2::new(-10.0, 0.0),
                anchor + Vec2::new(10.0, 0.0),
            ],
            (1.0, Color32::from_rgb(244, 214, 92)),
        );
        painter.line_segment(
            [
                anchor + Vec2::new(0.0, -10.0),
                anchor + Vec2::new(0.0, 10.0),
            ],
            (1.0, Color32::from_rgb(244, 214, 92)),
        );
    }

    let mut draw_order = metasprite.pieces.iter().enumerate().collect::<Vec<_>>();
    draw_order.sort_by_key(|(index, piece)| (piece.priority.min(3), *index));

    let mut piece_rects = Vec::with_capacity(metasprite.pieces.len());
    for (index, piece) in draw_order {
        let Some(tile) = tileset.tiles.get(piece.tile_index as usize) else {
            continue;
        };
        let flip_h = piece.h_flip ^ matches!(facing, Facing::Left);
        let rect = layout.piece_rect(piece, facing);
        super::draw_sprite_tile_pixels(&painter, rect, tile, palette, flip_h, piece.v_flip);
        let is_selected = app.selected_metasprite_piece == Some(index);
        if app.show_metasprite_bounds || is_selected {
            painter.rect_stroke(
                rect,
                0.0,
                (
                    if is_selected { 2.0 } else { 1.0 },
                    if is_selected {
                        Color32::from_rgb(244, 214, 92)
                    } else {
                        Color32::from_white_alpha(72)
                    },
                ),
                StrokeKind::Inside,
            );
        }
        if interactive {
            painter.text(
                rect.center_top() + Vec2::new(0.0, -2.0),
                Align2::CENTER_BOTTOM,
                format!("{}", index + 1),
                FontId::proportional(11.0),
                Color32::WHITE,
            );
        }
        piece_rects.push((index, rect));
    }

    if let Some(hitbox) = hitbox {
        let hitbox_rect = layout.hitbox_rect(hitbox, facing);
        painter.rect_stroke(
            hitbox_rect,
            0.0,
            (2.0, Color32::from_rgb(96, 208, 255)),
            StrokeKind::Inside,
        );
    }

    if interactive {
        if response.clicked_by(egui::PointerButton::Primary) {
            if let Some(pointer) = response.interact_pointer_pos() {
                if app.metasprite_place_mode {
                    if let Some(metasprite_index) = app.selected_metasprite {
                        let mut piece = selected_piece_template(app, metasprite);
                        let snapped = layout.snapped_piece_position(pointer, facing);
                        piece.x = snapped.x;
                        piece.y = snapped.y;
                        add_metasprite_piece(app, metasprite_index, piece, &metasprite.id);
                    }
                } else {
                    let clicked = piece_rects
                        .iter()
                        .rev()
                        .find(|(_, rect)| rect.contains(pointer))
                        .map(|(index, _)| *index);
                    app.selected_metasprite_piece = clicked;
                }
            }
        }

        if response.clicked_by(egui::PointerButton::Secondary) {
            if let Some(pointer) = response.interact_pointer_pos() {
                if let Some((clicked, _)) = piece_rects
                    .iter()
                    .rev()
                    .find(|(_, rect)| rect.contains(pointer))
                {
                    if let Some(metasprite_index) = app.selected_metasprite {
                        app.selected_metasprite_piece = Some(*clicked);
                        remove_metasprite_piece(app, metasprite_index, *clicked, &metasprite.id);
                    }
                }
            }
        }

        if !app.metasprite_place_mode && response.drag_started_by(egui::PointerButton::Primary) {
            app.metasprite_drag_state = response.interact_pointer_pos().and_then(|pointer| {
                let clicked = piece_rects
                    .iter()
                    .rev()
                    .find(|(_, rect)| rect.contains(pointer))
                    .map(|(index, _)| *index)?;
                app.selected_metasprite_piece = Some(clicked);
                let metasprite_index = app.selected_metasprite?;
                let piece = metasprite.pieces.get(clicked)?;
                Some(MetaspriteDragState {
                    metasprite_index,
                    piece_index: clicked,
                    pointer_origin: pointer,
                    piece_origin: PointI16 {
                        x: piece.x,
                        y: piece.y,
                    },
                    history_captured: false,
                    moved: false,
                })
            });
        }

        if !app.metasprite_place_mode && response.dragged_by(egui::PointerButton::Primary) {
            let drag = app.metasprite_drag_state.clone();
            if let (Some(pointer), Some(drag), Some(selected_metasprite)) = (
                response.interact_pointer_pos(),
                drag,
                app.selected_metasprite,
            ) {
                if selected_metasprite == drag.metasprite_index {
                    let delta_x =
                        ((pointer.x - drag.pointer_origin.x) / layout.zoom).round() as i16;
                    let delta_y =
                        ((pointer.y - drag.pointer_origin.y) / layout.zoom).round() as i16;
                    let next_position = PointI16 {
                        x: drag.piece_origin.x + delta_x,
                        y: drag.piece_origin.y + delta_y,
                    };
                    if next_position != drag.piece_origin {
                        if !drag.history_captured {
                            app.capture_history();
                            if let Some(state) = &mut app.metasprite_drag_state {
                                state.history_captured = true;
                            }
                        }
                        if let Some(bundle) = &mut app.bundle {
                            if let Some(metasprite) =
                                bundle.metasprites.get_mut(drag.metasprite_index)
                            {
                                if let Some(piece) = metasprite.pieces.get_mut(drag.piece_index) {
                                    piece.x = next_position.x;
                                    piece.y = next_position.y;
                                }
                            }
                        }
                        if let Some(state) = &mut app.metasprite_drag_state {
                            state.moved = true;
                        }
                    }
                }
            }
        }

        if !app.metasprite_place_mode && response.drag_stopped_by(egui::PointerButton::Primary) {
            if let Some(drag) = app.metasprite_drag_state.take() {
                if drag.moved {
                    let label = app
                        .bundle
                        .as_ref()
                        .and_then(|bundle| bundle.metasprites.get(drag.metasprite_index))
                        .map(|metasprite| metasprite.id.clone())
                        .unwrap_or_else(|| "metasprite".to_string());
                    app.mark_edited(format!("Moved piece in metasprite '{}'", label));
                }
            }
        }
    }
}

fn animation_total_frames(animation: &AnimationResource) -> u32 {
    animation
        .frames
        .iter()
        .map(|frame| frame.duration_frames.max(1) as u32)
        .sum::<u32>()
        .max(1)
}

fn animation_preview_tick(
    app: &EditorApp,
    animation: &AnimationResource,
    time_seconds: f32,
) -> u32 {
    let total_frames = animation_total_frames(animation);
    if app.animation_preview_playing {
        let elapsed_seconds = (time_seconds - app.animation_preview_play_anchor_seconds).max(0.0);
        let raw_tick = app.animation_preview_play_anchor_tick.saturating_add(
            (elapsed_seconds * app.animation_preview_speed.max(0.01) * 60.0) as u32,
        );
        if app.animation_preview_loop {
            raw_tick % total_frames
        } else {
            raw_tick.min(total_frames.saturating_sub(1))
        }
    } else {
        app.animation_preview_scrub_frame
            .min(total_frames.saturating_sub(1))
    }
}

fn animation_frame_index_for_tick(animation: &AnimationResource, tick: u32) -> usize {
    if animation.frames.is_empty() {
        return 0;
    }

    let total_frames = animation_total_frames(animation);
    let tick = tick.min(total_frames.saturating_sub(1));
    let mut cursor = 0_u32;
    for (index, frame) in animation.frames.iter().enumerate() {
        let duration = frame.duration_frames.max(1) as u32;
        if tick < cursor + duration {
            return index;
        }
        cursor += duration;
    }
    animation.frames.len().saturating_sub(1)
}

fn metasprite_for_animation_tick<'a>(
    bundle: &'a ProjectBundle,
    animation: &'a AnimationResource,
    tick: u32,
) -> Option<&'a MetaspriteResource> {
    if animation.frames.is_empty() {
        return None;
    }

    let total_frames = animation_total_frames(animation);
    let tick = tick.min(total_frames.saturating_sub(1));
    let mut cursor = 0_u32;
    for frame in &animation.frames {
        let duration = frame.duration_frames.max(1) as u32;
        if tick < cursor + duration {
            return bundle.metasprite(&frame.metasprite_id);
        }
        cursor += duration;
    }
    animation
        .frames
        .last()
        .and_then(|frame| bundle.metasprite(&frame.metasprite_id))
}

fn set_animation_preview_anchor(app: &mut EditorApp, time_seconds: f32, tick: u32) {
    app.animation_preview_play_anchor_seconds = time_seconds;
    app.animation_preview_play_anchor_tick = tick;
}

fn metasprite_for_entity_preview<'a>(
    bundle: &'a ProjectBundle,
    entity: &'a EntityPlacement,
    app: &EditorApp,
    time_seconds: f32,
) -> Option<&'a MetaspriteResource> {
    if let Some(animation) = bundle.animation(&entity.archetype) {
        let tick = animation_preview_tick(app, animation, time_seconds);
        return metasprite_for_animation_tick(bundle, animation, tick);
    }
    if let Some(metasprite) = bundle.metasprite(&entity.archetype) {
        return Some(metasprite);
    }
    let idle_id = format!("{}_idle", entity.archetype);
    bundle.animation(&idle_id).and_then(|animation| {
        let tick = animation_preview_tick(app, animation, time_seconds);
        metasprite_for_animation_tick(bundle, animation, tick)
    })
}

fn entity_has_animation(bundle: &ProjectBundle, entity: &EntityPlacement) -> bool {
    bundle.animation(&entity.archetype).is_some()
        || bundle
            .animation(&format!("{}_idle", entity.archetype))
            .is_some()
}

struct MetaspriteCanvasLayout {
    origin: Pos2,
    zoom: f32,
    min_x: i16,
    min_y: i16,
    max_x: i16,
}

impl MetaspriteCanvasLayout {
    fn for_rect(frame: Rect, metasprite: &MetaspriteResource) -> Self {
        let min_x = metasprite
            .pieces
            .iter()
            .map(|piece| piece.x)
            .min()
            .unwrap_or(0)
            .min(0);
        let min_y = metasprite
            .pieces
            .iter()
            .map(|piece| piece.y)
            .min()
            .unwrap_or(0)
            .min(0);
        let max_x = metasprite
            .pieces
            .iter()
            .map(|piece| piece.x + 8)
            .max()
            .unwrap_or(8)
            .max(8);
        let max_y = metasprite
            .pieces
            .iter()
            .map(|piece| piece.y + 8)
            .max()
            .unwrap_or(8)
            .max(8);
        let width = (max_x - min_x).max(8) as f32;
        let height = (max_y - min_y).max(8) as f32;
        let zoom = (frame.width() / width)
            .min(frame.height() / height)
            .clamp(2.0, 12.0);
        let origin = frame.center() - Vec2::new(width * zoom * 0.5, height * zoom * 0.5);

        Self {
            origin,
            zoom,
            min_x,
            min_y,
            max_x,
        }
    }

    fn anchor_point(&self, facing: Facing) -> Pos2 {
        let x = match facing {
            Facing::Right => (0 - self.min_x) as f32,
            Facing::Left => self.max_x as f32,
        };
        let y = (0 - self.min_y) as f32;
        self.origin + Vec2::new(x * self.zoom, y * self.zoom)
    }

    fn piece_rect(&self, piece: &SpriteTileRef, facing: Facing) -> Rect {
        let draw_x = match facing {
            Facing::Right => (piece.x - self.min_x) as f32,
            Facing::Left => self.max_x as f32 - (piece.x as f32 + 8.0),
        };
        Rect::from_min_size(
            self.origin
                + Vec2::new(
                    draw_x * self.zoom,
                    (piece.y - self.min_y) as f32 * self.zoom,
                ),
            Vec2::splat(8.0 * self.zoom),
        )
    }

    fn hitbox_rect(&self, hitbox: RectI16, facing: Facing) -> Rect {
        let x = match facing {
            Facing::Right => hitbox.x as f32,
            Facing::Left => -(hitbox.x as f32 + hitbox.width as f32),
        };
        let anchor = self.anchor_point(facing);
        Rect::from_min_size(
            anchor + Vec2::new(x * self.zoom, hitbox.y as f32 * self.zoom),
            Vec2::new(
                hitbox.width as f32 * self.zoom,
                hitbox.height as f32 * self.zoom,
            ),
        )
    }

    fn snapped_piece_position(&self, pointer: Pos2, facing: Facing) -> PointI16 {
        let local_x = ((pointer.x - self.origin.x) / self.zoom / 8.0).round() as i16 * 8;
        let local_y = ((pointer.y - self.origin.y) / self.zoom / 8.0).round() as i16 * 8;
        let x = match facing {
            Facing::Right => local_x + self.min_x,
            Facing::Left => self.max_x - local_x - 8,
        };
        PointI16 {
            x,
            y: local_y + self.min_y,
        }
    }
}

fn facing_label(facing: Facing) -> &'static str {
    match facing {
        Facing::Left => "Left",
        Facing::Right => "Right",
    }
}
