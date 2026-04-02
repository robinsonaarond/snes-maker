use super::*;
use snesmaker_events::{DialogueChoice, DialogueNode};
use std::collections::VecDeque;

pub(super) fn draw_events_tab(app: &mut EditorApp, ui: &mut egui::Ui) {
    let Some(bundle_snapshot) = app.bundle.as_ref().cloned() else {
        ui.heading("Events");
        ui.label("Load a project to edit dialogues and scripts.");
        return;
    };
    let Some(scene_snapshot) = bundle_snapshot.scenes.get(app.selected_scene).cloned() else {
        ui.heading("Events");
        ui.label("Select a scene to edit its event scripts.");
        return;
    };
    let catalog = EventCatalog::from_app(app, &bundle_snapshot, &scene_snapshot);

    let mut add_dialogue = false;
    let mut remove_dialogue_index = None;
    let mut add_script = false;
    let mut remove_script = None;

    ui.heading("Events");
    ui.label(format!(
        "Scene: {}  |  {} dialogue(s)  |  {} script(s)",
        scene_snapshot.id,
        bundle_snapshot.dialogues.len(),
        scene_snapshot.scripts.len()
    ));
    ui.separator();

    ui.columns(2, |columns| {
        columns[0].vertical(|ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Dialogues");
                if ui.button("+ Dialogue").clicked() {
                    add_dialogue = true;
                }
                if ui
                    .add_enabled(
                        app.selected_dialogue.is_some(),
                        egui::Button::new("Remove Dialogue"),
                    )
                    .clicked()
                {
                    remove_dialogue_index = app.selected_dialogue;
                }
            });
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .show(ui, |ui| {
                    for (index, dialogue) in bundle_snapshot.dialogues.iter().enumerate() {
                        let issue_count = count_dialogue_diagnostics(&app.report, &dialogue.id);
                        ui.horizontal(|ui| {
                            if ui
                                .selectable_label(
                                    app.selected_dialogue == Some(index),
                                    format!("{} ({} node(s))", dialogue.id, dialogue.nodes.len()),
                                )
                                .clicked()
                            {
                                app.select_dialogue(index);
                            }
                            if issue_count > 0 {
                                ui.colored_label(
                                    diagnostic_severity_color(Severity::Error),
                                    format!("{} issue(s)", issue_count),
                                );
                            }
                        });
                    }
                    if bundle_snapshot.dialogues.is_empty() {
                        ui.label("No dialogues yet.");
                    }
                });

            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("Scripts in '{}'", scene_snapshot.id));
                if ui.button("+ Script").clicked() {
                    add_script = true;
                }
                if ui
                    .add_enabled(
                        app.selected_script.is_some(),
                        egui::Button::new("Remove Script"),
                    )
                    .clicked()
                {
                    remove_script = app.selected_script;
                }
            });
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .show(ui, |ui| {
                    for (index, script) in scene_snapshot.scripts.iter().enumerate() {
                        let issue_count =
                            count_script_diagnostics(&app.report, &scene_snapshot.id, &script.id);
                        ui.horizontal(|ui| {
                            if ui
                                .selectable_label(
                                    app.selected_script == Some(index),
                                    format!("{} ({} command(s))", script.id, script.commands.len()),
                                )
                                .clicked()
                            {
                                app.select_script(app.selected_scene, index);
                            }
                            if issue_count > 0 {
                                ui.colored_label(
                                    diagnostic_severity_color(Severity::Error),
                                    format!("{} issue(s)", issue_count),
                                );
                            }
                        });
                    }
                    if scene_snapshot.scripts.is_empty() {
                        ui.label("No scripts in this scene yet.");
                    }
                });
        });

        columns[1].vertical(|ui| {
            if let Some(dialogue_index) = app.selected_dialogue {
                draw_dialogue_editor(app, ui, dialogue_index, &bundle_snapshot, &catalog);
            } else if let Some(script_index) = app.selected_script {
                draw_script_editor(app, ui, script_index, &scene_snapshot, &catalog);
            } else {
                ui.label("Select a dialogue or script to edit it here.");
            }
        });
    });

    if add_dialogue {
        let (dialogue_index, dialogue_id) = create_dialogue(app);
        app.select_dialogue(dialogue_index);
        app.status = format!("Added dialogue '{}'", dialogue_id);
    } else if let Some(dialogue_index) = remove_dialogue_index {
        if let Some(dialogue_id) = remove_dialogue(app, dialogue_index) {
            app.status = format!("Removed dialogue '{}'", dialogue_id);
        }
    }

    if add_script {
        app.capture_history();
        if let Some((script_index, script_id)) =
            create_scene_script(app, app.selected_scene, "script")
        {
            app.select_script(app.selected_scene, script_index);
            app.mark_edited(format!("Added script '{}'", script_id));
        }
    } else if let Some(script_index) = remove_script {
        if let Some(script_id) = remove_scene_script(app, app.selected_scene, script_index) {
            app.status = format!("Removed script '{}'", script_id);
        }
    }
}

pub(super) fn draw_trigger_inspector(
    app: &mut EditorApp,
    ui: &mut egui::Ui,
    scene_snapshot: &SceneResource,
) {
    ui.collapsing("Triggers", |ui| {
        let script_ids = scene_snapshot
            .scripts
            .iter()
            .map(|script| script.id.clone())
            .collect::<Vec<_>>();

        ui.horizontal(|ui| {
            if ui.button("+ Trigger").clicked() {
                app.capture_history();
                if let Some(scene) = app.current_scene_mut() {
                    let next_index = scene.triggers.len() + 1;
                    scene.triggers.push(TriggerVolume {
                        id: format!("trigger_{}", next_index),
                        kind: TriggerKind::Touch,
                        rect: RectI16 {
                            x: 8,
                            y: 8,
                            width: 16,
                            height: 16,
                        },
                        script_id: scene
                            .scripts
                            .first()
                            .map(|script| script.id.clone())
                            .unwrap_or_default(),
                    });
                    app.selected_trigger = Some(scene.triggers.len() - 1);
                }
                app.tool = EditorTool::Trigger;
                app.preview_focus = PreviewFocus::None;
                app.mark_edited("Added trigger");
            }
            if ui.button("- Remove").clicked() {
                if let Some(index) = app.selected_trigger {
                    app.capture_history();
                    if let Some(scene) = app.current_scene_mut() {
                        if index < scene.triggers.len() {
                            scene.triggers.remove(index);
                        }
                    }
                    app.selected_trigger = None;
                    app.preview_focus = PreviewFocus::None;
                    app.mark_edited("Removed trigger");
                }
            }
        });

        for (index, trigger) in scene_snapshot.triggers.iter().enumerate() {
            let issue_count =
                count_trigger_diagnostics(&app.report, &scene_snapshot.id, &trigger.id);
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(
                        app.selected_trigger == Some(index),
                        format!("{} ({:?})", trigger.id, trigger.kind),
                    )
                    .clicked()
                {
                    app.selected_trigger = Some(index);
                    app.tool = EditorTool::Trigger;
                    app.preview_focus = PreviewFocus::None;
                }
                if issue_count > 0 {
                    ui.colored_label(
                        diagnostic_severity_color(Severity::Error),
                        format!("{} issue(s)", issue_count),
                    );
                }
            });
        }

        if let Some(index) = app.selected_trigger {
            if let Some(trigger) = scene_snapshot.triggers.get(index) {
                let mut edited = trigger.clone();
                let mut changed = false;
                let mut open_script = None;
                let mut create_bound_script = false;

                changed |= ui.text_edit_singleline(&mut edited.id).changed();
                if script_ids.is_empty() {
                    ui.label("No scripts are available in this scene yet.");
                } else {
                    egui::ComboBox::from_label("Script")
                        .selected_text(if edited.script_id.is_empty() {
                            "Choose script"
                        } else {
                            edited.script_id.as_str()
                        })
                        .show_ui(ui, |ui| {
                            for script_id in &script_ids {
                                changed |= ui
                                    .selectable_value(
                                        &mut edited.script_id,
                                        script_id.clone(),
                                        script_id,
                                    )
                                    .changed();
                            }
                        });
                }
                if let Some(AssetDragPayload::Script { id, .. }) = app.asset_drag_payload.clone() {
                    let drop_response =
                        ui.button(format!("Drop Script Here: {}", edited.script_id));
                    if drop_response.hovered() && ui.input(|input| input.pointer.any_released()) {
                        edited.script_id = id;
                        app.asset_drag_payload = None;
                        changed = true;
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.button("New Script").clicked() {
                        create_bound_script = true;
                    }
                    if let Some(script_index) = scene_snapshot
                        .scripts
                        .iter()
                        .position(|script| script.id == edited.script_id)
                    {
                        if ui.small_button("Open Script").clicked() {
                            open_script = Some(script_index);
                        }
                    }
                });
                let trigger_diagnostics =
                    collect_trigger_diagnostics(&app.report, &scene_snapshot.id, &trigger.id);
                if !trigger_diagnostics.is_empty() {
                    ui.separator();
                    ui.label(format!(
                        "Binding validation ({} issue(s))",
                        trigger_diagnostics.len()
                    ));
                    for diagnostic in &trigger_diagnostics {
                        draw_inline_diagnostic_row(ui, diagnostic, None);
                    }
                }
                changed |= ui
                    .add(egui::DragValue::new(&mut edited.rect.x).speed(1))
                    .changed();
                changed |= ui
                    .add(egui::DragValue::new(&mut edited.rect.y).speed(1))
                    .changed();
                changed |= ui
                    .add(egui::DragValue::new(&mut edited.rect.width).speed(1))
                    .changed();
                changed |= ui
                    .add(egui::DragValue::new(&mut edited.rect.height).speed(1))
                    .changed();
                egui::ComboBox::from_label("Kind")
                    .selected_text(format!("{:?}", edited.kind))
                    .show_ui(ui, |ui| {
                        changed |= ui
                            .selectable_value(&mut edited.kind, TriggerKind::Touch, "Touch")
                            .changed();
                        changed |= ui
                            .selectable_value(&mut edited.kind, TriggerKind::Interact, "Interact")
                            .changed();
                        changed |= ui
                            .selectable_value(
                                &mut edited.kind,
                                TriggerKind::EnterScene,
                                "EnterScene",
                            )
                            .changed();
                        changed |= ui
                            .selectable_value(
                                &mut edited.kind,
                                TriggerKind::DefeatAllEnemies,
                                "DefeatAllEnemies",
                            )
                            .changed();
                    });
                if ui.button("Place Trigger In Scene").clicked() {
                    app.tool = EditorTool::Trigger;
                    app.preview_focus = PreviewFocus::None;
                }

                if create_bound_script {
                    let base = if edited.id.trim().is_empty() {
                        "script"
                    } else {
                        edited.id.as_str()
                    };
                    app.capture_history();
                    if let Some((script_index, script_id)) =
                        create_scene_script(app, app.selected_scene, &format!("{}_script", base))
                    {
                        edited.script_id = script_id.clone();
                        if let Some(scene) = app.current_scene_mut() {
                            if let Some(target) = scene.triggers.get_mut(index) {
                                *target = edited;
                            }
                        }
                        app.select_script(app.selected_scene, script_index);
                        app.mark_edited(format!("Created script '{}' for trigger", script_id));
                    }
                } else {
                    if changed {
                        app.capture_history();
                        if let Some(scene) = app.current_scene_mut() {
                            if let Some(target) = scene.triggers.get_mut(index) {
                                *target = edited;
                            }
                        }
                        app.mark_edited("Updated trigger");
                    }
                    if let Some(script_index) = open_script {
                        app.select_script(app.selected_scene, script_index);
                        app.status = format!(
                            "Opened script '{}'",
                            scene_snapshot.scripts[script_index].id
                        );
                    }
                }
            }
        }
    });
}

