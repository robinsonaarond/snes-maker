use super::*;

pub(super) struct LoadedSheetPreview {
    pub(super) rgba: Vec<u8>,
    pub(super) size: [usize; 2],
    pub(super) texture: TextureHandle,
}

#[derive(Default)]
pub(super) struct SpriteSheetImportState {
    pub(super) open: bool,
    pub(super) source_path: String,
    pub(super) base_id: String,
    pub(super) animation_id: String,
    pub(super) frame_width_px: u32,
    pub(super) frame_height_px: u32,
    pub(super) frame_count: usize,
    pub(super) columns: usize,
    pub(super) frame_duration: u8,
    pub(super) target_tileset_id: String,
    pub(super) target_palette_id: String,
    pub(super) status: String,
    pub(super) preview: Option<LoadedSheetPreview>,
}

impl SpriteSheetImportState {
    pub(super) fn with_defaults() -> Self {
        Self {
            open: false,
            source_path: String::new(),
            base_id: "player_run".to_string(),
            animation_id: "player_run".to_string(),
            frame_width_px: 16,
            frame_height_px: 16,
            frame_count: 1,
            columns: 1,
            frame_duration: 8,
            target_tileset_id: String::new(),
            target_palette_id: String::new(),
            status: String::new(),
            preview: None,
        }
    }

    pub(super) fn sync_to_bundle(&mut self, bundle: &ProjectBundle) {
        if self.target_tileset_id.is_empty() {
            if let Some(tileset) = bundle.tilesets.first() {
                self.target_tileset_id = tileset.id.clone();
            }
        }
        if self.target_palette_id.is_empty() {
            if let Some(palette) = bundle.palettes.first() {
                self.target_palette_id = palette.id.clone();
            }
        }
    }

    pub(super) fn to_request(&self) -> SpriteSheetImportRequest {
        SpriteSheetImportRequest {
            base_id: self.base_id.clone(),
            animation_id: self.animation_id.clone(),
            frame_width_px: self.frame_width_px,
            frame_height_px: self.frame_height_px,
            frame_count: self.frame_count,
            columns: self.columns,
            frame_duration: self.frame_duration,
            target_tileset_id: self.target_tileset_id.clone(),
            target_palette_id: self.target_palette_id.clone(),
        }
    }
}

impl EditorApp {
    pub(super) fn draw_import_window_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.bundle.is_none() {
            ui.label("Load a project before importing sprites.");
            return;
        }

        ui.heading("Project Sprite Library");
        ui.label(format!(
            "Store reusable PNG sheets in {} so they travel with the project.",
            PROJECT_SPRITE_SOURCE_DIR
        ));
        ui.horizontal(|ui| {
            if ui.button("Add PNG To Project...").clicked() {
                self.import_sprite_source_into_project(ctx);
            }
            if ui.button("Browse Anywhere...").clicked() {
                self.select_import_file(ctx);
            }
        });
        match self.list_project_sprite_sources() {
            Ok(paths) if paths.is_empty() => {
                ui.label("No project-local sprite sheets yet.");
            }
            Ok(paths) => {
                egui::ScrollArea::vertical()
                    .max_height(PROJECT_SPRITE_LIBRARY_MAX_HEIGHT)
                    .show(ui, |ui| {
                        for path in paths {
                            let label = self.project_sprite_source_relative_path(&path);
                            if ui
                                .selectable_label(self.import_state.source_path == label, &label)
                                .clicked()
                            {
                                self.load_import_preview_from_path(ctx, path.as_std_path(), label);
                            }
                        }
                    });
            }
            Err(error) => {
                ui.label(error.to_string());
            }
        }

        ui.separator();
        ui.label("Loaded source");
        ui.add_enabled(
            false,
            egui::TextEdit::singleline(&mut self.import_state.source_path)
                .desired_width(f32::INFINITY),
        );

        ui.horizontal(|ui| {
            ui.label("Base metasprite id");
            ui.text_edit_singleline(&mut self.import_state.base_id);
        });
        ui.horizontal(|ui| {
            ui.label("Animation id");
            ui.text_edit_singleline(&mut self.import_state.animation_id);
        });
        ui.horizontal(|ui| {
            ui.label("Frame width px");
            ui.add(egui::DragValue::new(&mut self.import_state.frame_width_px).range(8..=1024));
            ui.label("Frame height px");
            ui.add(egui::DragValue::new(&mut self.import_state.frame_height_px).range(8..=1024));
        });
        ui.horizontal(|ui| {
            ui.label("Frame count");
            ui.add(egui::DragValue::new(&mut self.import_state.frame_count).range(1..=512));
            ui.label("Columns");
            ui.add(egui::DragValue::new(&mut self.import_state.columns).range(1..=128));
            ui.label("Duration");
            ui.add(egui::DragValue::new(&mut self.import_state.frame_duration).range(1..=120));
        });

        if let Some(bundle) = &self.bundle {
            self.import_state.sync_to_bundle(bundle);
            egui::ComboBox::from_label("Target tileset")
                .selected_text(&self.import_state.target_tileset_id)
                .show_ui(ui, |ui| {
                    for tileset in &bundle.tilesets {
                        ui.selectable_value(
                            &mut self.import_state.target_tileset_id,
                            tileset.id.clone(),
                            &tileset.id,
                        );
                    }
                });
            egui::ComboBox::from_label("Target palette")
                .selected_text(&self.import_state.target_palette_id)
                .show_ui(ui, |ui| {
                    for palette in &bundle.palettes {
                        ui.selectable_value(
                            &mut self.import_state.target_palette_id,
                            palette.id.clone(),
                            &palette.id,
                        );
                    }
                });
        }

        ui.separator();
        if let Some(preview) = &self.import_state.preview {
            ui.label(format!(
                "Loaded sheet: {}x{} px",
                preview.size[0], preview.size[1]
            ));
            let size = fit_size(
                Vec2::new(preview.size[0] as f32, preview.size[1] as f32),
                Vec2::new(480.0, 280.0),
            );
            ui.add(
                egui::Image::from_texture(&preview.texture)
                    .fit_to_exact_size(size)
                    .sense(Sense::hover()),
            );
        } else {
            ui.label("No sprite sheet loaded yet.");
        }

        ui.separator();
        if ui.button("Import Into Project").clicked() {
            self.import_sprite_sheet();
        }
        if !self.import_state.status.is_empty() {
            ui.label(&self.import_state.status);
        }
    }
}
