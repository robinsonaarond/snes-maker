use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum DiagnosticGrouping {
    #[default]
    Severity,
    Code,
    Path,
}

impl DiagnosticGrouping {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Severity => "Severity",
            Self::Code => "Code",
            Self::Path => "Path",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct DiagnosticsViewState {
    pub(super) search: String,
    pub(super) show_errors: bool,
    pub(super) show_warnings: bool,
    pub(super) grouping: DiagnosticGrouping,
}

impl DiagnosticsViewState {
    pub(super) fn new() -> Self {
        Self {
            search: String::new(),
            show_errors: true,
            show_warnings: true,
            grouping: DiagnosticGrouping::Severity,
        }
    }
}

impl EditorApp {
    pub(super) fn diagnostic_has_quick_fix_impl(&self, diagnostic: &Diagnostic) -> bool {
        matches!(
            diagnostic.code.as_str(),
            "manifest.missing_entry_scene"
                | "asset.palette_too_large"
                | "asset.missing_palette"
                | "scene.trigger_missing_script"
                | "script.dialogue_missing"
                | "script.target_scene_missing"
                | "scene.duplicate_id"
                | "dialogue.duplicate_id"
                | "manifest.duplicate_physics"
        )
    }

    pub(super) fn apply_diagnostic_quick_fix_impl(&mut self, diagnostic: &Diagnostic) {
        let Some(bundle) = &self.bundle else {
            return;
        };
        let mut edited_bundle = bundle.clone();
        let mut status = None;

        match diagnostic.code.as_str() {
            "manifest.missing_entry_scene" => {
                if let Some(scene) = edited_bundle.scenes.first() {
                    edited_bundle.manifest.gameplay.entry_scene = scene.id.clone();
                    status = Some(format!("Set entry scene to '{}'", scene.id));
                }
            }
            "asset.palette_too_large" => {
                if let Some(palette_id) = diagnostic
                    .path
                    .as_deref()
                    .and_then(|path| path.strip_prefix("palette:"))
                {
                    if let Some(palette) = edited_bundle
                        .palettes
                        .iter_mut()
                        .find(|palette| palette.id == palette_id)
                    {
                        palette.colors.truncate(MAX_COLORS_PER_PALETTE);
                        status = Some(format!(
                            "Trimmed palette '{}' to {} colors",
                            palette.id, MAX_COLORS_PER_PALETTE
                        ));
                    }
                }
            }
            "asset.missing_palette" => {
                let first_palette = edited_bundle
                    .palettes
                    .first()
                    .map(|palette| palette.id.clone());
                if let (Some(tileset_id), Some(palette_id)) = (
                    diagnostic
                        .path
                        .as_deref()
                        .and_then(|path| path.strip_prefix("tileset:")),
                    first_palette,
                ) {
                    if let Some(tileset) = edited_bundle
                        .tilesets
                        .iter_mut()
                        .find(|tileset| tileset.id == tileset_id)
                    {
                        tileset.palette_id = palette_id.clone();
                        status = Some(format!(
                            "Reassigned '{}' to palette '{}'",
                            tileset.id, palette_id
                        ));
                    }
                }
            }
            "scene.trigger_missing_script" => {
                if let Some(scene_id) = diagnostic
                    .path
                    .as_deref()
                    .and_then(|path| path.strip_prefix("scene:"))
                    .map(|path| path.split(':').next().unwrap_or(path))
                {
                    if let Some(scene) = edited_bundle
                        .scenes
                        .iter_mut()
                        .find(|scene| scene.id == scene_id)
                    {
                        let missing_script_ids = scene
                            .triggers
                            .iter()
                            .filter(|trigger| {
                                scene
                                    .scripts
                                    .iter()
                                    .all(|script| script.id != trigger.script_id)
                            })
                            .map(|trigger| trigger.script_id.clone())
                            .collect::<Vec<_>>();
                        for script_id in &missing_script_ids {
                            scene.scripts.push(snesmaker_events::EventScript {
                                id: script_id.clone(),
                                commands: Vec::new(),
                            });
                        }
                        if !missing_script_ids.is_empty() {
                            status = Some(format!(
                                "Added {} missing script stub(s) to '{}'",
                                missing_script_ids.len(),
                                scene.id
                            ));
                        }
                    }
                }
            }
            "script.dialogue_missing" => {
                let placeholder_id = "auto_dialogue".to_string();
                if edited_bundle
                    .dialogues
                    .iter()
                    .all(|dialogue| dialogue.id != placeholder_id)
                {
                    edited_bundle
                        .dialogues
                        .push(snesmaker_events::DialogueGraph {
                            id: placeholder_id.clone(),
                            opening_node: "start".to_string(),
                            nodes: vec![snesmaker_events::DialogueNode {
                                id: "start".to_string(),
                                speaker: "System".to_string(),
                                text: "Placeholder dialogue".to_string(),
                                commands: Vec::new(),
                                choices: Vec::new(),
                                next: None,
                            }],
                        });
                }

                let valid_dialogues = edited_bundle
                    .dialogues
                    .iter()
                    .map(|dialogue| dialogue.id.clone())
                    .collect::<BTreeSet<_>>();

                for scene in &mut edited_bundle.scenes {
                    for script in &mut scene.scripts {
                        for command in &mut script.commands {
                            if let snesmaker_events::EventCommand::ShowDialogue {
                                dialogue_id,
                                ..
                            } = command
                            {
                                if !valid_dialogues.contains(dialogue_id) {
                                    *dialogue_id = placeholder_id.clone();
                                }
                            }
                        }
                    }
                }
                status = Some(
                    "Redirected missing dialogue references to a placeholder dialogue".to_string(),
                );
            }
            "script.target_scene_missing" => {
                let fallback_scene = edited_bundle.scenes.first().map(|scene| scene.id.clone());
                if let Some(fallback_scene) = fallback_scene {
                    let valid_scenes = edited_bundle
                        .scenes
                        .iter()
                        .map(|scene| scene.id.clone())
                        .collect::<BTreeSet<_>>();
                    for scene in &mut edited_bundle.scenes {
                        for script in &mut scene.scripts {
                            for command in &mut script.commands {
                                if let snesmaker_events::EventCommand::LoadScene {
                                    scene_id, ..
                                } = command
                                {
                                    if !valid_scenes.contains(scene_id) {
                                        *scene_id = fallback_scene.clone();
                                    }
                                }
                            }
                        }
                    }
                    status = Some(format!(
                        "Redirected missing scene loads to '{}'",
                        fallback_scene
                    ));
                }
            }
            "scene.duplicate_id" => {
                let mut seen = BTreeSet::new();
                for scene in &mut edited_bundle.scenes {
                    if !seen.insert(scene.id.clone()) {
                        scene.id = next_unique_layer_id(&seen, &scene.id);
                        seen.insert(scene.id.clone());
                    }
                }
                status = Some("Renamed duplicate scene ids".to_string());
            }
            "dialogue.duplicate_id" => {
                let mut seen = BTreeSet::new();
                for dialogue in &mut edited_bundle.dialogues {
                    if !seen.insert(dialogue.id.clone()) {
                        dialogue.id = next_unique_layer_id(&seen, &dialogue.id);
                        seen.insert(dialogue.id.clone());
                    }
                }
                status = Some("Renamed duplicate dialogue ids".to_string());
            }
            "manifest.duplicate_physics" => {
                let mut seen = BTreeSet::new();
                for preset in &mut edited_bundle.manifest.gameplay.physics_presets {
                    if !seen.insert(preset.id.clone()) {
                        preset.id = next_unique_layer_id(&seen, &preset.id);
                        seen.insert(preset.id.clone());
                    }
                }
                status = Some("Renamed duplicate physics preset ids".to_string());
            }
            _ => {}
        }

        if let Some(status) = status {
            self.capture_history();
            self.bundle = Some(edited_bundle);
            self.mark_edited(status);
        }
    }