pub(super) fn dialogue_preview_text(dialogue: &DialogueGraph) -> String {
    dialogue_preview_lines(dialogue)
        .into_iter()
        .next()
        .unwrap_or_else(|| "Empty dialogue".to_string())
}

pub(super) fn script_preview_text(script: &EventScript) -> String {
    script
        .commands
        .first()
        .map(command_summary)
        .map(|label| {
            format!(
                "{} command(s), starts with {}",
                script.commands.len(),
                label
            )
        })
        .unwrap_or_else(|| "No commands".to_string())
}

#[derive(Clone)]
struct EventCatalog {
    dialogue_ids: Vec<String>,
    dialogue_nodes: BTreeMap<String, Vec<String>>,
    scene_ids: Vec<String>,
    scene_spawns: BTreeMap<String, Vec<String>>,
    checkpoint_ids: Vec<String>,
    visual_ids: Vec<String>,
}

impl EventCatalog {
    fn from_app(app: &EditorApp, bundle: &ProjectBundle, scene: &SceneResource) -> Self {
        let dialogue_ids = bundle
            .dialogues
            .iter()
            .map(|dialogue| dialogue.id.clone())
            .collect::<Vec<_>>();
        let dialogue_nodes = bundle
            .dialogues
            .iter()
            .map(|dialogue| {
                (
                    dialogue.id.clone(),
                    dialogue
                        .nodes
                        .iter()
                        .map(|node| node.id.clone())
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let scene_ids = bundle
            .scenes
            .iter()
            .map(|scene| scene.id.clone())
            .collect::<Vec<_>>();
        let scene_spawns = bundle
            .scenes
            .iter()
            .map(|scene| {
                (
                    scene.id.clone(),
                    scene
                        .spawns
                        .iter()
                        .map(|spawn| spawn.id.clone())
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<BTreeMap<_, _>>();

        Self {
            dialogue_ids,
            dialogue_nodes,
            scene_ids,
            scene_spawns,
            checkpoint_ids: scene
                .checkpoints
                .iter()
                .map(|checkpoint| checkpoint.id.clone())
                .collect::<Vec<_>>(),
            visual_ids: app.available_visual_ids(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum EventDiagnosticTarget {
    Dialogue {
        dialogue_id: String,
        node_id: Option<String>,
    },
    Script {
        scene_id: String,
        script_id: String,
    },
    Trigger {
        scene_id: String,
        trigger_id: String,
    },
}

#[derive(Default, Clone)]
struct DialogueDiagnostics {
    general: Vec<Diagnostic>,
    nodes: BTreeMap<String, Vec<Diagnostic>>,
}

impl DialogueDiagnostics {
    fn issue_count(&self) -> usize {
        self.general.len() + self.nodes.values().map(Vec::len).sum::<usize>()
    }

    fn is_empty(&self) -> bool {
        self.issue_count() == 0
    }
}

#[derive(Clone, Copy)]
struct DialogueGraphNodeLayout {
    node_index: usize,
    rect: Rect,
}

pub(super) fn navigate_to_event_diagnostic(app: &mut EditorApp, path: &str) -> bool {
    match parse_event_diagnostic_path(path) {
        Some(EventDiagnosticTarget::Dialogue {
            dialogue_id,
            node_id,
        }) => {
            let selection = app.bundle.as_ref().and_then(|bundle| {
                let dialogue_index = bundle
                    .dialogues
                    .iter()
                    .position(|dialogue| dialogue.id == dialogue_id)?;
                let node_index = node_id.as_deref().and_then(|node_id| {
                    bundle.dialogues[dialogue_index]
                        .nodes
                        .iter()
                        .position(|node| node.id == node_id)
                });
                Some((dialogue_index, node_index))
            });

            if let Some((dialogue_index, node_index)) = selection {
                app.select_dialogue(dialogue_index);
                if node_id.is_some() {
                    app.selected_dialogue_node = node_index;
                }
                app.workspace.layout.show_tab(DockTab::Events);
                app.status = node_id
                    .map(|node_id| {
                        format!(
                            "Opened dialogue '{}' node '{}' from diagnostics",
                            dialogue_id, node_id
                        )
                    })
                    .unwrap_or_else(|| {
                        format!("Opened dialogue '{}' from diagnostics", dialogue_id)
                    });
                return true;
            }
        }
        Some(EventDiagnosticTarget::Script {
            scene_id,
            script_id,
        }) => {
            let selection = app.bundle.as_ref().and_then(|bundle| {
                let scene_index = bundle
                    .scenes
                    .iter()
                    .position(|scene| scene.id == scene_id)?;
                let script_index = bundle.scenes[scene_index]
                    .scripts
                    .iter()
                    .position(|script| script.id == script_id)?;
                Some((scene_index, script_index))
            });

            if let Some((scene_index, script_index)) = selection {
                app.selected_scene = scene_index;
                app.sync_selection();
                app.select_script(scene_index, script_index);
                app.status = format!(
                    "Opened script '{}' in scene '{}' from diagnostics",
                    script_id, scene_id
                );
                return true;
            }
        }
        Some(EventDiagnosticTarget::Trigger {
            scene_id,
            trigger_id,
        }) => {
            let selection = app.bundle.as_ref().and_then(|bundle| {
                let scene_index = bundle
                    .scenes
                    .iter()
                    .position(|scene| scene.id == scene_id)?;
                let trigger_index = bundle.scenes[scene_index]
                    .triggers
                    .iter()
                    .position(|trigger| trigger.id == trigger_id)?;
                Some((scene_index, trigger_index))
            });

            if let Some((scene_index, trigger_index)) = selection {
                app.selected_scene = scene_index;
                app.sync_selection();
                app.selected_trigger = Some(trigger_index);
                app.tool = EditorTool::Trigger;
                app.preview_focus = PreviewFocus::None;
                app.workspace.layout.show_tab(DockTab::Inspector);
                app.status = format!(
                    "Opened trigger '{}' in scene '{}' from diagnostics",
                    trigger_id, scene_id
                );
                return true;
            }
        }
        None => {}
    }

    false
}

fn parse_event_diagnostic_path(path: &str) -> Option<EventDiagnosticTarget> {
    if let Some(rest) = path.strip_prefix("dialogue:") {
        let (dialogue_id, node_id) = rest
            .split_once(":node:")
            .map(|(dialogue_id, node_id)| (dialogue_id, Some(node_id.to_string())))
            .unwrap_or((rest, None));
        return Some(EventDiagnosticTarget::Dialogue {
            dialogue_id: dialogue_id.to_string(),
            node_id,
        });
    }

    if let Some(rest) = path.strip_prefix("scene:") {
        if let Some((scene_id, script_id)) = rest.split_once(":script:") {
            return Some(EventDiagnosticTarget::Script {
                scene_id: scene_id.to_string(),
                script_id: script_id.to_string(),
            });
        }
        if let Some((scene_id, trigger_id)) = rest.split_once(":trigger:") {
            return Some(EventDiagnosticTarget::Trigger {
                scene_id: scene_id.to_string(),
                trigger_id: trigger_id.to_string(),
            });
        }
    }

    None
}

fn iter_diagnostics(report: &ValidationReport) -> impl Iterator<Item = &Diagnostic> {
    report.errors.iter().chain(report.warnings.iter())
}

fn collect_dialogue_diagnostics(
    report: &ValidationReport,
    dialogue_id: &str,
) -> DialogueDiagnostics {
    let mut diagnostics = DialogueDiagnostics::default();

    for diagnostic in iter_diagnostics(report) {
        match diagnostic
            .path
            .as_deref()
            .and_then(parse_event_diagnostic_path)
        {
            Some(EventDiagnosticTarget::Dialogue {
                dialogue_id: target_dialogue,
                node_id: Some(node_id),
            }) if target_dialogue == dialogue_id => {
                diagnostics
                    .nodes
                    .entry(node_id)
                    .or_default()
                    .push(diagnostic.clone());
            }
            Some(EventDiagnosticTarget::Dialogue {
                dialogue_id: target_dialogue,
                node_id: None,
            }) if target_dialogue == dialogue_id => diagnostics.general.push(diagnostic.clone()),
            _ => {}
        }
    }

    diagnostics
}

fn collect_script_diagnostics(
    report: &ValidationReport,
    scene_id: &str,
    script_id: &str,
) -> Vec<Diagnostic> {
    iter_diagnostics(report)
        .filter(|diagnostic| {
            matches!(
                diagnostic
                    .path
                    .as_deref()
                    .and_then(parse_event_diagnostic_path),
                Some(EventDiagnosticTarget::Script {
                    scene_id: ref target_scene_id,
                    script_id: ref target_script_id,
                }) if target_scene_id == scene_id && target_script_id == script_id
            )
        })
        .cloned()
        .collect()
}

fn collect_trigger_diagnostics(
    report: &ValidationReport,
    scene_id: &str,
    trigger_id: &str,
) -> Vec<Diagnostic> {
    iter_diagnostics(report)
        .filter(|diagnostic| {
            matches!(
                diagnostic
                    .path
                    .as_deref()
                    .and_then(parse_event_diagnostic_path),
                Some(EventDiagnosticTarget::Trigger {
                    scene_id: ref target_scene_id,
                    trigger_id: ref target_trigger_id,
                }) if target_scene_id == scene_id && target_trigger_id == trigger_id
            )
        })
        .cloned()
        .collect()
}

fn count_dialogue_diagnostics(report: &ValidationReport, dialogue_id: &str) -> usize {
    collect_dialogue_diagnostics(report, dialogue_id).issue_count()
}

fn count_script_diagnostics(report: &ValidationReport, scene_id: &str, script_id: &str) -> usize {
    collect_script_diagnostics(report, scene_id, script_id).len()
}

fn count_trigger_diagnostics(report: &ValidationReport, scene_id: &str, trigger_id: &str) -> usize {
    collect_trigger_diagnostics(report, scene_id, trigger_id).len()
}

fn diagnostic_severity_color(severity: Severity) -> Color32 {
    match severity {
        Severity::Error => Color32::from_rgb(232, 96, 96),
        Severity::Warning => Color32::from_rgb(236, 190, 88),
    }
}

fn draw_inline_diagnostic_row(
    ui: &mut egui::Ui,
    diagnostic: &Diagnostic,
    action_label: Option<&str>,
) -> bool {
    let mut action_clicked = false;
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(
            diagnostic_severity_color(diagnostic.severity),
            format!("[{}]", diagnostic.code),
        );
        ui.label(&diagnostic.message);
        if let Some(action_label) = action_label {
            if ui.small_button(action_label).clicked() {
                action_clicked = true;
            }
        }
    });
    action_clicked
}

fn draw_dialogue_editor(
    app: &mut EditorApp,
    ui: &mut egui::Ui,
    dialogue_index: usize,
    bundle_snapshot: &ProjectBundle,
    catalog: &EventCatalog,
) {
    let Some(dialogue_snapshot) = bundle_snapshot.dialogues.get(dialogue_index).cloned() else {
        ui.label("Selected dialogue is missing.");
        return;
    };

    let mut edited = dialogue_snapshot.clone();
    let dialogue_diagnostics = collect_dialogue_diagnostics(&app.report, &dialogue_snapshot.id);
    let mut changed = false;
    let mut node_action = None;
    let mut renamed_node = None;

    ui.heading("Dialogue");
    changed |= ui.text_edit_singleline(&mut edited.id).changed();
    let node_ids = edited
        .nodes
        .iter()
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();
    if edited.opening_node.is_empty() && !node_ids.is_empty() {
        edited.opening_node = node_ids[0].clone();
        changed = true;
    }
    egui::ComboBox::from_label("Opening Node")
        .selected_text(if edited.opening_node.is_empty() {
            "None"
        } else {
            edited.opening_node.as_str()
        })
        .show_ui(ui, |ui| {
            for node_id in &node_ids {
                changed |= ui
                    .selectable_value(&mut edited.opening_node, node_id.clone(), node_id)
                    .changed();
            }
        });

    ui.horizontal_wrapped(|ui| {
        if ui.button("+ Node").clicked() {
            node_action = Some(DialogueNodeAction::Add);
        }
        if ui
            .add_enabled(
                app.selected_dialogue_node.is_some(),
                egui::Button::new("Duplicate Node"),
            )
            .clicked()
        {
            node_action = app
                .selected_dialogue_node
                .map(DialogueNodeAction::Duplicate);
        }
        if ui
            .add_enabled(
                edited.nodes.len() > 1 && app.selected_dialogue_node.is_some(),
                egui::Button::new("Remove Node"),
            )
            .clicked()
        {
            node_action = app.selected_dialogue_node.map(DialogueNodeAction::Remove);
        }
    });

    ui.separator();
    if !dialogue_diagnostics.is_empty() {
        ui.group(|ui| {
            ui.label(format!(
                "Validation ({} issue(s))",
                dialogue_diagnostics.issue_count()
            ));
            for diagnostic in &dialogue_diagnostics.general {
                draw_inline_diagnostic_row(ui, diagnostic, None);
            }
            let mut node_ids = dialogue_diagnostics
                .nodes
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            node_ids.sort();
            for node_id in node_ids {
                for diagnostic in dialogue_diagnostics
                    .nodes
                    .get(&node_id)
                    .into_iter()
                    .flatten()
                {
                    if draw_inline_diagnostic_row(ui, diagnostic, Some("Focus Node")) {
                        app.selected_dialogue_node =
                            edited.nodes.iter().position(|node| node.id == node_id);
                    }
                }
            }
        });
        ui.separator();
    }
    ui.label("Graph");
    ui.small("Click a node in the canvas to select it.");
    draw_dialogue_graph_canvas(app, ui, &edited, &dialogue_diagnostics.nodes);
    ui.separator();
    ui.columns(2, |columns| {
        columns[0].label("Nodes");
        egui::ScrollArea::vertical()
            .max_height(260.0)
            .show(&mut columns[0], |ui| {
                for (index, node) in edited.nodes.iter().enumerate() {
                    if ui
                        .selectable_label(
                            app.selected_dialogue_node == Some(index),
                            format!("{}  |  {}", node.id, truncate_inline(&node.text, 26)),
                        )
                        .clicked()
                    {
                        app.selected_dialogue_node = Some(index);
                    }
                }
            });

        columns[1].vertical(|ui| {
            if let Some(node_index) = app.selected_dialogue_node {
                if let Some(node) = edited.nodes.get_mut(node_index) {
                    ui.label(format!("Editing node {}", node_index + 1));
                    if let Some(diagnostics) = dialogue_diagnostics.nodes.get(&node.id) {
                        ui.separator();
                        ui.label(format!("Node validation ({} issue(s))", diagnostics.len()));
                        for diagnostic in diagnostics {
                            draw_inline_diagnostic_row(ui, diagnostic, None);
                        }
                        ui.separator();
                    }
                    let old_node_id = node.id.clone();
                    changed |= ui.text_edit_singleline(&mut node.id).changed();
                    if node.id.trim().is_empty() {
                        node.id = old_node_id.clone();
                    }
                    if node.id != old_node_id {
                        renamed_node = Some((old_node_id, node.id.clone()));
                    }
                    changed |= ui.text_edit_singleline(&mut node.speaker).changed();
                    changed |= ui
                        .add(
                            egui::TextEdit::multiline(&mut node.text)
                                .desired_rows(4)
                                .hint_text("Dialogue text"),
                        )
                        .changed();

                    let mut has_next = node.next.is_some();
                    if ui
                        .checkbox(&mut has_next, "Use next-node routing")
                        .changed()
                    {
                        if has_next {
                            node.next = node_ids
                                .iter()
                                .find(|candidate| **candidate != node.id)
                                .cloned()
                                .or_else(|| node_ids.first().cloned());
                        } else {
                            node.next = None;
                        }
                        changed = true;
                    }
                    if let Some(next) = &mut node.next {
                        egui::ComboBox::from_label("Next Node")
                            .selected_text(next.as_str())
                            .show_ui(ui, |ui| {
                                for node_id in &node_ids {
                                    changed |= ui
                                        .selectable_value(next, node_id.clone(), node_id)
                                        .changed();
                                }
                            });
                    }

                    ui.separator();
                    ui.label("Choices");
                    let mut remove_choice = None;
                    for (choice_index, choice) in node.choices.iter_mut().enumerate() {
                        ui.group(|ui| {
                            changed |= ui.text_edit_singleline(&mut choice.text).changed();
                            egui::ComboBox::from_id_salt((
                                "dialogue_choice_next",
                                dialogue_index,
                                node_index,
                                choice_index,
                            ))
                            .selected_text(choice.next.as_str())
                            .show_ui(ui, |ui| {
                                for node_id in &node_ids {
                                    changed |= ui
                                        .selectable_value(
                                            &mut choice.next,
                                            node_id.clone(),
                                            node_id,
                                        )
                                        .changed();
                                }
                            });
                            let mut has_condition = choice.condition_flag.is_some();
                            if ui.checkbox(&mut has_condition, "Condition Flag").changed() {
                                if has_condition {
                                    choice.condition_flag = Some("flag_name".to_string());
                                } else {
                                    choice.condition_flag = None;
                                }
                                changed = true;
                            }
                            if let Some(condition_flag) = &mut choice.condition_flag {
                                changed |= ui.text_edit_singleline(condition_flag).changed();
                            }
                            if ui.small_button("Remove Choice").clicked() {
                                remove_choice = Some(choice_index);
                            }
                        });
                    }
                    if let Some(choice_index) = remove_choice {
                        if choice_index < node.choices.len() {
                            node.choices.remove(choice_index);
                            changed = true;
                        }
                    }
                    if ui.button("+ Choice").clicked() {
                        node.choices.push(DialogueChoice {
                            text: "New choice".to_string(),
                            next: node_ids
                                .first()
                                .cloned()
                                .unwrap_or_else(|| "start".to_string()),
                            condition_flag: None,
                        });
                        changed = true;
                    }

                    ui.separator();
                    ui.label("Commands");
                    changed |= draw_event_commands(
                        ui,
                        &mut node.commands,
                        catalog,
                        ("dialogue_node_commands", dialogue_index, node_index),
                    );
                }
            } else {
                ui.label("Select a node to edit it.");
            }
        });
    });

    if let Some((old_node_id, new_node_id)) = renamed_node.as_ref() {
        rename_dialogue_node_refs(&mut edited, old_node_id, new_node_id);
    }

    if let Some(action) = node_action {
        apply_dialogue_node_action(&mut edited, app, action);
        changed = true;
    }

    normalize_dialogue(&mut edited, &dialogue_snapshot.id);
    if changed {
        let old_dialogue_id = dialogue_snapshot.id.clone();
        app.capture_history();
        if let Some(bundle) = &mut app.bundle {
            if let Some(target) = bundle.dialogues.get_mut(dialogue_index) {
                *target = edited.clone();
            }
            if old_dialogue_id != edited.id {
                rename_dialogue_references(bundle, &old_dialogue_id, &edited.id);
            }
            if let Some((old_node_id, new_node_id)) = renamed_node {
                rename_script_dialogue_node_references(
                    bundle,
                    &edited.id,
                    &old_node_id,
                    &new_node_id,
                );
            }
        }
        app.mark_edited(format!("Updated dialogue '{}'", edited.id));
    }

    ui.separator();
    ui.collapsing("Preview", |ui| {
        for line in dialogue_preview_lines(&edited).into_iter().take(8) {
            ui.label(line);
        }
    });
}

fn draw_dialogue_graph_canvas(
    app: &mut EditorApp,
    ui: &mut egui::Ui,
    dialogue: &DialogueGraph,
    node_diagnostics: &BTreeMap<String, Vec<Diagnostic>>,
) {
    let columns = dialogue_graph_columns(dialogue);
    let node_size = Vec2::new(180.0, 100.0);
    let column_gap = 56.0;
    let row_gap = 28.0;
    let max_rows = columns.iter().map(Vec::len).max().unwrap_or(1).max(1);
    let graph_size = Vec2::new(
        columns.len().max(1) as f32 * node_size.x
            + columns.len().saturating_sub(1) as f32 * column_gap
            + 32.0,
        max_rows as f32 * node_size.y + max_rows.saturating_sub(1) as f32 * row_gap + 32.0,
    );

    egui::ScrollArea::both().max_height(280.0).show(ui, |ui| {
        let (response, painter) = ui.allocate_painter(graph_size, Sense::click());
        painter.rect_filled(response.rect, 8.0, Color32::from_rgb(18, 26, 34));

        let mut layouts = Vec::with_capacity(dialogue.nodes.len());
        for (column_index, column) in columns.iter().enumerate() {
            for (row_index, node_index) in column.iter().enumerate() {
                let rect = Rect::from_min_size(
                    response.rect.min
                        + Vec2::new(
                            16.0 + column_index as f32 * (node_size.x + column_gap),
                            16.0 + row_index as f32 * (node_size.y + row_gap),
                        ),
                    node_size,
                );
                layouts.push(DialogueGraphNodeLayout {
                    node_index: *node_index,
                    rect,
                });
            }
        }
        layouts.sort_by_key(|entry| entry.node_index);

        let node_lookup = dialogue
            .nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.id.as_str(), index))
            .collect::<BTreeMap<_, _>>();

        for (node_index, node) in dialogue.nodes.iter().enumerate() {
            let Some(layout) = layouts.get(node_index) else {
                continue;
            };
            if let Some(next) = &node.next {
                if let Some(&target_index) = node_lookup.get(next.as_str()) {
                    if let Some(target_layout) = layouts.get(target_index) {
                        draw_dialogue_link(
                            &painter,
                            layout.rect.right_center(),
                            target_layout.rect.left_center(),
                            Color32::from_rgb(96, 208, 255),
                            Some("next"),
                        );
                    }
                }
            }
            for choice in &node.choices {
                if let Some(&target_index) = node_lookup.get(choice.next.as_str()) {
                    if let Some(target_layout) = layouts.get(target_index) {
                        draw_dialogue_link(
                            &painter,
                            layout.rect.right_center() + Vec2::new(0.0, 16.0),
                            target_layout.rect.left_center() + Vec2::new(0.0, 16.0),
                            Color32::from_rgb(214, 132, 84),
                            Some(choice.text.as_str()),
                        );
                    }
                }
            }
        }

        for entry in &layouts {
            let node = &dialogue.nodes[entry.node_index];
            let diagnostics = node_diagnostics.get(&node.id);
            let is_selected = app.selected_dialogue_node == Some(entry.node_index);
            let is_opening = dialogue.opening_node == node.id;
            let fill = if is_selected {
                Color32::from_rgb(72, 92, 52)
            } else if is_opening {
                Color32::from_rgb(44, 70, 92)
            } else {
                Color32::from_rgb(34, 44, 58)
            };
            let stroke = if is_selected {
                Color32::from_rgb(244, 214, 92)
            } else if is_opening {
                Color32::from_rgb(96, 208, 255)
            } else {
                Color32::from_white_alpha(72)
            };
            if let Some(diagnostics) = diagnostics {
                if let Some(severity) = diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.severity)
                    .max_by_key(|severity| match severity {
                        Severity::Warning => 0,
                        Severity::Error => 1,
                    })
                {
                    painter.rect_stroke(
                        entry.rect.expand(2.0),
                        10.0,
                        (1.0, diagnostic_severity_color(severity)),
                        StrokeKind::Outside,
                    );
                }
            }
            painter.rect_filled(entry.rect, 8.0, fill);
            painter.rect_stroke(entry.rect, 8.0, (2.0, stroke), StrokeKind::Inside);
            painter.text(
                entry.rect.min + Vec2::new(10.0, 8.0),
                Align2::LEFT_TOP,
                &node.id,
                FontId::proportional(13.0),
                Color32::WHITE,
            );
            painter.text(
                entry.rect.min + Vec2::new(10.0, 28.0),
                Align2::LEFT_TOP,
                if node.speaker.trim().is_empty() {
                    "Narrator"
                } else {
                    node.speaker.as_str()
                },
                FontId::proportional(11.0),
                Color32::from_rgb(196, 208, 220),
            );
            painter.text(
                entry.rect.min + Vec2::new(10.0, 46.0),
                Align2::LEFT_TOP,
                truncate_inline(&node.text, 56),
                FontId::proportional(11.0),
                Color32::from_gray(192),
            );
            painter.text(
                entry.rect.max - Vec2::new(10.0, 8.0),
                Align2::RIGHT_BOTTOM,
                format!(
                    "{} choice(s)  |  {} cmd(s)",
                    node.choices.len(),
                    node.commands.len()
                ),
                FontId::proportional(10.0),
                Color32::from_gray(164),
            );
            if let Some(diagnostics) = diagnostics {
                let severity = diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.severity)
                    .max_by_key(|severity| match severity {
                        Severity::Warning => 0,
                        Severity::Error => 1,
                    })
                    .unwrap_or(Severity::Warning);
                let badge_rect = Rect::from_min_size(
                    Pos2::new(entry.rect.max.x - 36.0, entry.rect.min.y + 8.0),
                    Vec2::new(28.0, 16.0),
                );
                painter.rect_filled(badge_rect, 6.0, diagnostic_severity_color(severity));
                painter.text(
                    badge_rect.center(),
                    Align2::CENTER_CENTER,
                    format!(
                        "{}{}",
                        match severity {
                            Severity::Error => "E",
                            Severity::Warning => "W",
                        },
                        diagnostics.len()
                    ),
                    FontId::proportional(9.0),
                    Color32::from_rgb(18, 24, 30),
                );
            }

            let node_response = ui.interact(
                entry.rect,
                ui.make_persistent_id((
                    "dialogue_node_canvas",
                    dialogue.id.as_str(),
                    entry.node_index,
                )),
                Sense::click(),
            );
            if node_response.clicked() {
                app.selected_dialogue_node = Some(entry.node_index);
            }
            if let Some(diagnostics) = diagnostics {
                if node_response.hovered() {
                    let hover_text = diagnostics
                        .iter()
                        .map(|diagnostic| format!("[{}] {}", diagnostic.code, diagnostic.message))
                        .collect::<Vec<_>>()
                        .join("\n");
                    node_response.clone().on_hover_text(hover_text);
                }
            }
        }
    });
}

fn draw_script_editor(
    app: &mut EditorApp,
    ui: &mut egui::Ui,
    script_index: usize,
    scene_snapshot: &SceneResource,
    catalog: &EventCatalog,
) {
    let Some(script_snapshot) = scene_snapshot.scripts.get(script_index).cloned() else {
        ui.label("Selected script is missing.");
        return;
    };

    let mut edited = script_snapshot.clone();
    let mut changed = false;
    changed |= ui.text_edit_singleline(&mut edited.id).changed();
    if edited.id.trim().is_empty() {
        edited.id = script_snapshot.id.clone();
    }

    let bound_triggers = scene_snapshot
        .triggers
        .iter()
        .filter(|trigger| trigger.script_id == script_snapshot.id)
        .map(|trigger| trigger.id.clone())
        .collect::<Vec<_>>();

    ui.heading("Script");
    ui.label(format!("Scene: {}", scene_snapshot.id));
    if !bound_triggers.is_empty() {
        ui.label(format!("Bound triggers: {}", bound_triggers.join(", ")));
    }
    let script_diagnostics =
        collect_script_diagnostics(&app.report, &scene_snapshot.id, &script_snapshot.id);
    if !script_diagnostics.is_empty() {
        ui.separator();
        ui.label(format!(
            "Validation ({} issue(s))",
            script_diagnostics.len()
        ));
        for diagnostic in &script_diagnostics {
            draw_inline_diagnostic_row(ui, diagnostic, None);
        }
    }
    ui.separator();
    changed |= draw_event_commands(
        ui,
        &mut edited.commands,
        catalog,
        ("scene_script", script_index, 0),
    );

    if changed {
        let old_script_id = script_snapshot.id.clone();
        app.capture_history();
        if let Some(scene) = app.current_scene_mut() {
            if let Some(target) = scene.scripts.get_mut(script_index) {
                *target = edited.clone();
            }
            if old_script_id != edited.id {
                rename_trigger_script_references(scene, &old_script_id, &edited.id);
            }
        }
        app.mark_edited(format!("Updated script '{}'", edited.id));
    }

    ui.separator();
    ui.collapsing("Preview", |ui| {
        if edited.commands.is_empty() {
            ui.label("No commands yet.");
        } else {
            for (index, command) in edited.commands.iter().enumerate() {
                ui.label(format!("{}. {}", index + 1, command_summary(command)));
            }
        }
    });
}

fn dialogue_graph_columns(dialogue: &DialogueGraph) -> Vec<Vec<usize>> {
    if dialogue.nodes.is_empty() {
        return Vec::new();
    }

    let node_lookup = dialogue
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    let mut columns = vec![usize::MAX; dialogue.nodes.len()];
    let mut queue = VecDeque::new();
    let start_index = node_lookup
        .get(dialogue.opening_node.as_str())
        .copied()
        .unwrap_or(0);

    columns[start_index] = 0;
    queue.push_back(start_index);

    while let Some(node_index) = queue.pop_front() {
        let next_column = columns[node_index].saturating_add(1);
        for target_index in dialogue_link_targets(dialogue, &node_lookup, node_index) {
            if columns[target_index] == usize::MAX || next_column < columns[target_index] {
                columns[target_index] = next_column;
                queue.push_back(target_index);
            }
        }
    }

    let mut next_root_column = columns
        .iter()
        .copied()
        .filter(|column| *column != usize::MAX)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    for index in 0..dialogue.nodes.len() {
        if columns[index] != usize::MAX {
            continue;
        }
        columns[index] = next_root_column;
        queue.push_back(index);
        while let Some(node_index) = queue.pop_front() {
            let next_column = columns[node_index].saturating_add(1);
            for target_index in dialogue_link_targets(dialogue, &node_lookup, node_index) {
                if columns[target_index] == usize::MAX {
                    columns[target_index] = next_column;
                    queue.push_back(target_index);
                }
            }
        }
        next_root_column = columns
            .iter()
            .copied()
            .filter(|column| *column != usize::MAX)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
    }

    let mut grouped = BTreeMap::<usize, Vec<usize>>::new();
    for (node_index, column_index) in columns.into_iter().enumerate() {
        grouped.entry(column_index).or_default().push(node_index);
    }
    grouped.into_values().collect()
}

fn dialogue_link_targets(
    dialogue: &DialogueGraph,
    node_lookup: &BTreeMap<&str, usize>,
    node_index: usize,
) -> Vec<usize> {
    let mut targets = Vec::new();
    let Some(node) = dialogue.nodes.get(node_index) else {
        return targets;
    };

    if let Some(next) = &node.next {
        if let Some(&target_index) = node_lookup.get(next.as_str()) {
            targets.push(target_index);
        }
    }
    for choice in &node.choices {
        if let Some(&target_index) = node_lookup.get(choice.next.as_str()) {
            if !targets.contains(&target_index) {
                targets.push(target_index);
            }
        }
    }

    targets
}

fn draw_dialogue_link(
    painter: &egui::Painter,
    from: Pos2,
    to: Pos2,
    color: Color32,
    label: Option<&str>,
) {
    let mid_x = (from.x + to.x) * 0.5;
    let points = [from, Pos2::new(mid_x, from.y), Pos2::new(mid_x, to.y), to];
    for segment in points.windows(2) {
        painter.line_segment([segment[0], segment[1]], (2.0, color));
    }

    let arrow_base = points[2];
    let arrow_tip = to;
    let direction = (arrow_tip - arrow_base).normalized();
    let back = -direction * 8.0;
    let side = Vec2::new(-direction.y, direction.x) * 4.0;
    painter.line_segment([arrow_tip, arrow_tip + back + side], (2.0, color));
    painter.line_segment([arrow_tip, arrow_tip + back - side], (2.0, color));

    if let Some(label) = label {
        painter.text(
            Pos2::new(mid_x, (from.y + to.y) * 0.5 - 6.0),
            Align2::CENTER_BOTTOM,
            truncate_inline(label, 18),
            FontId::proportional(9.0),
            color,
        );
    }
}

#[derive(Clone, Copy)]
enum DialogueNodeAction {
    Add,
    Duplicate(usize),
    Remove(usize),
}

fn apply_dialogue_node_action(
    dialogue: &mut DialogueGraph,
    app: &mut EditorApp,
    action: DialogueNodeAction,
) {
    match action {
        DialogueNodeAction::Add => {
            let existing = dialogue
                .nodes
                .iter()
                .map(|node| node.id.clone())
                .collect::<BTreeSet<_>>();
            let id = next_numbered_id(&existing, "node");
            dialogue.nodes.push(DialogueNode {
                id,
                speaker: String::new(),
                text: String::new(),
                commands: Vec::new(),
                choices: Vec::new(),
                next: None,
            });
            app.selected_dialogue_node = Some(dialogue.nodes.len().saturating_sub(1));
        }
        DialogueNodeAction::Duplicate(index) if index < dialogue.nodes.len() => {
            let existing = dialogue
                .nodes
                .iter()
                .map(|node| node.id.clone())
                .collect::<BTreeSet<_>>();
            let mut duplicate = dialogue.nodes[index].clone();
            duplicate.id = next_numbered_id(&existing, &duplicate.id);
            let insert_at = (index + 1).min(dialogue.nodes.len());
            dialogue.nodes.insert(insert_at, duplicate);
            app.selected_dialogue_node = Some(insert_at);
        }
        DialogueNodeAction::Remove(index)
            if index < dialogue.nodes.len() && dialogue.nodes.len() > 1 =>
        {
            let removed_id = dialogue.nodes[index].id.clone();
            dialogue.nodes.remove(index);
            let fallback = dialogue.nodes.first().map(|node| node.id.clone());
            remove_dialogue_node_refs(dialogue, &removed_id, fallback.as_deref());
            app.selected_dialogue_node = sanitize_optional_index(Some(index), dialogue.nodes.len());
        }
        _ => {}
    }
}

fn draw_event_commands(
    ui: &mut egui::Ui,
    commands: &mut Vec<EventCommand>,
    catalog: &EventCatalog,
    salt: (&'static str, usize, usize),
) -> bool {
    let mut changed = false;
    let mut action = None;
    let command_count = commands.len();

    for (index, command) in commands.iter_mut().enumerate() {
        ui.group(|ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("Command {}", index + 1));
                egui::ComboBox::from_id_salt((salt.0, salt.1, salt.2, index, "kind"))
                    .selected_text(event_command_label(command))
                    .show_ui(ui, |ui| {
                        for kind in COMMAND_KINDS {
                            if ui
                                .selectable_label(event_command_label(command) == kind, kind)
                                .clicked()
                            {
                                *command = default_event_command(kind, catalog);
                                changed = true;
                                ui.close();
                            }
                        }
                    });
                if ui.small_button("Up").clicked() && index > 0 {
                    action = Some(CommandAction::MoveUp(index));
                }
                if ui.small_button("Down").clicked() && index + 1 < command_count {
                    action = Some(CommandAction::MoveDown(index));
                }
                if ui.small_button("Remove").clicked() {
                    action = Some(CommandAction::Remove(index));
                }
            });
            changed |= draw_event_command_fields(ui, command, catalog, salt, index);
        });
    }

    if let Some(action) = action {
        match action {
            CommandAction::MoveUp(index) if index > 0 && index < commands.len() => {
                commands.swap(index, index - 1);
                changed = true;
            }
            CommandAction::MoveDown(index) if index + 1 < commands.len() => {
                commands.swap(index, index + 1);
                changed = true;
            }
            CommandAction::Remove(index) if index < commands.len() => {
                commands.remove(index);
                changed = true;
            }
            _ => {}
        }
    }

    if ui.button("+ Command").clicked() {
        commands.push(default_event_command("Wait", catalog));
        changed = true;
    }

    changed
}

#[derive(Clone, Copy)]
enum CommandAction {
    MoveUp(usize),
    MoveDown(usize),
    Remove(usize),
}

const COMMAND_KINDS: [&str; 10] = [
    "ShowDialogue",
    "SetFlag",
    "Wait",
    "MoveCamera",
    "FreezePlayer",
    "SpawnEntity",
    "LoadScene",
    "StartBattleScene",
    "PlayCutscene",
    "EmitCheckpoint",
];

fn draw_event_command_fields(
    ui: &mut egui::Ui,
    command: &mut EventCommand,
    catalog: &EventCatalog,
    salt: (&'static str, usize, usize),
    index: usize,
) -> bool {
    let mut changed = false;
    match command {
        EventCommand::ShowDialogue {
            dialogue_id,
            node_id,
        } => {
            egui::ComboBox::from_id_salt((salt.0, salt.1, salt.2, index, "dialogue"))
                .selected_text(if dialogue_id.is_empty() {
                    "Choose dialogue"
                } else {
                    dialogue_id.as_str()
                })
                .show_ui(ui, |ui| {
                    for candidate in &catalog.dialogue_ids {
                        changed |= ui
                            .selectable_value(dialogue_id, candidate.clone(), candidate)
                            .changed();
                    }
                });
            let mut use_node_override = node_id.is_some();
            if ui
                .checkbox(&mut use_node_override, "Start at a specific node")
                .changed()
            {
                if use_node_override {
                    *node_id = catalog
                        .dialogue_nodes
                        .get(dialogue_id)
                        .and_then(|node_ids| node_ids.first().cloned())
                        .or_else(|| Some("start".to_string()));
                } else {
                    *node_id = None;
                }
                changed = true;
            }
            if let Some(node_id) = node_id {
                let node_options = catalog
                    .dialogue_nodes
                    .get(dialogue_id)
                    .cloned()
                    .unwrap_or_default();
                if node_options.is_empty() {
                    changed |= ui.text_edit_singleline(node_id).changed();
                } else {
                    egui::ComboBox::from_id_salt((salt.0, salt.1, salt.2, index, "node_id"))
                        .selected_text(node_id.as_str())
                        .show_ui(ui, |ui| {
                            for candidate in &node_options {
                                changed |= ui
                                    .selectable_value(node_id, candidate.clone(), candidate)
                                    .changed();
                            }
                        });
                }
            }
        }
        EventCommand::SetFlag { flag, value } => {
            changed |= ui.text_edit_singleline(flag).changed();
            changed |= ui.checkbox(value, "Value").changed();
        }
        EventCommand::Wait { frames } => {
            changed |= ui
                .add(egui::DragValue::new(frames).range(1..=u16::MAX))
                .changed();
        }
        EventCommand::MoveCamera {
            target_x,
            target_y,
            frames,
        } => {
            ui.horizontal(|ui| {
                ui.label("X");
                changed |= ui.add(egui::DragValue::new(target_x).speed(1)).changed();
                ui.label("Y");
                changed |= ui.add(egui::DragValue::new(target_y).speed(1)).changed();
                ui.label("Frames");
                changed |= ui
                    .add(egui::DragValue::new(frames).range(1..=u16::MAX))
                    .changed();
            });
        }
        EventCommand::FreezePlayer { frozen } => {
            changed |= ui.checkbox(frozen, "Freeze player").changed();
        }
        EventCommand::SpawnEntity { archetype, x, y } => {
            egui::ComboBox::from_id_salt((salt.0, salt.1, salt.2, index, "spawn_entity"))
                .selected_text(if archetype.is_empty() {
                    "Choose visual"
                } else {
                    archetype.as_str()
                })
                .show_ui(ui, |ui| {
                    for candidate in &catalog.visual_ids {
                        changed |= ui
                            .selectable_value(archetype, candidate.clone(), candidate)
                            .changed();
                    }
                });
            ui.horizontal(|ui| {
                ui.label("X");
                changed |= ui.add(egui::DragValue::new(x).speed(1)).changed();
                ui.label("Y");
                changed |= ui.add(egui::DragValue::new(y).speed(1)).changed();
            });
        }
        EventCommand::LoadScene { scene_id, spawn } => {
            egui::ComboBox::from_id_salt((salt.0, salt.1, salt.2, index, "scene_id"))
                .selected_text(if scene_id.is_empty() {
                    "Choose scene"
                } else {
                    scene_id.as_str()
                })
                .show_ui(ui, |ui| {
                    for candidate in &catalog.scene_ids {
                        changed |= ui
                            .selectable_value(scene_id, candidate.clone(), candidate)
                            .changed();
                    }
                });
            let mut use_spawn = spawn.is_some();
            if ui.checkbox(&mut use_spawn, "Use spawn override").changed() {
                if use_spawn {
                    *spawn = catalog
                        .scene_spawns
                        .get(scene_id)
                        .and_then(|spawns| spawns.first().cloned())
                        .or_else(|| Some("spawn".to_string()));
                } else {
                    *spawn = None;
                }
                changed = true;
            }
            if let Some(spawn) = spawn {
                let spawn_options = catalog
                    .scene_spawns
                    .get(scene_id)
                    .cloned()
                    .unwrap_or_default();
                if spawn_options.is_empty() {
                    changed |= ui.text_edit_singleline(spawn).changed();
                } else {
                    egui::ComboBox::from_id_salt((salt.0, salt.1, salt.2, index, "spawn_id"))
                        .selected_text(spawn.as_str())
                        .show_ui(ui, |ui| {
                            for candidate in &spawn_options {
                                changed |= ui
                                    .selectable_value(spawn, candidate.clone(), candidate)
                                    .changed();
                            }
                        });
                }
            }
        }
        EventCommand::StartBattleScene { battle_id } => {
            changed |= ui.text_edit_singleline(battle_id).changed();
        }
        EventCommand::PlayCutscene { cutscene_id } => {
            changed |= ui.text_edit_singleline(cutscene_id).changed();
        }
        EventCommand::EmitCheckpoint { checkpoint_id } => {
            if catalog.checkpoint_ids.is_empty() {
                changed |= ui.text_edit_singleline(checkpoint_id).changed();
            } else {
                egui::ComboBox::from_id_salt((salt.0, salt.1, salt.2, index, "checkpoint_id"))
                    .selected_text(if checkpoint_id.is_empty() {
                        "Choose checkpoint"
                    } else {
                        checkpoint_id.as_str()
                    })
                    .show_ui(ui, |ui| {
                        for candidate in &catalog.checkpoint_ids {
                            changed |= ui
                                .selectable_value(checkpoint_id, candidate.clone(), candidate)
                                .changed();
                        }
                    });
            }
        }
    }
    changed
}

fn default_event_command(kind: &str, catalog: &EventCatalog) -> EventCommand {
    match kind {
        "ShowDialogue" => EventCommand::ShowDialogue {
            dialogue_id: catalog.dialogue_ids.first().cloned().unwrap_or_default(),
            node_id: None,
        },
        "SetFlag" => EventCommand::SetFlag {
            flag: "flag_name".to_string(),
            value: true,
        },
        "Wait" => EventCommand::Wait { frames: 1 },
        "MoveCamera" => EventCommand::MoveCamera {
            target_x: 0,
            target_y: 0,
            frames: 30,
        },
        "FreezePlayer" => EventCommand::FreezePlayer { frozen: true },
        "SpawnEntity" => EventCommand::SpawnEntity {
            archetype: catalog.visual_ids.first().cloned().unwrap_or_default(),
            x: 0,
            y: 0,
        },
        "LoadScene" => EventCommand::LoadScene {
            scene_id: catalog.scene_ids.first().cloned().unwrap_or_default(),
            spawn: None,
        },
        "StartBattleScene" => EventCommand::StartBattleScene {
            battle_id: "battle_1".to_string(),
        },
        "PlayCutscene" => EventCommand::PlayCutscene {
            cutscene_id: "cutscene_1".to_string(),
        },
        "EmitCheckpoint" => EventCommand::EmitCheckpoint {
            checkpoint_id: catalog.checkpoint_ids.first().cloned().unwrap_or_default(),
        },
        _ => EventCommand::Wait { frames: 1 },
    }
}

fn command_summary(command: &EventCommand) -> String {
    match command {
        EventCommand::ShowDialogue {
            dialogue_id,
            node_id,
        } => match node_id {
            Some(node_id) if !node_id.is_empty() => {
                format!("ShowDialogue {} @ {}", dialogue_id, node_id)
            }
            _ => format!("ShowDialogue {}", dialogue_id),
        },
        EventCommand::SetFlag { flag, value } => format!("SetFlag {}={}", flag, value),
        EventCommand::Wait { frames } => format!("Wait {} frame(s)", frames),
        EventCommand::MoveCamera {
            target_x,
            target_y,
            frames,
        } => format!("MoveCamera ({}, {}) over {}", target_x, target_y, frames),
        EventCommand::FreezePlayer { frozen } => format!("FreezePlayer {}", frozen),
        EventCommand::SpawnEntity { archetype, x, y } => {
            format!("SpawnEntity {} at ({}, {})", archetype, x, y)
        }
        EventCommand::LoadScene { scene_id, spawn } => match spawn {
            Some(spawn) if !spawn.is_empty() => format!("LoadScene {} @ {}", scene_id, spawn),
            _ => format!("LoadScene {}", scene_id),
        },
        EventCommand::StartBattleScene { battle_id } => format!("StartBattleScene {}", battle_id),
        EventCommand::PlayCutscene { cutscene_id } => format!("PlayCutscene {}", cutscene_id),
        EventCommand::EmitCheckpoint { checkpoint_id } => {
            format!("EmitCheckpoint {}", checkpoint_id)
        }
    }
}

fn event_command_label(command: &EventCommand) -> &'static str {
    match command {
        EventCommand::ShowDialogue { .. } => "ShowDialogue",
        EventCommand::SetFlag { .. } => "SetFlag",
        EventCommand::Wait { .. } => "Wait",
        EventCommand::MoveCamera { .. } => "MoveCamera",
        EventCommand::FreezePlayer { .. } => "FreezePlayer",
        EventCommand::SpawnEntity { .. } => "SpawnEntity",
        EventCommand::LoadScene { .. } => "LoadScene",
        EventCommand::StartBattleScene { .. } => "StartBattleScene",
        EventCommand::PlayCutscene { .. } => "PlayCutscene",
        EventCommand::EmitCheckpoint { .. } => "EmitCheckpoint",
    }
}

fn create_dialogue(app: &mut EditorApp) -> (usize, String) {
    let existing = app
        .bundle
        .as_ref()
        .map(|bundle| {
            bundle
                .dialogues
                .iter()
                .map(|dialogue| dialogue.id.clone())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let dialogue_id = next_numbered_id(&existing, "dialogue");

    app.capture_history();
    let mut dialogue_index = 0;
    if let Some(bundle) = &mut app.bundle {
        bundle.dialogues.push(DialogueGraph {
            id: dialogue_id.clone(),
            opening_node: "start".to_string(),
            nodes: vec![DialogueNode {
                id: "start".to_string(),
                speaker: String::new(),
                text: String::new(),
                commands: Vec::new(),
                choices: Vec::new(),
                next: None,
            }],
        });
        dialogue_index = bundle.dialogues.len().saturating_sub(1);
    }
    app.mark_edited(format!("Added dialogue '{}'", dialogue_id));
    (dialogue_index, dialogue_id)
}

fn remove_dialogue(app: &mut EditorApp, dialogue_index: usize) -> Option<String> {
    let dialogue_id = app
        .bundle
        .as_ref()
        .and_then(|bundle| bundle.dialogues.get(dialogue_index))
        .map(|dialogue| dialogue.id.clone())?;

    app.capture_history();
    if let Some(bundle) = &mut app.bundle {
        if dialogue_index < bundle.dialogues.len() {
            bundle.dialogues.remove(dialogue_index);
            let fallback_dialogue = bundle.dialogues.first().map(|dialogue| dialogue.id.clone());
            clear_removed_dialogue_references(bundle, &dialogue_id, fallback_dialogue.as_deref());
        }
    }
    app.selected_dialogue = app
        .bundle
        .as_ref()
        .map(|bundle| sanitize_optional_index(Some(dialogue_index), bundle.dialogues.len()))
        .unwrap_or(None);
    app.selected_dialogue_node = app
        .selected_dialogue
        .and_then(|index| app.bundle.as_ref()?.dialogues.get(index))
        .and_then(|dialogue| sanitize_optional_index(Some(0), dialogue.nodes.len()));
    app.mark_edited(format!("Removed dialogue '{}'", dialogue_id));
    Some(dialogue_id)
}

fn create_scene_script(
    app: &mut EditorApp,
    scene_index: usize,
    base: &str,
) -> Option<(usize, String)> {
    let existing = app
        .bundle
        .as_ref()
        .and_then(|bundle| bundle.scenes.get(scene_index))
        .map(|scene| {
            scene
                .scripts
                .iter()
                .map(|script| script.id.clone())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let script_id = next_numbered_id(&existing, base);

    if let Some(scene) = app
        .bundle
        .as_mut()
        .and_then(|bundle| bundle.scenes.get_mut(scene_index))
    {
        scene.scripts.push(EventScript {
            id: script_id.clone(),
            commands: Vec::new(),
        });
        return Some((scene.scripts.len().saturating_sub(1), script_id));
    }

    None
}

fn remove_scene_script(
    app: &mut EditorApp,
    scene_index: usize,
    script_index: usize,
) -> Option<String> {
    let script_id = app
        .bundle
        .as_ref()
        .and_then(|bundle| bundle.scenes.get(scene_index))
        .and_then(|scene| scene.scripts.get(script_index))
        .map(|script| script.id.clone())?;

    app.capture_history();
    if let Some(scene) = app
        .bundle
        .as_mut()
        .and_then(|bundle| bundle.scenes.get_mut(scene_index))
    {
        if script_index < scene.scripts.len() {
            scene.scripts.remove(script_index);
            let fallback = scene.scripts.first().map(|script| script.id.clone());
            clear_removed_script_references(scene, &script_id, fallback.as_deref());
        }
    }
    app.selected_script = app
        .bundle
        .as_ref()
        .and_then(|bundle| bundle.scenes.get(scene_index))
        .map(|scene| sanitize_optional_index(Some(script_index), scene.scripts.len()))
        .unwrap_or(None);
    app.mark_edited(format!("Removed script '{}'", script_id));
    Some(script_id)
}

fn normalize_dialogue(dialogue: &mut DialogueGraph, fallback_id: &str) {
    if dialogue.id.trim().is_empty() {
        dialogue.id = fallback_id.to_string();
    }
    if dialogue.nodes.is_empty() {
        dialogue.nodes.push(DialogueNode {
            id: "start".to_string(),
            speaker: String::new(),
            text: String::new(),
            commands: Vec::new(),
            choices: Vec::new(),
            next: None,
        });
    }

    let mut seen = BTreeSet::new();
    for node in &mut dialogue.nodes {
        if node.id.trim().is_empty() {
            node.id = "node".to_string();
        }
        if !seen.insert(node.id.clone()) {
            node.id = next_numbered_id(&seen, &node.id);
            seen.insert(node.id.clone());
        }
    }

    let node_ids = dialogue
        .nodes
        .iter()
        .map(|node| node.id.clone())
        .collect::<BTreeSet<_>>();
    if !node_ids.contains(&dialogue.opening_node) {
        dialogue.opening_node = dialogue.nodes[0].id.clone();
    }
    for node in &mut dialogue.nodes {
        if node
            .next
            .as_ref()
            .is_some_and(|next| !node_ids.contains(next))
        {
            node.next = None;
        }
        for choice in &mut node.choices {
            if !node_ids.contains(&choice.next) {
                choice.next = dialogue.opening_node.clone();
            }
        }
    }
}

fn rename_dialogue_node_refs(dialogue: &mut DialogueGraph, old_id: &str, new_id: &str) {
    if dialogue.opening_node == old_id {
        dialogue.opening_node = new_id.to_string();
    }
    for node in &mut dialogue.nodes {
        if node.next.as_deref() == Some(old_id) {
            node.next = Some(new_id.to_string());
        }
        for choice in &mut node.choices {
            if choice.next == old_id {
                choice.next = new_id.to_string();
            }
        }
    }
}

fn remove_dialogue_node_refs(
    dialogue: &mut DialogueGraph,
    removed_id: &str,
    fallback: Option<&str>,
) {
    if dialogue.opening_node == removed_id {
        dialogue.opening_node = fallback.unwrap_or_default().to_string();
    }
    for node in &mut dialogue.nodes {
        if node.next.as_deref() == Some(removed_id) {
            node.next = fallback.map(ToString::to_string);
        }
        for choice in &mut node.choices {
            if choice.next == removed_id {
                choice.next = fallback.unwrap_or_default().to_string();
            }
        }
    }
}

fn rename_dialogue_references(bundle: &mut ProjectBundle, old_id: &str, new_id: &str) {
    for scene in &mut bundle.scenes {
        for script in &mut scene.scripts {
            for command in &mut script.commands {
                if let EventCommand::ShowDialogue { dialogue_id, .. } = command {
                    if dialogue_id == old_id {
                        *dialogue_id = new_id.to_string();
                    }
                }
            }
        }
    }
}

fn rename_script_dialogue_node_references(
    bundle: &mut ProjectBundle,
    dialogue_id: &str,
    old_node_id: &str,
    new_node_id: &str,
) {
    for scene in &mut bundle.scenes {
        for script in &mut scene.scripts {
            for command in &mut script.commands {
                if let EventCommand::ShowDialogue {
                    dialogue_id: target_dialogue_id,
                    node_id,
                } = command
                {
                    if target_dialogue_id == dialogue_id && node_id.as_deref() == Some(old_node_id)
                    {
                        *node_id = Some(new_node_id.to_string());
                    }
                }
            }
        }
    }
}

fn clear_removed_dialogue_references(
    bundle: &mut ProjectBundle,
    removed_dialogue_id: &str,
    fallback_dialogue_id: Option<&str>,
) {
    for scene in &mut bundle.scenes {
        for script in &mut scene.scripts {
            for command in &mut script.commands {
                if let EventCommand::ShowDialogue {
                    dialogue_id,
                    node_id,
                } = command
                {
                    if dialogue_id == removed_dialogue_id {
                        *dialogue_id = fallback_dialogue_id.unwrap_or_default().to_string();
                        *node_id = None;
                    }
                }
            }
        }
    }
}

fn rename_trigger_script_references(scene: &mut SceneResource, old_id: &str, new_id: &str) {
    for trigger in &mut scene.triggers {
        if trigger.script_id == old_id {
            trigger.script_id = new_id.to_string();
        }
    }
}

fn clear_removed_script_references(
    scene: &mut SceneResource,
    removed_script_id: &str,
    fallback_script_id: Option<&str>,
) {
    for trigger in &mut scene.triggers {
        if trigger.script_id == removed_script_id {
            trigger.script_id = fallback_script_id.unwrap_or_default().to_string();
        }
    }
}

fn dialogue_preview_lines(dialogue: &DialogueGraph) -> Vec<String> {
    let mut lines = Vec::new();
    let nodes_by_id = dialogue
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<BTreeMap<_, _>>();
    let mut cursor = dialogue.opening_node.clone();
    let mut visited = BTreeSet::new();

    for _ in 0..8 {
        if cursor.is_empty() {
            break;
        }
        if !visited.insert(cursor.clone()) {
            lines.push(format!("Loop back to {}", cursor));
            break;
        }
        let Some(node) = nodes_by_id.get(cursor.as_str()) else {
            lines.push(format!("Missing node {}", cursor));
            break;
        };
        let speaker = if node.speaker.trim().is_empty() {
            "Narrator"
        } else {
            node.speaker.as_str()
        };
        lines.push(format!("{}: {}", speaker, truncate_inline(&node.text, 64)));
        if !node.choices.is_empty() {
            for choice in node.choices.iter().take(3) {
                lines.push(format!(
                    "Choice: {} -> {}",
                    truncate_inline(&choice.text, 36),
                    choice.next
                ));
            }
            break;
        }
        if let Some(next) = &node.next {
            cursor = next.clone();
        } else {
            break;
        }
    }

    if lines.is_empty() {
        lines.push("Empty dialogue".to_string());
    }
    lines
}

fn truncate_inline(value: &str, max_chars: usize) -> String {
    let value = value.trim().replace('\n', " ");
    let mut chars = value.chars();
    let preview = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{}...", preview.trim_end())
    } else if preview.is_empty() {
        "(empty)".to_string()
    } else {
        preview
    }
}

fn next_numbered_id(existing: &BTreeSet<String>, base: &str) -> String {
    let stem = slugify(base);
    if stem.is_empty() {
        return next_numbered_id(existing, "item");
    }
    if !existing.contains(&stem) {
        return stem;
    }

    let mut suffix = 2;
    loop {
        let candidate = format!("{}_{}", stem, suffix);
        if !existing.contains(&candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialogue_preview_follows_opening_path() {
        let dialogue = DialogueGraph {
            id: "intro".to_string(),
            opening_node: "start".to_string(),
            nodes: vec![
                DialogueNode {
                    id: "start".to_string(),
                    speaker: "Guide".to_string(),
                    text: "Welcome aboard".to_string(),
                    commands: Vec::new(),
                    choices: Vec::new(),
                    next: Some("followup".to_string()),
                },
                DialogueNode {
                    id: "followup".to_string(),
                    speaker: String::new(),
                    text: "Stay alert".to_string(),
                    commands: Vec::new(),
                    choices: Vec::new(),
                    next: None,
                },
            ],
        };

        let lines = dialogue_preview_lines(&dialogue);
        assert_eq!(lines[0], "Guide: Welcome aboard");
        assert_eq!(lines[1], "Narrator: Stay alert");
    }

    #[test]
    fn renaming_dialogue_node_updates_links() {
        let mut dialogue = DialogueGraph {
            id: "intro".to_string(),
            opening_node: "start".to_string(),
            nodes: vec![
                DialogueNode {
                    id: "start".to_string(),
                    speaker: String::new(),
                    text: String::new(),
                    commands: Vec::new(),
                    choices: vec![DialogueChoice {
                        text: "Go".to_string(),
                        next: "branch".to_string(),
                        condition_flag: None,
                    }],
                    next: Some("branch".to_string()),
                },
                DialogueNode {
                    id: "branch".to_string(),
                    speaker: String::new(),
                    text: String::new(),
                    commands: Vec::new(),
                    choices: Vec::new(),
                    next: None,
                },
            ],
        };

        rename_dialogue_node_refs(&mut dialogue, "branch", "branch_2");

        assert_eq!(dialogue.nodes[0].next.as_deref(), Some("branch_2"));
        assert_eq!(dialogue.nodes[0].choices[0].next, "branch_2");
    }

    #[test]
    fn dialogue_graph_columns_place_unreachable_nodes_after_main_flow() {
        let dialogue = DialogueGraph {
            id: "intro".to_string(),
            opening_node: "start".to_string(),
            nodes: vec![
                DialogueNode {
                    id: "start".to_string(),
                    speaker: String::new(),
                    text: String::new(),
                    commands: Vec::new(),
                    choices: vec![DialogueChoice {
                        text: "Branch".to_string(),
                        next: "branch".to_string(),
                        condition_flag: None,
                    }],
                    next: Some("followup".to_string()),
                },
                DialogueNode {
                    id: "followup".to_string(),
                    speaker: String::new(),
                    text: String::new(),
                    commands: Vec::new(),
                    choices: Vec::new(),
                    next: None,
                },
                DialogueNode {
                    id: "branch".to_string(),
                    speaker: String::new(),
                    text: String::new(),
                    commands: Vec::new(),
                    choices: Vec::new(),
                    next: None,
                },
                DialogueNode {
                    id: "secret".to_string(),
                    speaker: String::new(),
                    text: String::new(),
                    commands: Vec::new(),
                    choices: Vec::new(),
                    next: None,
                },
            ],
        };

        let columns = dialogue_graph_columns(&dialogue);

        assert_eq!(columns[0], vec![0]);
        assert_eq!(columns[1], vec![1, 2]);
        assert_eq!(columns[2], vec![3]);
    }

    #[test]
    fn parses_event_diagnostic_targets() {
        assert_eq!(
            parse_event_diagnostic_path("dialogue:intro:node:branch"),
            Some(EventDiagnosticTarget::Dialogue {
                dialogue_id: "intro".to_string(),
                node_id: Some("branch".to_string()),
            })
        );
        assert_eq!(
            parse_event_diagnostic_path("scene:intro_stage:script:start_dialogue"),
            Some(EventDiagnosticTarget::Script {
                scene_id: "intro_stage".to_string(),
                script_id: "start_dialogue".to_string(),
            })
        );
        assert_eq!(
            parse_event_diagnostic_path("scene:intro_stage:trigger:intro_dialogue"),
            Some(EventDiagnosticTarget::Trigger {
                scene_id: "intro_stage".to_string(),
                trigger_id: "intro_dialogue".to_string(),
            })
        );
    }
}
