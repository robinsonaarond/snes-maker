use super::*;

pub(super) use super::SceneCanvasOutcome;

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_scene_canvas(
    ui: &mut egui::Ui,
    bundle: &ProjectBundle,
    scene_index: usize,
    zoom: f32,
    camera_offset: Vec2,
    viewport_size: Vec2,
    show_grid: bool,
    show_collision: bool,
    selected_layer: usize,
    selected_tile: usize,
    selected_spawn: Option<usize>,
    selected_checkpoint: Option<usize>,
    selected_entity: Option<usize>,
    selected_trigger: Option<usize>,
    selected_prefab_instance: Option<usize>,
    current_selection: Option<&SceneSelection>,
    selection_drag_anchor: Option<(usize, usize)>,
    selection_mode: bool,
    solo_layer: Option<(usize, usize)>,
    solo_group: Option<SceneObjectGroup>,
    show_spawns: bool,
    show_checkpoints: bool,
    show_entities: bool,
    show_triggers: bool,
    time_seconds: f32,
) -> SceneCanvasOutcome {
    let Some(raw_scene) = bundle.scenes.get(scene_index) else {
        ui.label("No scene selected.");
        return SceneCanvasOutcome::default();
    };
    let scene = bundle.resolve_scene(raw_scene);
    if scene.layers.is_empty() {
        ui.label("Scene has no tile layer.");
        return SceneCanvasOutcome::default();
    }
    let selected_layer = selected_layer.min(scene.layers.len().saturating_sub(1));
    let render_layers = scene
        .layers
        .iter()
        .enumerate()
        .filter(|(layer_index, _)| {
            solo_layer.is_none() || solo_layer == Some((scene_index, *layer_index))
        })
        .filter(|(_, layer)| layer.visible)
        .filter_map(|(_, layer)| {
            let tileset = bundle.tileset(&layer.tileset_id)?;
            let palette = bundle.palette(&tileset.palette_id)?;
            Some((layer, tileset, palette))
        })
        .collect::<Vec<_>>();

    let cell_size = 8.0 * zoom;
    let desired_size = Vec2::new(
        scene.size_tiles.width as f32 * cell_size,
        scene.size_tiles.height as f32 * cell_size,
    );
    let viewport_size = Vec2::new(viewport_size.x.max(64.0), viewport_size.y.max(64.0));
    let (viewport_rect, response) = ui.allocate_exact_size(viewport_size, Sense::click_and_drag());
    let painter = ui.painter().with_clip_rect(viewport_rect);
    painter.rect_filled(viewport_rect, 6.0, Color32::from_rgb(18, 26, 34));
    let rect = Rect::from_min_size(viewport_rect.min - camera_offset, desired_size);

    let visible_min_x = ((camera_offset.x / cell_size).floor().max(0.0) as usize)
        .min(scene.size_tiles.width as usize);
    let visible_max_x = (((camera_offset.x + viewport_rect.width()) / cell_size)
        .ceil()
        .max(0.0) as usize)
        .min(scene.size_tiles.width as usize);
    let visible_min_y = ((camera_offset.y / cell_size).floor().max(0.0) as usize)
        .min(scene.size_tiles.height as usize);
    let visible_max_y = (((camera_offset.y + viewport_rect.height()) / cell_size)
        .ceil()
        .max(0.0) as usize)
        .min(scene.size_tiles.height as usize);

    for tile_y in visible_min_y..visible_max_y {
        for tile_x in visible_min_x..visible_max_x {
            let cell_index = tile_y * scene.size_tiles.width as usize + tile_x;
            let cell_rect = Rect::from_min_size(
                rect.min + Vec2::new(tile_x as f32 * cell_size, tile_y as f32 * cell_size),
                Vec2::splat(cell_size),
            );

            let mut drew_tile = false;
            for (layer, tileset, palette) in &render_layers {
                let tile_index = layer.tiles.get(cell_index).copied().unwrap_or_default() as usize;
                if let Some(tile) = tileset.tiles.get(tile_index) {
                    draw_tile_pixels(&painter, cell_rect, tile, palette);
                    drew_tile = true;
                }
            }
            if !drew_tile {
                painter.rect_filled(cell_rect, 0.0, Color32::BLACK);
            }

            if show_collision {
                if scene
                    .collision
                    .solids
                    .get(cell_index)
                    .copied()
                    .unwrap_or(false)
                {
                    painter.rect_filled(
                        cell_rect,
                        0.0,
                        Color32::from_rgba_premultiplied(44, 54, 66, 96),
                    );
                }
                if scene
                    .collision
                    .ladders
                    .get(cell_index)
                    .copied()
                    .unwrap_or(false)
                {
                    painter.rect_filled(
                        cell_rect,
                        0.0,
                        Color32::from_rgba_premultiplied(52, 180, 88, 92),
                    );
                }
                if scene
                    .collision
                    .hazards
                    .get(cell_index)
                    .copied()
                    .unwrap_or(false)
                {
                    painter.rect_filled(
                        cell_rect,
                        0.0,
                        Color32::from_rgba_premultiplied(212, 84, 32, 108),
                    );
                }
            }

            if show_grid {
                painter.rect_stroke(
                    cell_rect,
                    0.0,
                    (1.0, Color32::from_gray(26)),
                    StrokeKind::Inside,
                );
            }
        }
    }

    if show_spawns && (solo_group.is_none() || solo_group == Some(SceneObjectGroup::Spawns)) {
        draw_spawns(
            &painter,
            rect,
            zoom,
            &scene.spawns,
            selected_spawn,
            Color32::from_rgb(64, 212, 255),
        );
    }
    if show_checkpoints
        && (solo_group.is_none() || solo_group == Some(SceneObjectGroup::Checkpoints))
    {
        draw_checkpoints(
            &painter,
            rect,
            zoom,
            &scene.checkpoints,
            selected_checkpoint,
            Color32::from_rgb(255, 220, 72),
        );
    }
    if show_triggers && (solo_group.is_none() || solo_group == Some(SceneObjectGroup::Triggers)) {
        draw_triggers(&painter, rect, zoom, &scene.triggers, selected_trigger);
    }
    if show_entities && (solo_group.is_none() || solo_group == Some(SceneObjectGroup::Entities)) {
        draw_entities(
            &painter,
            rect,
            zoom,
            bundle,
            &scene.entities,
            selected_entity,
            time_seconds,
        );
    }
    if solo_group.is_none() || solo_group == Some(SceneObjectGroup::Prefabs) {
        draw_prefab_instances(
            &painter,
            rect,
            zoom,
            bundle,
            &raw_scene.prefab_instances,
            selected_prefab_instance,
        );
    }

    if let Some(selection) = current_selection {
        draw_scene_selection_overlay(&painter, rect, zoom, raw_scene, selection);
    }

    let selected_rect =
        selected_tile_preview_rect(rect, &scene, selected_layer, selected_tile, zoom);
    if let Some(highlight) = selected_rect {
        painter.rect_stroke(
            highlight,
            0.0,
            (2.0, Color32::from_rgb(255, 240, 96)),
            StrokeKind::Inside,
        );
    }

    let hover_pos = response.hover_pos();
    let hovered_tile = hover_pos.and_then(|position| {
        world_tile_from_pos(position, rect, &scene, zoom).map(|(x, y, _)| (x, y))
    });
    let sampling_mode = !selection_mode && ui.input(|input| input.modifiers.alt);
    let sampled_cell = if sampling_mode && response.clicked_by(egui::PointerButton::Primary) {
        response.interact_pointer_pos().and_then(|position| {
            world_tile_from_pos(position, rect, &scene, zoom).map(|(_, _, index)| index)
        })
    } else {
        None
    };

    let interact_tile = response.interact_pointer_pos().and_then(|position| {
        world_tile_from_pos(position, rect, &scene, zoom).map(|(x, y, _)| (x, y))
    });

    let selection_started = selection_mode.then_some(()).and_then(|_| {
        response
            .drag_started_by(egui::PointerButton::Primary)
            .then_some(interact_tile)
            .flatten()
    });
    let selection_finished = selection_mode.then_some(()).and_then(|_| {
        response
            .drag_stopped_by(egui::PointerButton::Primary)
            .then_some(interact_tile)
            .flatten()
    });
    let selection_clicked = selection_mode.then_some(()).and_then(|_| {
        response
            .clicked_by(egui::PointerButton::Primary)
            .then_some(interact_tile)
            .flatten()
    });

    if selection_mode {
        if let Some(anchor) = selection_drag_anchor.or(selection_started) {
            if let Some(current) = interact_tile.or(hovered_tile) {
                let preview_rect = TileSelectionRect::from_points(anchor, current);
                draw_tile_selection_rect(
                    &painter,
                    rect,
                    zoom,
                    preview_rect,
                    Color32::from_rgba_premultiplied(255, 224, 96, 36),
                    Color32::from_rgb(255, 232, 120),
                );
            }
        }
    }

    let primary_cell =
        if !selection_mode && !sampling_mode && ui.input(|input| input.pointer.primary_down()) {
            hover_pos.and_then(|position| {
                world_tile_from_pos(position, rect, &scene, zoom).map(|(_, _, index)| index)
            })
        } else {
            None
        };

    let secondary_cell = if !selection_mode && ui.input(|input| input.pointer.secondary_down()) {
        hover_pos.and_then(|position| {
            world_tile_from_pos(position, rect, &scene, zoom).map(|(_, _, index)| index)
        })
    } else {
        None
    };

    let world_cell_position = hover_pos
        .and_then(|position| world_tile_from_pos(position, rect, &scene, zoom))
        .map(|(tile_x, tile_y, _)| PointI16 {
            x: (tile_x * 8) as i16,
            y: (tile_y * 8) as i16,
        })
        .unwrap_or(PointI16 { x: 0, y: 0 });

    SceneCanvasOutcome {
        viewport_rect,
        hovered_tile,
        sampled_cell,
        primary_cell,
        secondary_cell,
        world_cell_position,
        selection_started,
        selection_finished,
        selection_clicked,
    }
}