    pub(super) fn draw_diagnostics_panel(&mut self, ui: &mut egui::Ui) {
        let mut navigate_to = None;
        let mut quick_fix = None;

        ui.heading("Diagnostics");
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.diagnostics_view.search)
                    .hint_text("Search message, code, or asset path"),
            );
            ui.checkbox(&mut self.diagnostics_view.show_errors, "Errors");
            ui.checkbox(&mut self.diagnostics_view.show_warnings, "Warnings");
            egui::ComboBox::from_label("Group")
                .selected_text(self.diagnostics_view.grouping.label())
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.diagnostics_view.grouping,
                        DiagnosticGrouping::Severity,
                        DiagnosticGrouping::Severity.label(),
                    );
                    ui.selectable_value(
                        &mut self.diagnostics_view.grouping,
                        DiagnosticGrouping::Code,
                        DiagnosticGrouping::Code.label(),
                    );
                    ui.selectable_value(
                        &mut self.diagnostics_view.grouping,
                        DiagnosticGrouping::Path,
                        DiagnosticGrouping::Path.label(),
                    );
                });
        });

        ui.separator();
        ui.collapsing("Budgets", |ui| {
            let tile_peak = self
                .bundle
                .as_ref()
                .map(|bundle| {
                    bundle
                        .tilesets
                        .iter()
                        .map(|tileset| tileset.tiles.len())
                        .max()
                        .unwrap_or(0)
                })
                .unwrap_or(0);
            let max_rom_banks = self
                .bundle
                .as_ref()
                .map(|bundle| bundle.manifest.build.rom_bank_count.max(1) as usize)
                .unwrap_or(1);
            let tile_fraction = (tile_peak as f32 / MAX_TILESET_TILES as f32).clamp(0.0, 1.0);
            let total_palette_capacity = MAX_PALETTES * MAX_COLORS_PER_PALETTE;
            let palette_fraction = (self.report.budgets.palette_colors as f32
                / total_palette_capacity as f32)
                .clamp(0.0, 1.0);
            let rom_fraction = (self.report.budgets.estimated_rom_banks as f32
                / max_rom_banks as f32)
                .clamp(0.0, 1.0);
            let metasprite_fraction = (self.report.budgets.metasprite_piece_peak as f32
                / MAX_METASPRITE_TILES_HARD as f32)
                .clamp(0.0, 1.0);
            let rom_bytes_capacity = max_rom_banks * ROM_BANK_SIZE;
            let rom_bytes_fraction = (self.report.budgets.estimated_rom_bytes as f32
                / rom_bytes_capacity as f32)
                .clamp(0.0, 1.0);
            let metasprite_warning =
                self.report.budgets.metasprite_piece_peak >= MAX_METASPRITE_TILES_WARN;

            ui.label("Tileset peak");
            ui.add(
                egui::ProgressBar::new(tile_fraction)
                    .text(format!("{} / {} tiles", tile_peak, MAX_TILESET_TILES)),
            );
            ui.label("Palette colors");
            ui.add(egui::ProgressBar::new(palette_fraction).text(format!(
                "{} / {} colors",
                self.report.budgets.palette_colors, total_palette_capacity
            )));
            ui.label("Metasprite peak");
            ui.add(egui::ProgressBar::new(metasprite_fraction).text(format!(
                "{} / {} pieces{}",
                self.report.budgets.metasprite_piece_peak,
                MAX_METASPRITE_TILES_HARD,
                if metasprite_warning {
                    " (warning zone)"
                } else {
                    ""
                }
            )));
            ui.label("ROM banks");
            ui.add(egui::ProgressBar::new(rom_fraction).text(format!(
                "{} / {} bank(s)",
                self.report.budgets.estimated_rom_banks, max_rom_banks
            )));
            ui.label("ROM bytes");
            ui.add(egui::ProgressBar::new(rom_bytes_fraction).text(format!(
                "{} / {} bytes",
                self.report.budgets.estimated_rom_bytes, rom_bytes_capacity
            )));
        });

        let diagnostics = self.filtered_diagnostics();
        if diagnostics.is_empty() {
            ui.separator();
            ui.label("No diagnostics match the current filters.");
        } else {
            ui.separator();
            match self.diagnostics_view.grouping {
                DiagnosticGrouping::Severity => {
                    for (heading, severity) in
                        [("Errors", Severity::Error), ("Warnings", Severity::Warning)]
                    {
                        let group = diagnostics
                            .iter()
                            .filter(|diagnostic| diagnostic.severity == severity)
                            .cloned()
                            .collect::<Vec<_>>();
                        if group.is_empty() {
                            continue;
                        }
                        ui.collapsing(format!("{} ({})", heading, group.len()), |ui| {
                            for diagnostic in &group {
                                draw_diagnostic_row(
                                    ui,
                                    diagnostic,
                                    self.diagnostic_has_quick_fix_impl(diagnostic),
                                    &mut navigate_to,
                                    &mut quick_fix,
                                );
                            }
                        });
                    }
                }
                DiagnosticGrouping::Code => {
                    let mut groups = BTreeMap::<String, Vec<Diagnostic>>::new();
                    for diagnostic in diagnostics {
                        groups
                            .entry(diagnostic.code.clone())
                            .or_default()
                            .push(diagnostic);
                    }
                    for (code, group) in groups {
                        ui.collapsing(format!("{} ({})", code, group.len()), |ui| {
                            for diagnostic in &group {
                                draw_diagnostic_row(
                                    ui,
                                    diagnostic,
                                    self.diagnostic_has_quick_fix_impl(diagnostic),
                                    &mut navigate_to,
                                    &mut quick_fix,
                                );
                            }
                        });
                    }
                }
                DiagnosticGrouping::Path => {
                    let mut groups = BTreeMap::<String, Vec<Diagnostic>>::new();
                    for diagnostic in diagnostics {
                        let key = diagnostic
                            .path
                            .clone()
                            .unwrap_or_else(|| "unscoped".to_string());
                        groups.entry(key).or_default().push(diagnostic);
                    }
                    for (path, group) in groups {
                        ui.collapsing(format!("{} ({})", path, group.len()), |ui| {
                            for diagnostic in &group {
                                draw_diagnostic_row(
                                    ui,
                                    diagnostic,
                                    self.diagnostic_has_quick_fix_impl(diagnostic),
                                    &mut navigate_to,
                                    &mut quick_fix,
                                );
                            }
                        });
                    }
                }
            }
        }

        if let Some(path) = navigate_to {
            self.navigate_to_diagnostic_path(&path);
        }
        if let Some(diagnostic) = quick_fix {
            self.apply_diagnostic_quick_fix_impl(&diagnostic);
        }
    }
}
