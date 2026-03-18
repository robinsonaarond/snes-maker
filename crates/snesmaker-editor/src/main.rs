use std::path::PathBuf;

use anyhow::Result;
use camino::Utf8PathBuf;
use eframe::egui::{self, Color32, RichText, StrokeKind, Vec2};
use snesmaker_export::build_rom;
use snesmaker_project::{ProjectBundle, RgbaColor, Tile8};
use snesmaker_validator::{Severity, ValidationReport, validate_project};

fn main() -> Result<()> {
    let project_root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().expect("cwd"));
    let project_root = Utf8PathBuf::from_path_buf(project_root)
        .map_err(|_| anyhow::anyhow!("project path must be utf-8"))?;

    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "SNES Maker",
        native_options,
        Box::new(move |_cc| Ok(Box::new(EditorApp::new(project_root.clone())))),
    )
    .map_err(|error| anyhow::anyhow!(error.to_string()))
}

struct EditorApp {
    project_root: Utf8PathBuf,
    bundle: Option<ProjectBundle>,
    report: ValidationReport,
    selected_scene: usize,
    selected_tile: usize,
    working_tile: Option<Tile8>,
    status: String,
}

impl EditorApp {
    fn new(project_root: Utf8PathBuf) -> Self {
        let mut app = Self {
            project_root,
            bundle: None,
            report: ValidationReport::default(),
            selected_scene: 0,
            selected_tile: 0,
            working_tile: None,
            status: String::new(),
        };
        app.reload();
        app
    }

    fn reload(&mut self) {
        match ProjectBundle::load(&self.project_root) {
            Ok(bundle) => {
                self.report = validate_project(&bundle);
                self.working_tile = bundle
                    .tilesets
                    .first()
                    .and_then(|tileset| tileset.tiles.get(self.selected_tile))
                    .cloned();
                self.bundle = Some(bundle);
                self.status = format!("Loaded {}", self.project_root);
            }
            Err(error) => {
                self.bundle = None;
                self.report = ValidationReport::default();
                self.working_tile = None;
                self.status = error.to_string();
            }
        }
    }
}

impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("SNES Maker");
                ui.label(self.project_root.as_str());
                if ui.button("Reload").clicked() {
                    self.reload();
                }
                if ui.button("Validate").clicked() {
                    if let Some(bundle) = &self.bundle {
                        self.report = validate_project(bundle);
                        self.status = format!(
                            "Validated: {} error(s), {} warning(s)",
                            self.report.errors.len(),
                            self.report.warnings.len()
                        );
                    }
                }
                if ui.button("Build ROM").clicked() {
                    match build_rom(&self.project_root, None) {
                        Ok(outcome) => {
                            self.report = outcome.validation;
                            self.status = if outcome.rom_built {
                                format!("Built ROM at {}", outcome.rom_path)
                            } else {
                                format!("Generated build assets at {}", outcome.build_dir)
                            };
                        }
                        Err(error) => self.status = error.to_string(),
                    }
                }
            });
            ui.label(&self.status);
        });

        egui::SidePanel::left("scenes")
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("Scenes");
                if let Some(bundle) = &self.bundle {
                    for (index, scene) in bundle.scenes.iter().enumerate() {
                        if ui
                            .selectable_label(self.selected_scene == index, &scene.id)
                            .clicked()
                        {
                            self.selected_scene = index;
                        }
                    }
                    ui.separator();
                    ui.heading("Budgets");
                    ui.label(format!("Scenes: {}", self.report.budgets.scene_count));
                    ui.label(format!("Tiles: {}", self.report.budgets.unique_tiles));
                    ui.label(format!(
                        "Palette colors: {}",
                        self.report.budgets.palette_colors
                    ));
                    ui.label(format!(
                        "Estimated ROM banks: {}",
                        self.report.budgets.estimated_rom_banks
                    ));
                } else {
                    ui.label("No project loaded.");
                }
            });

        egui::SidePanel::right("diagnostics")
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("Diagnostics");
                if self.report.errors.is_empty() && self.report.warnings.is_empty() {
                    ui.label("No diagnostics.");
                }
                for diagnostic in self.report.errors.iter().chain(self.report.warnings.iter()) {
                    let color = match diagnostic.severity {
                        Severity::Error => Color32::from_rgb(180, 48, 48),
                        Severity::Warning => Color32::from_rgb(196, 136, 24),
                    };
                    ui.colored_label(
                        color,
                        format!("[{}] {}", diagnostic.code, diagnostic.message),
                    );
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(bundle) = &self.bundle {
                let scene = bundle.scenes.get(self.selected_scene).cloned();
                let physics_presets = bundle.manifest.gameplay.physics_presets.clone();
                let tileset = bundle.tilesets.first().cloned();
                let palette = bundle.palettes.first().cloned();

                if let Some(scene) = scene {
                    ui.heading(RichText::new(&scene.id).size(24.0));
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "Scene size: {}x{} tiles",
                            scene.size_tiles.width, scene.size_tiles.height
                        ));
                        ui.label(format!(
                            "Chunk size: {}x{}",
                            scene.chunk_size_tiles.width, scene.chunk_size_tiles.height
                        ));
                        ui.label(format!("Entities: {}", scene.entities.len()));
                        ui.label(format!("Triggers: {}", scene.triggers.len()));
                    });
                    ui.separator();
                    ui.label("Physics presets");
                    for preset in &physics_presets {
                        ui.monospace(format!(
                            "{}: gravity={} run_speed={} jump={}",
                            preset.id,
                            preset.gravity_fp,
                            preset.max_run_speed_fp,
                            preset.jump_velocity_fp
                        ));
                    }
                    ui.separator();
                    if let Some(tileset) = tileset {
                        ui.horizontal(|ui| {
                            ui.heading("Tile Editor");
                            ui.label(format!("Tileset: {}", tileset.name));
                            ui.add(
                                egui::Slider::new(
                                    &mut self.selected_tile,
                                    0..=tileset.tiles.len().saturating_sub(1),
                                )
                                .text("tile"),
                            );
                            if ui.button("Load Tile").clicked() {
                                self.working_tile = tileset.tiles.get(self.selected_tile).cloned();
                            }
                        });

                        if self.working_tile.is_none() {
                            self.working_tile = tileset.tiles.get(self.selected_tile).cloned();
                        }

                        if let (Some(tile), Some(palette)) = (&mut self.working_tile, palette.as_ref()) {
                            draw_tile_editor(ui, tile, palette);
                            ui.label("Tile edits are currently in-memory only. Save/export wiring comes next.");
                        }
                    }
                }
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("Open a project path as the first argument to inspect it here.");
                });
            }
        });
    }
}

fn draw_tile_editor(
    ui: &mut egui::Ui,
    tile: &mut Tile8,
    palette: &snesmaker_project::PaletteResource,
) {
    let cell_size = 24.0;
    let desired_size = Vec2::new(cell_size * 8.0, cell_size * 8.0);
    let (response, painter) = ui.allocate_painter(desired_size, egui::Sense::click());
    let rect = response.rect;

    for y in 0..8 {
        for x in 0..8 {
            let index = y * 8 + x;
            let palette_index = tile.pixels.get(index).copied().unwrap_or_default() as usize;
            let color = palette
                .colors
                .get(palette_index)
                .map(to_color32)
                .unwrap_or(Color32::BLACK);
            let cell = egui::Rect::from_min_size(
                rect.min + Vec2::new(x as f32 * cell_size, y as f32 * cell_size),
                Vec2::splat(cell_size),
            );
            painter.rect_filled(cell, 0.0, color);
            painter.rect_stroke(cell, 0.0, (1.0, Color32::from_gray(30)), StrokeKind::Inside);

            if response.clicked()
                && response
                    .interact_pointer_pos()
                    .is_some_and(|pos| cell.contains(pos))
            {
                if let Some(pixel) = tile.pixels.get_mut(index) {
                    let max_index = palette.colors.len().saturating_sub(1) as u8;
                    *pixel = if max_index == 0 {
                        0
                    } else {
                        (*pixel + 1) % (max_index + 1)
                    };
                }
            }
        }
    }
}

fn to_color32(color: &RgbaColor) -> Color32 {
    Color32::from_rgba_premultiplied(color.r, color.g, color.b, color.a)
}
