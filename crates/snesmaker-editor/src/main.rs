use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use eframe::egui::{
    self, Align, Align2, Color32, ColorImage, FontId, Key, KeyboardShortcut, Layout, Modifiers,
    Pos2, Rect, Sense, StrokeKind, TextureHandle, TextureOptions, Vec2, ViewportCommand,
};
use image::RgbaImage;
use rfd::FileDialog;
use snesmaker_events::{DialogueGraph, EventCommand, EventScript, TriggerKind};
use snesmaker_export::{BuildOutcome, build_rom};
use snesmaker_platformer::{InputFrame, PlaytestSession, simulate_trace};
use snesmaker_project::{
    default_entity_hitbox, slugify, AnimationFrame, AnimationResource, Checkpoint, CombatProfile,
    EntityAction, EntityKind, EntityPlacement, Facing, HealthHudStyle, MetaspriteResource,
    MovementPattern, PaletteResource, PointI16, ProjectBundle, RectI16, RgbaColor, SceneResource,
    SpawnPoint, SpriteTileRef, Tile8, TileLayer, TilesetResource, TriggerVolume, PhysicsProfile,
    PROJECT_SPRITE_SOURCE_DIR,
};
use snesmaker_validator::{
    Diagnostic, MAX_COLORS_PER_PALETTE, MAX_METASPRITE_TILES_HARD, MAX_METASPRITE_TILES_WARN,
    MAX_PALETTES, MAX_TILESET_TILES, ROM_BANK_SIZE, Severity, ValidationReport, validate_project,
};

mod workspace;

use workspace::{
    DockArea, DockLayout, DockSlot, DockTab, SavedDockLayout, SavedSceneSnippet, SavedTileBrush,
    WorkspaceAddons, WorkspaceFile, copy_workspace_file, load_workspace_addons,
    load_workspace_file, save_workspace_addons, save_workspace_file,
};

const HISTORY_LIMIT: usize = 64;
const SCENE_MIN_ZOOM: f32 = 2.0;
const SCENE_MAX_ZOOM: f32 = 20.0;
const PROJECT_SPRITE_LIBRARY_MAX_HEIGHT: f32 = 180.0;

fn main() -> Result<()> {
    let project_root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().expect("cwd"));
    let project_root = Utf8PathBuf::from_path_buf(project_root)
        .map_err(|_| anyhow!("project path must be utf-8"))?;

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1540.0, 940.0])
            .with_min_inner_size([1180.0, 720.0]),
        ..Default::default()
    };

    eframe::run_native(
        "SNES Maker",
        native_options,
        Box::new(move |_cc| Ok(Box::new(EditorApp::new(project_root.clone())))),
    )
    .map_err(|error| anyhow!(error.to_string()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditorTool {
    Select,
    Paint,
    Erase,
    Solid,
    Ladder,
    Hazard,
    Spawn,
    Checkpoint,
    Entity,
    Trigger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectionAction {
    PaintTile(u16),
    SetSolid(bool),
    SetLadder(bool),
    SetHazard(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SceneObjectGroup {
    Spawns,
    Checkpoints,
    Entities,
    Triggers,
    Scripts,
}

impl SceneObjectGroup {
    fn label(self) -> &'static str {
        match self {
            Self::Spawns => "Spawns",
            Self::Checkpoints => "Checkpoints",
            Self::Entities => "Entities",
            Self::Triggers => "Triggers",
            Self::Scripts => "Scripts",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum DiagnosticGrouping {
    #[default]
    Severity,
    Code,
    Path,
}

impl DiagnosticGrouping {
    fn label(self) -> &'static str {
        match self {
            Self::Severity => "Severity",
            Self::Code => "Code",
            Self::Path => "Path",
        }
    }
}

#[derive(Debug, Clone, Default)]
struct DiagnosticsViewState {
    search: String,
    show_errors: bool,
    show_warnings: bool,
    grouping: DiagnosticGrouping,
}

impl DiagnosticsViewState {
    fn new() -> Self {
        Self {
            search: String::new(),
            show_errors: true,
            show_warnings: true,
            grouping: DiagnosticGrouping::Severity,
        }
    }
}

impl EditorTool {
    fn label(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::Paint => "Paint",
            Self::Erase => "Erase",
            Self::Solid => "Solid",
            Self::Ladder => "Ladder",
            Self::Hazard => "Hazard",
            Self::Spawn => "Spawn",
            Self::Checkpoint => "Checkpoint",
            Self::Entity => "Entity",
            Self::Trigger => "Trigger",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct TileSelectionRect {
    min_x: usize,
    min_y: usize,
    max_x: usize,
    max_y: usize,
}

impl TileSelectionRect {
    fn from_points(a: (usize, usize), b: (usize, usize)) -> Self {
        Self {
            min_x: a.0.min(b.0),
            min_y: a.1.min(b.1),
            max_x: a.0.max(b.0),
            max_y: a.1.max(b.1),
        }
    }

    fn width_tiles(self) -> usize {
        self.max_x.saturating_sub(self.min_x) + 1
    }

    fn height_tiles(self) -> usize {
        self.max_y.saturating_sub(self.min_y) + 1
    }

    fn origin_pixels(self) -> PointI16 {
        PointI16 {
            x: (self.min_x * 8) as i16,
            y: (self.min_y * 8) as i16,
        }
    }

    fn contains_point_pixels(self, point: PointI16) -> bool {
        let min = self.origin_pixels();
        let max_x = ((self.max_x + 1) * 8) as i16;
        let max_y = ((self.max_y + 1) * 8) as i16;
        point.x >= min.x && point.x < max_x && point.y >= min.y && point.y < max_y
    }

    fn intersects_rect_pixels(self, rect: RectI16) -> bool {
        let min = self.origin_pixels();
        let max_x = ((self.max_x + 1) * 8) as i16;
        let max_y = ((self.max_y + 1) * 8) as i16;
        let rect_right = rect.x.saturating_add(rect.width as i16);
        let rect_bottom = rect.y.saturating_add(rect.height as i16);
        rect.x < max_x && rect_right > min.x && rect.y < max_y && rect_bottom > min.y
    }
}

#[derive(Debug, Clone)]
struct SceneSelection {
    rect: TileSelectionRect,
    spawns: Vec<usize>,
    checkpoints: Vec<usize>,
    entities: Vec<usize>,
    triggers: Vec<usize>,
}

#[derive(Debug, Clone)]
struct SceneClipboard {
    width_tiles: usize,
    height_tiles: usize,
    tiles: Vec<u16>,
    solids: Vec<bool>,
    ladders: Vec<bool>,
    hazards: Vec<bool>,
    spawns: Vec<SpawnPoint>,
    checkpoints: Vec<Checkpoint>,
    entities: Vec<EntityPlacement>,
    triggers: Vec<TriggerVolume>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum PreviewFocus {
    #[default]
    None,
    Animation,
    Entity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspacePreset {
    LevelDesign,
    Animation,
    Eventing,
    Debug,
    Custom,
}

impl WorkspacePreset {
    fn label(self) -> &'static str {
        match self {
            Self::LevelDesign => "Level Design",
            Self::Animation => "Animation",
            Self::Eventing => "Eventing",
            Self::Debug => "Debug",
            Self::Custom => "Custom",
        }
    }

    fn layout(self) -> DockLayout {
        match self {
            Self::LevelDesign => DockLayout {
                show_status_bar: true,
                left: DockSlot::new(320.0, vec![DockTab::Toolbox, DockTab::Outliner], 0),
                center: DockSlot::new(0.0, vec![DockTab::Scene], 0),
                right: DockSlot::new(380.0, vec![DockTab::Inspector, DockTab::Animation], 0),
                bottom: DockSlot::new(
                    280.0,
                    vec![DockTab::Assets, DockTab::Diagnostics, DockTab::BuildReport, DockTab::Playtest],
                    0,
                ),
            },
            Self::Animation => DockLayout {
                show_status_bar: true,
                left: DockSlot::new(320.0, vec![DockTab::Assets, DockTab::Toolbox], 0),
                center: DockSlot::new(0.0, vec![DockTab::Scene], 0),
                right: DockSlot::new(380.0, vec![DockTab::Animation, DockTab::Inspector], 0),
                bottom: DockSlot::new(260.0, vec![DockTab::Diagnostics, DockTab::BuildReport], 0),
            },
            Self::Eventing => DockLayout {
                show_status_bar: true,
                left: DockSlot::new(320.0, vec![DockTab::Outliner, DockTab::Assets], 0),
                center: DockSlot::new(0.0, vec![DockTab::Scene], 0),
                right: DockSlot::new(380.0, vec![DockTab::Inspector, DockTab::Diagnostics], 0),
                bottom: DockSlot::new(260.0, vec![DockTab::BuildReport, DockTab::Playtest], 0),
            },
            Self::Debug => DockLayout {
                show_status_bar: true,
                left: DockSlot::new(320.0, vec![DockTab::Toolbox, DockTab::Outliner], 1),
                center: DockSlot::new(0.0, vec![DockTab::Scene], 0),
                right: DockSlot::new(380.0, vec![DockTab::Inspector, DockTab::Animation], 0),
                bottom: DockSlot::new(
                    300.0,
                    vec![DockTab::Diagnostics, DockTab::BuildReport, DockTab::Playtest, DockTab::Assets],
                    0,
                ),
            },
            Self::Custom => WorkspacePreset::LevelDesign.layout(),
        }
    }
}

#[derive(Debug, Clone)]
struct WorkspaceState {
    layout: DockLayout,
    saved_layouts: Vec<SavedDockLayout>,
    active_saved_layout: Option<String>,
}

impl WorkspaceState {
    fn for_preset(preset: WorkspacePreset) -> Self {
        Self {
            layout: preset.layout(),
            saved_layouts: Vec::new(),
            active_saved_layout: None,
        }
    }
}

#[derive(Default)]
struct SaveLayoutState {
    open: bool,
    name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlaytestStartMode {
    SceneStart,
    SelectedSpawn,
    SelectedCheckpoint,
}

impl PlaytestStartMode {
    fn label(self) -> &'static str {
        match self {
            Self::SceneStart => "Scene Start",
            Self::SelectedSpawn => "Selected Spawn",
            Self::SelectedCheckpoint => "Selected Checkpoint",
        }
    }
}

struct PlaytestState {
    last_status: String,
    session: Option<PlaytestSession>,
    playing: bool,
    speed_multiplier: f32,
    accumulated_seconds: f32,
    selected_physics_id: String,
    start_mode: PlaytestStartMode,
    show_camera_bounds: bool,
    show_spawns: bool,
    show_checkpoints: bool,
    show_triggers: bool,
    show_entities: bool,
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

#[derive(Default)]
struct UndoHistory {
    undo: Vec<ProjectBundle>,
    redo: Vec<ProjectBundle>,
}

impl UndoHistory {
    fn capture(&mut self, bundle: &ProjectBundle) {
        if self.undo.len() == HISTORY_LIMIT {
            self.undo.remove(0);
        }
        self.undo.push(bundle.clone());
        self.redo.clear();
    }

    fn undo(&mut self, current: &mut ProjectBundle) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo.push(current.clone());
        *current = previous;
        true
    }

    fn redo(&mut self, current: &mut ProjectBundle) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(current.clone());
        *current = next;
        true
    }

    fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

#[derive(Default)]
struct NewProjectState {
    open: bool,
    project_name: String,
    destination: String,
}

struct LoadedSheetPreview {
    rgba: Vec<u8>,
    size: [usize; 2],
    texture: TextureHandle,
}

#[derive(Default)]
struct SpriteSheetImportState {
    open: bool,
    source_path: String,
    base_id: String,
    animation_id: String,
    frame_width_px: u32,
    frame_height_px: u32,
    frame_count: usize,
    columns: usize,
    frame_duration: u8,
    target_tileset_id: String,
    target_palette_id: String,
    status: String,
    preview: Option<LoadedSheetPreview>,
}

impl SpriteSheetImportState {
    fn with_defaults() -> Self {
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

    fn sync_to_bundle(&mut self, bundle: &ProjectBundle) {
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
}

struct EditorApp {
    project_root: Utf8PathBuf,
    bundle: Option<ProjectBundle>,
    report: ValidationReport,
    status: String,
    dirty: bool,
    selected_scene: usize,
    selected_layer: usize,
    selected_tile: usize,
    selected_animation: usize,
    selected_spawn: Option<usize>,
    selected_checkpoint: Option<usize>,
    selected_entity: Option<usize>,
    selected_trigger: Option<usize>,
    preview_focus: PreviewFocus,
    tool: EditorTool,
    show_grid: bool,
    show_collision: bool,
    show_help: bool,
    scene_zoom: f32,
    scene_scroll_offset: Vec2,
    history: UndoHistory,
    active_canvas_cell: Option<usize>,
    selection: Option<SceneSelection>,
    selection_drag_anchor: Option<(usize, usize)>,
    clipboard: Option<SceneClipboard>,
    last_canvas_tile: Option<(usize, usize)>,
    confirm_exit: bool,
    import_state: SpriteSheetImportState,
    new_project_state: NewProjectState,
    workspace_preset: WorkspacePreset,
    workspace: WorkspaceState,
    workspace_addons: WorkspaceAddons,
    save_layout_state: SaveLayoutState,
    last_build_outcome: Option<BuildOutcome>,
    playtest_state: PlaytestState,
    outliner_filter: String,
    asset_browser_filter: String,
    asset_browser_sprite_previews: BTreeMap<String, TextureHandle>,
    locked_layers: BTreeSet<(usize, usize)>,
    solo_layer: Option<(usize, usize)>,
    solo_group: Option<SceneObjectGroup>,
    pending_focus_rect: Option<RectI16>,
    diagnostics_view: DiagnosticsViewState,
}

impl EditorApp {
    fn new(project_root: Utf8PathBuf) -> Self {
        let mut app = Self {
            project_root,
            bundle: None,
            report: ValidationReport::default(),
            status: String::new(),
            dirty: false,
            selected_scene: 0,
            selected_layer: 0,
            selected_tile: 0,
            selected_animation: 0,
            selected_spawn: Some(0),
            selected_checkpoint: Some(0),
            selected_entity: Some(0),
            selected_trigger: Some(0),
            preview_focus: PreviewFocus::None,
            tool: EditorTool::Select,
            show_grid: true,
            show_collision: true,
            show_help: false,
            scene_zoom: 8.0,
            scene_scroll_offset: Vec2::ZERO,
            history: UndoHistory::default(),
            active_canvas_cell: None,
            selection: None,
            selection_drag_anchor: None,
            clipboard: None,
            last_canvas_tile: None,
            confirm_exit: false,
            import_state: SpriteSheetImportState::with_defaults(),
            new_project_state: NewProjectState {
                open: false,
                project_name: "My SNES Game".to_string(),
                destination: String::new(),
            },
            workspace_preset: WorkspacePreset::LevelDesign,
            workspace: WorkspaceState::for_preset(WorkspacePreset::LevelDesign),
            workspace_addons: WorkspaceAddons::default(),
            save_layout_state: SaveLayoutState::default(),
            last_build_outcome: None,
            playtest_state: PlaytestState::default(),
            outliner_filter: String::new(),
            asset_browser_filter: String::new(),
            asset_browser_sprite_previews: BTreeMap::new(),
            locked_layers: BTreeSet::new(),
            solo_layer: None,
            solo_group: None,
            pending_focus_rect: None,
            diagnostics_view: DiagnosticsViewState::new(),
        };
        app.reload();
        app
    }

    fn reload(&mut self) {
        match ProjectBundle::load(&self.project_root) {
            Ok(bundle) => {
                self.bundle = Some(bundle);
                self.report = validate_project(self.bundle.as_ref().expect("bundle"));
                self.load_workspace_state();
                self.refresh_last_build_report();
                self.status = format!("Loaded {}", self.project_root);
                self.dirty = false;
                self.history.clear();
                self.active_canvas_cell = None;
                self.selection = None;
                self.selection_drag_anchor = None;
                self.last_canvas_tile = None;
                self.preview_focus = PreviewFocus::None;
                self.scene_scroll_offset = Vec2::ZERO;
                self.locked_layers.clear();
                self.solo_layer = None;
                self.solo_group = None;
                self.pending_focus_rect = None;
                self.asset_browser_sprite_previews.clear();
                self.sync_selection();
            }
            Err(error) => {
                self.bundle = None;
                self.report = ValidationReport::default();
                self.workspace_preset = WorkspacePreset::LevelDesign;
                self.workspace = WorkspaceState::for_preset(WorkspacePreset::LevelDesign);
                self.workspace_addons = WorkspaceAddons::default();
                self.last_build_outcome = None;
                self.status = error.to_string();
                self.dirty = false;
                self.history.clear();
                self.active_canvas_cell = None;
                self.selection = None;
                self.selection_drag_anchor = None;
                self.last_canvas_tile = None;
                self.preview_focus = PreviewFocus::None;
                self.scene_scroll_offset = Vec2::ZERO;
                self.locked_layers.clear();
                self.solo_layer = None;
                self.solo_group = None;
                self.pending_focus_rect = None;
                self.asset_browser_sprite_previews.clear();
            }
        }
    }

    fn open_project(&mut self, root: Utf8PathBuf) {
        self.project_root = root;
        self.reload();
    }

    fn sync_selection(&mut self) {
        let Some(bundle) = &self.bundle else {
            self.selected_scene = 0;
            self.selected_layer = 0;
            self.selected_tile = 0;
            self.selected_animation = 0;
            self.selected_spawn = None;
            self.selected_checkpoint = None;
            self.selected_entity = None;
            self.selected_trigger = None;
            return;
        };

        self.selected_scene = self
            .selected_scene
            .min(bundle.scenes.len().saturating_sub(1));
        self.selected_animation = self
            .selected_animation
            .min(bundle.animations.len().saturating_sub(1));
        self.import_state.sync_to_bundle(bundle);

        if let Some(scene) = bundle.scenes.get(self.selected_scene) {
            self.selected_layer = self
                .selected_layer
                .min(scene.layers.len().saturating_sub(1));
            self.selected_tile = scene
                .layers
                .get(self.selected_layer)
                .and_then(|layer| bundle.tileset(&layer.tileset_id))
                .map(|tileset| {
                    self.selected_tile
                        .min(tileset.tiles.len().saturating_sub(1))
                })
                .unwrap_or(0);
            self.selected_spawn = sanitize_optional_index(self.selected_spawn, scene.spawns.len());
            self.selected_checkpoint =
                sanitize_optional_index(self.selected_checkpoint, scene.checkpoints.len());
            self.selected_entity =
                sanitize_optional_index(self.selected_entity, scene.entities.len());
            self.selected_trigger =
                sanitize_optional_index(self.selected_trigger, scene.triggers.len());
        } else {
            self.selected_layer = 0;
            self.selected_tile = 0;
            self.selected_spawn = None;
            self.selected_checkpoint = None;
            self.selected_entity = None;
            self.selected_trigger = None;
        }

        if self.preview_focus == PreviewFocus::Entity && self.selected_entity.is_none() {
            self.preview_focus = PreviewFocus::None;
        }
        if self.preview_focus == PreviewFocus::Animation && bundle.animations.is_empty() {
            self.preview_focus = PreviewFocus::None;
        }
    }

    fn refresh_report(&mut self) {
        if let Some(bundle) = &self.bundle {
            self.report = validate_project(bundle);
            self.sync_selection();
        } else {
            self.report = ValidationReport::default();
        }
    }

    fn sprite_source_preview_texture(
        &mut self,
        ctx: &egui::Context,
        path: &Utf8Path,
    ) -> Option<TextureHandle> {
        let key = path.as_str().to_string();
        if let Some(texture) = self.asset_browser_sprite_previews.get(&key) {
            return Some(texture.clone());
        }

        let preview = load_sheet_preview(ctx, path.as_std_path()).ok()?;
        let texture = preview.texture;
        self.asset_browser_sprite_previews
            .insert(key, texture.clone());
        Some(texture)
    }

    fn diagnostic_matches_filters(&self, diagnostic: &Diagnostic) -> bool {
        let severity_matches = match diagnostic.severity {
            Severity::Error => self.diagnostics_view.show_errors,
            Severity::Warning => self.diagnostics_view.show_warnings,
        };
        if !severity_matches {
            return false;
        }

        let search = self.diagnostics_view.search.trim().to_ascii_lowercase();
        if search.is_empty() {
            return true;
        }

        diagnostic.code.to_ascii_lowercase().contains(&search)
            || diagnostic.message.to_ascii_lowercase().contains(&search)
            || diagnostic
                .path
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains(&search)
    }

    fn filtered_diagnostics(&self) -> Vec<Diagnostic> {
        self.report
            .errors
            .iter()
            .chain(self.report.warnings.iter())
            .filter(|diagnostic| self.diagnostic_matches_filters(diagnostic))
            .cloned()
            .collect()
    }

    fn navigate_to_diagnostic_path(&mut self, path: &str) {
        if path == "project.toml" {
            self.workspace.layout.show_tab(DockTab::Inspector);
            self.status = "Opened project settings in the inspector.".to_string();
            return;
        }

        if let Some(scene_id) = path.strip_prefix("scene:") {
            if let Some(scene_index) = self
                .bundle
                .as_ref()
                .and_then(|bundle| bundle.scenes.iter().position(|scene| scene.id == scene_id))
            {
                self.selected_scene = scene_index;
                self.sync_selection();
                self.focus_scene(scene_index);
            }
            return;
        }

        if let Some(dialogue_id) = path.strip_prefix("dialogue:") {
            self.asset_browser_filter = dialogue_id.to_string();
            self.workspace.layout.show_tab(DockTab::Assets);
            self.status = format!("Filtered asset browser to dialogue '{}'", dialogue_id);
            return;
        }

        for prefix in ["palette:", "tileset:", "metasprite:", "animation:"] {
            if let Some(asset_id) = path.strip_prefix(prefix) {
                self.asset_browser_filter = asset_id.to_string();
                self.workspace.layout.show_tab(DockTab::Assets);
                self.status = format!("Filtered asset browser to '{}'", asset_id);
                return;
            }
        }
    }

    fn diagnostic_has_quick_fix(&self, diagnostic: &Diagnostic) -> bool {
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

    fn apply_diagnostic_quick_fix(&mut self, diagnostic: &Diagnostic) {
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
                if let Some(palette_id) = diagnostic.path.as_deref().and_then(|path| path.strip_prefix("palette:")) {
                    if let Some(palette) = edited_bundle
                        .palettes
                        .iter_mut()
                        .find(|palette| palette.id == palette_id)
                    {
                        palette.colors.truncate(MAX_COLORS_PER_PALETTE);
                        status = Some(format!("Trimmed palette '{}' to {} colors", palette.id, MAX_COLORS_PER_PALETTE));
                    }
                }
            }
            "asset.missing_palette" => {
                let first_palette = edited_bundle.palettes.first().map(|palette| palette.id.clone());
                if let (Some(tileset_id), Some(palette_id)) = (
                    diagnostic.path.as_deref().and_then(|path| path.strip_prefix("tileset:")),
                    first_palette,
                ) {
                    if let Some(tileset) = edited_bundle
                        .tilesets
                        .iter_mut()
                        .find(|tileset| tileset.id == tileset_id)
                    {
                        tileset.palette_id = palette_id.clone();
                        status = Some(format!("Reassigned '{}' to palette '{}'", tileset.id, palette_id));
                    }
                }
            }
            "scene.trigger_missing_script" => {
                if let Some(scene_id) = diagnostic.path.as_deref().and_then(|path| path.strip_prefix("scene:")) {
                    if let Some(scene) = edited_bundle.scenes.iter_mut().find(|scene| scene.id == scene_id) {
                        let missing_script_ids = scene
                            .triggers
                            .iter()
                            .filter(|trigger| scene.scripts.iter().all(|script| script.id != trigger.script_id))
                            .map(|trigger| trigger.script_id.clone())
                            .collect::<Vec<_>>();
                        for script_id in &missing_script_ids {
                            scene.scripts.push(snesmaker_events::EventScript {
                                id: script_id.clone(),
                                commands: Vec::new(),
                            });
                        }
                        if !missing_script_ids.is_empty() {
                            status = Some(format!("Added {} missing script stub(s) to '{}'", missing_script_ids.len(), scene.id));
                        }
                    }
                }
            }
            "script.dialogue_missing" => {
                let placeholder_id = "auto_dialogue".to_string();
                if edited_bundle.dialogues.iter().all(|dialogue| dialogue.id != placeholder_id) {
                    edited_bundle.dialogues.push(snesmaker_events::DialogueGraph {
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
                            if let snesmaker_events::EventCommand::ShowDialogue { dialogue_id, .. } = command {
                                if !valid_dialogues.contains(dialogue_id) {
                                    *dialogue_id = placeholder_id.clone();
                                }
                            }
                        }
                    }
                }
                status = Some("Redirected missing dialogue references to a placeholder dialogue".to_string());
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
                                if let snesmaker_events::EventCommand::LoadScene { scene_id, .. } = command {
                                    if !valid_scenes.contains(scene_id) {
                                        *scene_id = fallback_scene.clone();
                                    }
                                }
                            }
                        }
                    }
                    status = Some(format!("Redirected missing scene loads to '{}'", fallback_scene));
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

    fn detect_workspace_preset(layout: &DockLayout) -> WorkspacePreset {
        for preset in [
            WorkspacePreset::LevelDesign,
            WorkspacePreset::Animation,
            WorkspacePreset::Eventing,
            WorkspacePreset::Debug,
        ] {
            if layout == &preset.layout() {
                return preset;
            }
        }
        WorkspacePreset::Custom
    }

    fn load_workspace_state(&mut self) {
        match load_workspace_file(&self.project_root) {
            Ok(Some(mut workspace_file)) => {
                workspace_file.normalize();
                self.workspace.layout = workspace_file.current_layout;
                self.workspace.saved_layouts = workspace_file.saved_layouts;
                self.workspace.active_saved_layout = workspace_file.active_saved_layout;
                self.workspace_preset =
                    Self::detect_workspace_preset(&self.workspace.layout);
                let active_saved_layout = self
                    .workspace
                    .active_saved_layout
                    .clone()
                    .unwrap_or_else(|| self.workspace_preset.label().to_string());
                self.save_layout_state.name = active_saved_layout;
                self.workspace_addons = load_workspace_addons(&self.project_root)
                    .ok()
                    .flatten()
                    .unwrap_or_default();
            }
            Ok(None) => {
                self.workspace_preset = WorkspacePreset::LevelDesign;
                self.workspace = WorkspaceState::for_preset(WorkspacePreset::LevelDesign);
                self.workspace_addons = load_workspace_addons(&self.project_root)
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                self.save_layout_state.name = WorkspacePreset::LevelDesign.label().to_string();
            }
            Err(error) => {
                self.workspace_preset = WorkspacePreset::LevelDesign;
                self.workspace = WorkspaceState::for_preset(WorkspacePreset::LevelDesign);
                self.workspace_addons = WorkspaceAddons::default();
                self.save_layout_state.name = WorkspacePreset::LevelDesign.label().to_string();
                self.status = format!(
                    "Loaded {} with default workspace ({}).",
                    self.project_root, error
                );
            }
        }
    }

    fn persist_workspace_state(&mut self) {
        let workspace_file = WorkspaceFile {
            current_layout: self.workspace.layout.clone(),
            saved_layouts: self.workspace.saved_layouts.clone(),
            active_saved_layout: self.workspace.active_saved_layout.clone(),
        };

        if let Err(error) = save_workspace_file(&self.project_root, &workspace_file) {
            self.status = format!("Failed to save workspace layout: {}", error);
        }
    }

    fn persist_workspace_addons(&mut self) {
        if let Err(error) = save_workspace_addons(&self.project_root, &self.workspace_addons) {
            self.status = format!("Failed to save editor addons: {}", error);
        }
    }

    fn set_workspace_preset(&mut self, preset: WorkspacePreset) {
        if preset == WorkspacePreset::Custom {
            return;
        }

        self.workspace_preset = preset;
        self.workspace.layout = preset.layout();
        self.workspace.active_saved_layout = None;
        self.save_layout_state.name = preset.label().to_string();
        self.persist_workspace_state();
        self.status = format!("Workspace: {}", preset.label());
    }

    fn mark_workspace_custom(&mut self) {
        self.workspace_preset = WorkspacePreset::Custom;
        self.workspace.active_saved_layout = None;
        self.persist_workspace_state();
    }

    fn load_saved_workspace(&mut self, name: &str) {
        let Some(saved) = self
            .workspace
            .saved_layouts
            .iter()
            .find(|layout| layout.name.eq_ignore_ascii_case(name))
            .cloned()
        else {
            self.status = format!("Workspace layout '{}' no longer exists.", name);
            return;
        };

        self.workspace.layout = saved.layout;
        self.workspace_preset = Self::detect_workspace_preset(&self.workspace.layout);
        self.workspace.active_saved_layout = Some(saved.name.clone());
        self.save_layout_state.name = saved.name.clone();
        self.persist_workspace_state();
        self.status = format!("Loaded workspace layout '{}'", saved.name);
    }

    fn save_current_workspace_layout(&mut self) {
        let name = self.save_layout_state.name.trim().to_string();
        if name.is_empty() {
            self.status = "Enter a workspace layout name first.".to_string();
            return;
        }

        let saved = SavedDockLayout {
            name: name.clone(),
            layout: self.workspace.layout.clone(),
        };
        if let Some(existing) = self
            .workspace
            .saved_layouts
            .iter_mut()
            .find(|layout| layout.name.eq_ignore_ascii_case(&name))
        {
            *existing = saved;
        } else {
            self.workspace.saved_layouts.push(saved);
        }
        self.workspace
            .saved_layouts
            .sort_by(|left, right| left.name.cmp(&right.name));
        self.workspace.active_saved_layout = Some(name.clone());
        self.workspace_preset = WorkspacePreset::Custom;
        self.persist_workspace_state();
        self.save_layout_state.open = false;
        self.status = format!("Saved workspace layout '{}'", name);
    }

    fn delete_saved_workspace(&mut self, name: &str) {
        let before = self.workspace.saved_layouts.len();
        self.workspace
            .saved_layouts
            .retain(|layout| !layout.name.eq_ignore_ascii_case(name));

        if self.workspace.saved_layouts.len() == before {
            self.status = format!("Workspace layout '{}' was already removed.", name);
            return;
        }

        if self
            .workspace
            .active_saved_layout
            .as_deref()
            .is_some_and(|active| active.eq_ignore_ascii_case(name))
        {
            self.workspace.active_saved_layout = None;
        }
        self.persist_workspace_state();
        self.status = format!("Deleted workspace layout '{}'", name);
    }

    fn set_dock_tab_visibility(&mut self, tab: DockTab, visible: bool) {
        if visible {
            self.workspace.layout.show_tab(tab);
        } else {
            self.workspace.layout.hide_tab(tab);
        }
        self.mark_workspace_custom();
    }

    fn move_dock_tab(&mut self, tab: DockTab, area: DockArea) {
        self.workspace.layout.move_tab(tab, area);
        self.mark_workspace_custom();
    }

    fn move_active_dock_tab_within_slot(&mut self, area: DockArea, direction: i32) {
        self.workspace.layout.move_active_within_slot(area, direction);
        self.mark_workspace_custom();
    }

    fn capture_history(&mut self) {
        if let Some(bundle) = &self.bundle {
            self.history.capture(bundle);
        }
    }

    fn mark_edited(&mut self, status: impl Into<String>) {
        self.dirty = true;
        self.status = status.into();
        self.refresh_report();
    }

    fn current_scene(&self) -> Option<&SceneResource> {
        self.bundle
            .as_ref()
            .and_then(|bundle| bundle.scenes.get(self.selected_scene))
    }

    fn layer(
        &self,
        scene_index: usize,
        layer_index: usize,
    ) -> Option<&snesmaker_project::TileLayer> {
        let bundle = self.bundle.as_ref()?;
        let scene = bundle.scenes.get(scene_index)?;
        let layer_index = layer_index.min(scene.layers.len().saturating_sub(1));
        scene.layers.get(layer_index)
    }

    fn layer_mut(
        &mut self,
        scene_index: usize,
        layer_index: usize,
    ) -> Option<&mut snesmaker_project::TileLayer> {
        let bundle = self.bundle.as_mut()?;
        let scene = bundle.scenes.get_mut(scene_index)?;
        let layer_index = layer_index.min(scene.layers.len().saturating_sub(1));
        scene.layers.get_mut(layer_index)
    }

    fn current_layer(&self) -> Option<&snesmaker_project::TileLayer> {
        self.layer(self.selected_scene, self.selected_layer)
    }

    fn current_layer_mut(&mut self) -> Option<&mut snesmaker_project::TileLayer> {
        self.layer_mut(self.selected_scene, self.selected_layer)
    }

    fn active_tileset_and_palette(&self) -> Option<(&TilesetResource, &PaletteResource)> {
        let bundle = self.bundle.as_ref()?;
        let layer = self.current_layer()?;
        let tileset = bundle.tileset(&layer.tileset_id)?;
        let palette = bundle.palette(&tileset.palette_id)?;
        Some((tileset, palette))
    }

    fn is_layer_locked(&self, scene_index: usize, layer_index: usize) -> bool {
        self.locked_layers.contains(&(scene_index, layer_index))
    }

    fn active_layer_locked(&self) -> bool {
        self.is_layer_locked(self.selected_scene, self.selected_layer)
    }

    fn select_layer(&mut self, scene_index: usize, layer_index: usize) {
        self.selected_scene = scene_index;
        self.selected_layer = layer_index;
        self.scene_scroll_offset = Vec2::ZERO;
        self.clear_selection();
        self.preview_focus = PreviewFocus::None;
        self.sync_selection();
        if let Some(layer) = self.current_layer() {
            self.status = format!("Active layer: '{}'", layer.id);
        }
    }

    fn toggle_layer_lock(&mut self, scene_index: usize, layer_index: usize) {
        let key = (scene_index, layer_index);
        let locked = if self.locked_layers.contains(&key) {
            self.locked_layers.remove(&key);
            false
        } else {
            self.locked_layers.insert(key);
            true
        };

        if let Some(layer) = self.layer(scene_index, layer_index) {
            self.status = format!(
                "{} layer '{}'",
                if locked { "Locked" } else { "Unlocked" },
                layer.id
            );
        }
    }

    fn toggle_layer_visibility(&mut self, scene_index: usize, layer_index: usize) {
        let mut status = None;
        self.capture_history();
        if let Some(layer) = self.layer_mut(scene_index, layer_index) {
            layer.visible = !layer.visible;
            status = Some(format!(
                "{} layer '{}'",
                if layer.visible { "Showed" } else { "Hid" },
                layer.id
            ));
        }

        if let Some(status) = status {
            self.mark_edited(status);
        }
    }

    fn is_layer_soloed(&self, scene_index: usize, layer_index: usize) -> bool {
        self.solo_layer == Some((scene_index, layer_index))
    }

    fn toggle_layer_solo(&mut self, scene_index: usize, layer_index: usize) {
        if self.is_layer_soloed(scene_index, layer_index) {
            self.solo_layer = None;
            self.status = "Cleared layer solo".to_string();
        } else {
            self.solo_layer = Some((scene_index, layer_index));
            self.status = self
                .layer(scene_index, layer_index)
                .map(|layer| format!("Solo layer '{}'", layer.id))
                .unwrap_or_else(|| "Soloed layer".to_string());
        }
    }

    fn is_group_soloed(&self, group: SceneObjectGroup) -> bool {
        self.solo_group == Some(group)
    }

    fn toggle_group_solo(&mut self, group: SceneObjectGroup) {
        if self.is_group_soloed(group) {
            self.solo_group = None;
            self.status = "Cleared object-group solo".to_string();
        } else {
            self.solo_group = Some(group);
            self.status = format!("Solo {}", group.label());
        }
    }

    fn request_focus_rect(&mut self, rect: RectI16, status: impl Into<String>) {
        self.pending_focus_rect = Some(rect);
        self.status = status.into();
    }

    fn focus_scene(&mut self, scene_index: usize) {
        let Some(scene) = self
            .bundle
            .as_ref()
            .and_then(|bundle| bundle.scenes.get(scene_index))
        else {
            return;
        };

        self.request_focus_rect(
            RectI16 {
                x: 0,
                y: 0,
                width: scene.size_tiles.width.saturating_mul(8),
                height: scene.size_tiles.height.saturating_mul(8),
            },
            format!("Focused scene '{}'", scene.id),
        );
    }

    fn focus_layer(&mut self, scene_index: usize, layer_index: usize) {
        let Some(scene) = self
            .bundle
            .as_ref()
            .and_then(|bundle| bundle.scenes.get(scene_index))
        else {
            return;
        };
        let Some(layer) = scene.layers.get(layer_index) else {
            return;
        };

        let rect = layer_bounds_pixels(scene, layer).unwrap_or(RectI16 {
            x: 0,
            y: 0,
            width: scene.size_tiles.width.saturating_mul(8),
            height: scene.size_tiles.height.saturating_mul(8),
        });
        self.request_focus_rect(rect, format!("Focused layer '{}'", layer.id));
    }

    fn focus_point(&mut self, label: &str, point: PointI16) {
        self.request_focus_rect(
            RectI16 {
                x: point.x,
                y: point.y,
                width: 16,
                height: 16,
            },
            format!("Focused {}", label),
        );
    }

    fn focus_trigger_rect(&mut self, label: &str, rect: RectI16) {
        self.request_focus_rect(rect, format!("Focused {}", label));
    }

    fn duplicate_layer(&mut self, scene_index: usize, layer_index: usize) {
        let Some(scene) = self
            .bundle
            .as_ref()
            .and_then(|bundle| bundle.scenes.get(scene_index))
        else {
            return;
        };
        let Some(layer) = scene.layers.get(layer_index).cloned() else {
            return;
        };

        let existing_ids = scene
            .layers
            .iter()
            .map(|entry| entry.id.clone())
            .collect::<BTreeSet<_>>();
        let new_id = next_unique_layer_id(&existing_ids, &layer.id);

        self.capture_history();
        if let Some(scene) = self
            .bundle
            .as_mut()
            .and_then(|bundle| bundle.scenes.get_mut(scene_index))
        {
            let insert_at = (layer_index + 1).min(scene.layers.len());
            let mut duplicate = layer;
            duplicate.id = new_id.clone();
            scene.layers.insert(insert_at, duplicate);
            self.selected_scene = scene_index;
            self.selected_layer = insert_at;
            self.sync_selection();
            self.mark_edited(format!("Duplicated layer '{}'", new_id));
        }
    }

    fn clear_scene_layer_locks(&mut self, scene_index: usize) {
        self.locked_layers
            .retain(|(locked_scene_index, _)| *locked_scene_index != scene_index);
    }

    fn add_layer_to_current_scene(&mut self) {
        let Some(default_tileset_id) = self
            .current_layer()
            .map(|layer| layer.tileset_id.clone())
            .or_else(|| {
                self.bundle
                    .as_ref()
                    .and_then(|bundle| bundle.tilesets.first().map(|tileset| tileset.id.clone()))
            })
        else {
            self.status = "Add a tileset before creating a new layer.".to_string();
            return;
        };

        self.capture_history();
        let mut added_layer = None;
        if let Some(scene) = self.current_scene_mut() {
            let mut next_index = scene.layers.len() + 1;
            let id = loop {
                let candidate = format!("layer_{}", next_index);
                if scene.layers.iter().all(|layer| layer.id != candidate) {
                    break candidate;
                }
                next_index += 1;
            };

            scene.layers.push(TileLayer {
                id: id.clone(),
                tileset_id: default_tileset_id,
                visible: true,
                parallax_x: 1,
                parallax_y: 1,
                tiles: vec![0; scene.size_tiles.tile_count()],
            });
            added_layer = Some((scene.layers.len() - 1, id));
        }

        if let Some((layer_index, id)) = added_layer {
            self.selected_layer = layer_index;
            self.sync_selection();
            self.mark_edited(format!("Added layer '{}'", id));
        }
    }

    fn remove_selected_layer(&mut self) {
        let scene_index = self.selected_scene;
        let Some(scene) = self.current_scene() else {
            self.status = "No scene loaded.".to_string();
            return;
        };
        if scene.layers.len() <= 1 {
            self.status = "Each scene needs at least one layer.".to_string();
            return;
        }

        let selected_layer = self
            .selected_layer
            .min(scene.layers.len().saturating_sub(1));
        self.capture_history();
        let mut removed_layer_id = None;
        if let Some(scene) = self.current_scene_mut() {
            if selected_layer < scene.layers.len() {
                removed_layer_id = Some(scene.layers.remove(selected_layer).id);
            }
        }

        if let Some(layer_id) = removed_layer_id {
            self.clear_scene_layer_locks(scene_index);
            self.selected_layer = selected_layer.saturating_sub(1);
            self.sync_selection();
            self.mark_edited(format!("Removed layer '{}'", layer_id));
        }
    }

    fn move_selected_layer(&mut self, direction: i32) {
        let scene_index = self.selected_scene;
        let Some(scene) = self.current_scene() else {
            self.status = "No scene loaded.".to_string();
            return;
        };
        if scene.layers.is_empty() {
            self.status = "This scene has no layers.".to_string();
            return;
        }

        let selected_layer = self
            .selected_layer
            .min(scene.layers.len().saturating_sub(1));
        let target_layer = match direction {
            -1 if selected_layer > 0 => selected_layer - 1,
            1 if selected_layer + 1 < scene.layers.len() => selected_layer + 1,
            _ => return,
        };

        self.capture_history();
        let mut moved_layer_id = None;
        if let Some(scene) = self.current_scene_mut() {
            scene.layers.swap(selected_layer, target_layer);
            moved_layer_id = scene.layers.get(target_layer).map(|layer| layer.id.clone());
        }

        if let Some(layer_id) = moved_layer_id {
            self.clear_scene_layer_locks(scene_index);
            self.selected_layer = target_layer;
            self.sync_selection();
            self.mark_edited(format!("Moved layer '{}'", layer_id));
        }
    }

    fn clear_selection(&mut self) {
        self.selection = None;
        self.selection_drag_anchor = None;
    }

    fn commit_selection(&mut self, rect: TileSelectionRect) {
        let Some(scene) = self.current_scene() else {
            self.clear_selection();
            return;
        };

        let selection = build_scene_selection(scene, rect);
        self.selection = Some(selection);
        self.selection_drag_anchor = None;
        self.status = format!(
            "Selected {}x{} tiles",
            rect.width_tiles(),
            rect.height_tiles()
        );
    }

    fn copy_selection_to_clipboard(&mut self) {
        let Some(selection) = &self.selection else {
            self.status = "Nothing selected to copy.".to_string();
            return;
        };
        let Some(scene) = self.current_scene() else {
            self.status = "No scene loaded.".to_string();
            return;
        };
        let Some(layer) = self.current_layer() else {
            self.status = "Current scene has no active layer.".to_string();
            return;
        };

        let rect = selection.rect;
        let width_tiles = rect.width_tiles();
        let height_tiles = rect.height_tiles();
        let scene_width = scene.size_tiles.width as usize;
        let mut tiles = Vec::with_capacity(width_tiles * height_tiles);
        let mut solids = Vec::with_capacity(width_tiles * height_tiles);
        let mut ladders = Vec::with_capacity(width_tiles * height_tiles);
        let mut hazards = Vec::with_capacity(width_tiles * height_tiles);

        for tile_y in rect.min_y..=rect.max_y {
            for tile_x in rect.min_x..=rect.max_x {
                let cell_index = tile_y * scene_width + tile_x;
                tiles.push(layer.tiles.get(cell_index).copied().unwrap_or_default());
                solids.push(
                    scene
                        .collision
                        .solids
                        .get(cell_index)
                        .copied()
                        .unwrap_or(false),
                );
                ladders.push(
                    scene
                        .collision
                        .ladders
                        .get(cell_index)
                        .copied()
                        .unwrap_or(false),
                );
                hazards.push(
                    scene
                        .collision
                        .hazards
                        .get(cell_index)
                        .copied()
                        .unwrap_or(false),
                );
            }
        }

        let origin = rect.origin_pixels();
        let layer_id = layer.id.clone();
        self.clipboard = Some(SceneClipboard {
            width_tiles,
            height_tiles,
            tiles,
            solids,
            ladders,
            hazards,
            spawns: selection
                .spawns
                .iter()
                .filter_map(|index| scene.spawns.get(*index))
                .cloned()
                .map(|mut spawn| {
                    spawn.position.x -= origin.x;
                    spawn.position.y -= origin.y;
                    spawn
                })
                .collect(),
            checkpoints: selection
                .checkpoints
                .iter()
                .filter_map(|index| scene.checkpoints.get(*index))
                .cloned()
                .map(|mut checkpoint| {
                    checkpoint.position.x -= origin.x;
                    checkpoint.position.y -= origin.y;
                    checkpoint
                })
                .collect(),
            entities: selection
                .entities
                .iter()
                .filter_map(|index| scene.entities.get(*index))
                .cloned()
                .map(|mut entity| {
                    entity.position.x -= origin.x;
                    entity.position.y -= origin.y;
                    entity
                })
                .collect(),
            triggers: selection
                .triggers
                .iter()
                .filter_map(|index| scene.triggers.get(*index))
                .cloned()
                .map(|mut trigger| {
                    trigger.rect.x -= origin.x;
                    trigger.rect.y -= origin.y;
                    trigger
                })
                .collect(),
        });
        self.status = format!(
            "Copied {}x{} selection from '{}'",
            width_tiles, height_tiles, layer_id
        );
    }

    fn paste_clipboard(&mut self) {
        let Some(clipboard) = self.clipboard.clone() else {
            self.status = "Clipboard is empty.".to_string();
            return;
        };
        if self.active_layer_locked() {
            self.status = self
                .current_layer()
                .map(|layer| format!("Layer '{}' is locked.", layer.id))
                .unwrap_or_else(|| "Active layer is locked.".to_string());
            return;
        }
        let anchor = self
            .last_canvas_tile
            .or_else(|| {
                self.selection
                    .as_ref()
                    .map(|selection| (selection.rect.min_x, selection.rect.min_y))
            })
            .unwrap_or((0, 0));
        let origin = PointI16 {
            x: (anchor.0 * 8) as i16,
            y: (anchor.1 * 8) as i16,
        };
        let pasted_rect = TileSelectionRect {
            min_x: anchor.0,
            min_y: anchor.1,
            max_x: anchor.0 + clipboard.width_tiles.saturating_sub(1),
            max_y: anchor.1 + clipboard.height_tiles.saturating_sub(1),
        };

        self.capture_history();
        let mut new_selection = SceneSelection {
            rect: pasted_rect,
            spawns: Vec::new(),
            checkpoints: Vec::new(),
            entities: Vec::new(),
            triggers: Vec::new(),
        };

        let layer_index = self
            .current_scene()
            .map(|scene| {
                self.selected_layer
                    .min(scene.layers.len().saturating_sub(1))
            })
            .unwrap_or(0);
        if let Some(scene) = self.current_scene_mut() {
            let scene_width = scene.size_tiles.width as usize;
            let scene_height = scene.size_tiles.height as usize;
            if let Some(layer) = scene.layers.get_mut(layer_index) {
                for local_y in 0..clipboard.height_tiles {
                    for local_x in 0..clipboard.width_tiles {
                        let target_x = anchor.0 + local_x;
                        let target_y = anchor.1 + local_y;
                        if target_x >= scene_width || target_y >= scene_height {
                            continue;
                        }
                        let target_index = target_y * scene_width + target_x;
                        let source_index = local_y * clipboard.width_tiles + local_x;
                        if let Some(tile) = clipboard.tiles.get(source_index) {
                            layer.tiles[target_index] = *tile;
                        }
                        if let Some(value) = clipboard.solids.get(source_index) {
                            scene.collision.solids[target_index] = *value;
                        }
                        if let Some(value) = clipboard.ladders.get(source_index) {
                            scene.collision.ladders[target_index] = *value;
                        }
                        if let Some(value) = clipboard.hazards.get(source_index) {
                            scene.collision.hazards[target_index] = *value;
                        }
                    }
                }
            }

            let mut spawn_ids = scene
                .spawns
                .iter()
                .map(|spawn| spawn.id.clone())
                .collect::<BTreeSet<_>>();
            let mut checkpoint_ids = scene
                .checkpoints
                .iter()
                .map(|checkpoint| checkpoint.id.clone())
                .collect::<BTreeSet<_>>();
            let mut entity_ids = scene
                .entities
                .iter()
                .map(|entity| entity.id.clone())
                .collect::<BTreeSet<_>>();
            let mut trigger_ids = scene
                .triggers
                .iter()
                .map(|trigger| trigger.id.clone())
                .collect::<BTreeSet<_>>();

            for mut spawn in clipboard.spawns {
                spawn.id = next_unique_copy_id(&mut spawn_ids, &spawn.id);
                spawn.position.x += origin.x;
                spawn.position.y += origin.y;
                scene.spawns.push(spawn);
                new_selection.spawns.push(scene.spawns.len() - 1);
            }
            for mut checkpoint in clipboard.checkpoints {
                checkpoint.id = next_unique_copy_id(&mut checkpoint_ids, &checkpoint.id);
                checkpoint.position.x += origin.x;
                checkpoint.position.y += origin.y;
                scene.checkpoints.push(checkpoint);
                new_selection.checkpoints.push(scene.checkpoints.len() - 1);
            }
            for mut entity in clipboard.entities {
                entity.id = next_unique_copy_id(&mut entity_ids, &entity.id);
                entity.position.x += origin.x;
                entity.position.y += origin.y;
                scene.entities.push(entity);
                new_selection.entities.push(scene.entities.len() - 1);
            }
            for mut trigger in clipboard.triggers {
                trigger.id = next_unique_copy_id(&mut trigger_ids, &trigger.id);
                trigger.rect.x += origin.x;
                trigger.rect.y += origin.y;
                scene.triggers.push(trigger);
                new_selection.triggers.push(scene.triggers.len() - 1);
            }
        }

        self.selection = Some(new_selection);
        let layer_name = self
            .current_layer()
            .map(|layer| layer.id.as_str())
            .unwrap_or("layer");
        self.mark_edited(format!(
            "Pasted selection into '{}' at {}, {}",
            layer_name, anchor.0, anchor.1
        ));
    }

    fn is_asset_favorite(&self, kind: &str, id: &str) -> bool {
        let favorites = &self.workspace_addons.editor_favorites;
        match kind {
            "scene" => favorites.scenes.iter().any(|value| value == id),
            "palette" => favorites.palettes.iter().any(|value| value == id),
            "tileset" => favorites.tilesets.iter().any(|value| value == id),
            "metasprite" => favorites.metasprites.iter().any(|value| value == id),
            "animation" => favorites.animations.iter().any(|value| value == id),
            "dialogue" => favorites.dialogues.iter().any(|value| value == id),
            "sprite_source" => favorites.sprite_sources.iter().any(|value| value == id),
            _ => false,
        }
    }

    fn toggle_asset_favorite(&mut self, kind: &str, id: &str) {
        let values = match kind {
            "scene" => &mut self.workspace_addons.editor_favorites.scenes,
            "palette" => &mut self.workspace_addons.editor_favorites.palettes,
            "tileset" => &mut self.workspace_addons.editor_favorites.tilesets,
            "metasprite" => &mut self.workspace_addons.editor_favorites.metasprites,
            "animation" => &mut self.workspace_addons.editor_favorites.animations,
            "dialogue" => &mut self.workspace_addons.editor_favorites.dialogues,
            "sprite_source" => &mut self.workspace_addons.editor_favorites.sprite_sources,
            _ => return,
        };

        if let Some(index) = values.iter().position(|value| value == id) {
            values.remove(index);
        } else {
            values.push(id.to_string());
        }
        self.persist_workspace_addons();
    }

    fn next_snippet_name(&self, prefix: &str) -> String {
        let used = self
            .workspace_addons
            .scene_library
            .snippets
            .iter()
            .map(|snippet| snippet.name.clone())
            .chain(
                self.workspace_addons
                    .scene_library
                    .brushes
                    .iter()
                    .map(|brush| brush.name.clone()),
            )
            .collect::<BTreeSet<_>>();
        next_unique_layer_id(&used, prefix)
    }

    fn save_selection_as_brush(&mut self) {
        let Some(selection) = &self.selection else {
            self.status = "Select a region before saving a brush.".to_string();
            return;
        };
        let Some(scene) = self.current_scene() else {
            return;
        };
        let Some(layer) = self.current_layer() else {
            return;
        };

        let width = selection.rect.width_tiles();
        let height = selection.rect.height_tiles();
        let scene_width = scene.size_tiles.width as usize;
        let mut brush = SavedTileBrush {
            name: self.next_snippet_name("terrain_brush"),
            size_tiles: snesmaker_project::GridSize {
                width: width as u16,
                height: height as u16,
            },
            ..SavedTileBrush::default()
        };

        for tile_y in selection.rect.min_y..=selection.rect.max_y {
            for tile_x in selection.rect.min_x..=selection.rect.max_x {
                let cell_index = tile_y * scene_width + tile_x;
                brush
                    .tiles
                    .push(layer.tiles.get(cell_index).copied().unwrap_or_default());
                brush
                    .solids
                    .push(scene.collision.solids.get(cell_index).copied().unwrap_or(false));
                brush
                    .ladders
                    .push(scene.collision.ladders.get(cell_index).copied().unwrap_or(false));
                brush
                    .hazards
                    .push(scene.collision.hazards.get(cell_index).copied().unwrap_or(false));
            }
        }

        let label = brush.name.clone();
        self.workspace_addons.scene_library.brushes.push(brush);
        self.workspace_addons.scene_library.normalize();
        self.persist_workspace_addons();
        self.status = format!("Saved brush '{}'", label);
    }

    fn save_selection_as_snippet(&mut self) {
        let Some(selection) = &self.selection else {
            self.status = "Select a region before saving a snippet.".to_string();
            return;
        };
        let Some(scene) = self.current_scene() else {
            return;
        };

        let width = selection.rect.width_tiles();
        let height = selection.rect.height_tiles();
        let scene_width = scene.size_tiles.width as usize;
        let origin = selection.rect.origin_pixels();
        let mut snippet = SavedSceneSnippet {
            name: self.next_snippet_name("scene_snippet"),
            source_scene_id: Some(scene.id.clone()),
            scene_kind: scene.kind,
            size_tiles: snesmaker_project::GridSize {
                width: width as u16,
                height: height as u16,
            },
            ..SavedSceneSnippet::default()
        };

        for tile_y in selection.rect.min_y..=selection.rect.max_y {
            for tile_x in selection.rect.min_x..=selection.rect.max_x {
                let cell_index = tile_y * scene_width + tile_x;
                snippet
                    .collision
                    .solids
                    .push(scene.collision.solids.get(cell_index).copied().unwrap_or(false));
                snippet
                    .collision
                    .ladders
                    .push(scene.collision.ladders.get(cell_index).copied().unwrap_or(false));
                snippet
                    .collision
                    .hazards
                    .push(scene.collision.hazards.get(cell_index).copied().unwrap_or(false));
            }
        }

        for layer in &scene.layers {
            let mut snippet_layer = TileLayer {
                id: layer.id.clone(),
                tileset_id: layer.tileset_id.clone(),
                visible: layer.visible,
                parallax_x: layer.parallax_x,
                parallax_y: layer.parallax_y,
                tiles: Vec::with_capacity(width * height),
            };
            for tile_y in selection.rect.min_y..=selection.rect.max_y {
                for tile_x in selection.rect.min_x..=selection.rect.max_x {
                    let cell_index = tile_y * scene_width + tile_x;
                    snippet_layer
                        .tiles
                        .push(layer.tiles.get(cell_index).copied().unwrap_or_default());
                }
            }
            snippet.layers.push(snippet_layer);
        }

        snippet.spawns = selection
            .spawns
            .iter()
            .filter_map(|index| scene.spawns.get(*index))
            .cloned()
            .map(|mut spawn| {
                spawn.position.x -= origin.x;
                spawn.position.y -= origin.y;
                spawn
            })
            .collect();
        snippet.checkpoints = selection
            .checkpoints
            .iter()
            .filter_map(|index| scene.checkpoints.get(*index))
            .cloned()
            .map(|mut checkpoint| {
                checkpoint.position.x -= origin.x;
                checkpoint.position.y -= origin.y;
                checkpoint
            })
            .collect();
        snippet.entities = selection
            .entities
            .iter()
            .filter_map(|index| scene.entities.get(*index))
            .cloned()
            .map(|mut entity| {
                entity.position.x -= origin.x;
                entity.position.y -= origin.y;
                entity
            })
            .collect();
        snippet.triggers = selection
            .triggers
            .iter()
            .filter_map(|index| scene.triggers.get(*index))
            .cloned()
            .map(|mut trigger| {
                trigger.rect.x -= origin.x;
                trigger.rect.y -= origin.y;
                trigger
            })
            .collect();

        let label = snippet.name.clone();
        self.workspace_addons.scene_library.snippets.push(snippet);
        self.workspace_addons.scene_library.normalize();
        self.persist_workspace_addons();
        self.status = format!("Saved scene snippet '{}'", label);
    }

    fn load_brush_into_clipboard(&mut self, name: &str) {
        let Some(brush) = self
            .workspace_addons
            .scene_library
            .brushes
            .iter()
            .find(|brush| brush.name.eq_ignore_ascii_case(name))
            .cloned()
        else {
            return;
        };
        self.clipboard = Some(SceneClipboard {
            width_tiles: brush.size_tiles.width as usize,
            height_tiles: brush.size_tiles.height as usize,
            tiles: brush.tiles,
            solids: brush.solids,
            ladders: brush.ladders,
            hazards: brush.hazards,
            spawns: Vec::new(),
            checkpoints: Vec::new(),
            entities: Vec::new(),
            triggers: Vec::new(),
        });
        self.status = format!("Loaded brush '{}' into the clipboard", brush.name);
    }

    fn load_snippet_into_clipboard(&mut self, name: &str) {
        let Some(snippet) = self
            .workspace_addons
            .scene_library
            .snippets
            .iter()
            .find(|snippet| snippet.name.eq_ignore_ascii_case(name))
            .cloned()
        else {
            return;
        };
        let tiles = snippet
            .layers
            .first()
            .map(|layer| layer.tiles.clone())
            .unwrap_or_default();
        self.clipboard = Some(SceneClipboard {
            width_tiles: snippet.size_tiles.width as usize,
            height_tiles: snippet.size_tiles.height as usize,
            tiles,
            solids: snippet.collision.solids,
            ladders: snippet.collision.ladders,
            hazards: snippet.collision.hazards,
            spawns: snippet.spawns,
            checkpoints: snippet.checkpoints,
            entities: snippet.entities,
            triggers: snippet.triggers,
        });
        self.status = format!(
            "Loaded scene snippet '{}' into the clipboard",
            snippet.name
        );
    }

    fn sample_tile_from_cell(&mut self, cell_index: usize) {
        let Some(layer) = self.current_layer() else {
            self.status = "No active layer loaded.".to_string();
            return;
        };
        let layer_id = layer.id.clone();
        let tile_index = layer.tiles.get(cell_index).copied().unwrap_or_default() as usize;
        self.selected_tile = tile_index;
        self.tool = EditorTool::Paint;
        self.preview_focus = PreviewFocus::None;
        self.status = format!("Sampled tile {} from '{}'", tile_index, layer_id);
    }

    fn apply_selection_action(&mut self, action: SelectionAction) {
        let Some(selection) = self.selection.as_ref() else {
            self.status = "Select a region first.".to_string();
            return;
        };
        if matches!(action, SelectionAction::PaintTile(_)) && self.active_layer_locked() {
            self.status = self
                .current_layer()
                .map(|layer| format!("Layer '{}' is locked.", layer.id))
                .unwrap_or_else(|| "Active layer is locked.".to_string());
            return;
        }

        let rect = selection.rect;
        let width_tiles = rect.width_tiles();
        let height_tiles = rect.height_tiles();
        let selected_layer = self.selected_layer;

        self.capture_history();
        let mut edited = false;
        if let Some(scene) = self.current_scene_mut() {
            let scene_width = scene.size_tiles.width as usize;
            let layer_index = selected_layer.min(scene.layers.len().saturating_sub(1));
            for tile_y in rect.min_y..=rect.max_y {
                for tile_x in rect.min_x..=rect.max_x {
                    let cell_index = tile_y * scene_width + tile_x;
                    match action {
                        SelectionAction::PaintTile(tile_index) => {
                            if let Some(layer) = scene.layers.get_mut(layer_index) {
                                if cell_index < layer.tiles.len() {
                                    layer.tiles[cell_index] = tile_index;
                                    edited = true;
                                }
                            }
                        }
                        SelectionAction::SetSolid(value) => {
                            if cell_index < scene.collision.solids.len() {
                                scene.collision.solids[cell_index] = value;
                                edited = true;
                            }
                        }
                        SelectionAction::SetLadder(value) => {
                            if cell_index < scene.collision.ladders.len() {
                                scene.collision.ladders[cell_index] = value;
                                edited = true;
                            }
                        }
                        SelectionAction::SetHazard(value) => {
                            if cell_index < scene.collision.hazards.len() {
                                scene.collision.hazards[cell_index] = value;
                                edited = true;
                            }
                        }
                    }
                }
            }
        }

        if edited {
            let status = match action {
                SelectionAction::PaintTile(0) => {
                    format!(
                        "Cleared tiles in {}x{} selection",
                        width_tiles, height_tiles
                    )
                }
                SelectionAction::PaintTile(tile_index) => format!(
                    "Filled {}x{} selection with tile {}",
                    width_tiles, height_tiles, tile_index
                ),
                SelectionAction::SetSolid(true) => {
                    format!("Marked {}x{} selection as solid", width_tiles, height_tiles)
                }
                SelectionAction::SetSolid(false) => {
                    format!(
                        "Cleared solid collision in {}x{} selection",
                        width_tiles, height_tiles
                    )
                }
                SelectionAction::SetLadder(true) => {
                    format!(
                        "Marked {}x{} selection as ladder",
                        width_tiles, height_tiles
                    )
                }
                SelectionAction::SetLadder(false) => format!(
                    "Cleared ladder collision in {}x{} selection",
                    width_tiles, height_tiles
                ),
                SelectionAction::SetHazard(true) => {
                    format!(
                        "Marked {}x{} selection as hazard",
                        width_tiles, height_tiles
                    )
                }
                SelectionAction::SetHazard(false) => format!(
                    "Cleared hazard collision in {}x{} selection",
                    width_tiles, height_tiles
                ),
            };
            self.mark_edited(status);
        }
    }

    fn draw_line_in_selection(&mut self) {
        let Some(selection_rect) = self.selection.as_ref().map(|selection| selection.rect) else {
            self.status = "Select a region first.".to_string();
            return;
        };
        if self.active_layer_locked() {
            self.status = "Active layer is locked.".to_string();
            return;
        }

        let start = (selection_rect.min_x as i32, selection_rect.min_y as i32);
        let end = (selection_rect.max_x as i32, selection_rect.max_y as i32);
        let points = bresenham_line(start, end);
        let selected_layer = self.selected_layer;
        let selected_tile = self.selected_tile as u16;
        self.capture_history();
        if let Some(scene) = self.current_scene_mut() {
            let width = scene.size_tiles.width as usize;
            if let Some(layer) = scene.layers.get_mut(selected_layer) {
                for (tile_x, tile_y) in points {
                    let index = tile_y as usize * width + tile_x as usize;
                    if let Some(tile) = layer.tiles.get_mut(index) {
                        *tile = selected_tile;
                    }
                }
            }
        }
        self.mark_edited("Drew a line across the current selection");
    }

    fn mirror_selection(&mut self, horizontal: bool) {
        let Some(selection_rect) = self.selection.as_ref().map(|selection| selection.rect) else {
            self.status = "Select a region first.".to_string();
            return;
        };
        if self.active_layer_locked() {
            self.status = "Active layer is locked.".to_string();
            return;
        }

        let selected_layer = self.selected_layer;
        self.capture_history();
        if let Some(scene) = self.current_scene_mut() {
            let width = scene.size_tiles.width as usize;
            let layer_index = selected_layer.min(scene.layers.len().saturating_sub(1));
            if let Some(layer) = scene.layers.get_mut(layer_index) {
                let original_tiles = layer.tiles.clone();
                let original_solids = scene.collision.solids.clone();
                let original_ladders = scene.collision.ladders.clone();
                let original_hazards = scene.collision.hazards.clone();

                for tile_y in selection_rect.min_y..=selection_rect.max_y {
                    for tile_x in selection_rect.min_x..=selection_rect.max_x {
                        let mirror_x = if horizontal {
                            selection_rect.max_x - (tile_x - selection_rect.min_x)
                        } else {
                            tile_x
                        };
                        let mirror_y = if horizontal {
                            tile_y
                        } else {
                            selection_rect.max_y - (tile_y - selection_rect.min_y)
                        };
                        let source_index = mirror_y * width + mirror_x;
                        let target_index = tile_y * width + tile_x;
                        layer.tiles[target_index] = original_tiles[source_index];
                        scene.collision.solids[target_index] = original_solids[source_index];
                        scene.collision.ladders[target_index] = original_ladders[source_index];
                        scene.collision.hazards[target_index] = original_hazards[source_index];
                    }
                }
            }
        }
        self.mark_edited(if horizontal {
            "Mirrored the current selection horizontally"
        } else {
            "Mirrored the current selection vertically"
        });
    }

    fn flood_fill_from_hovered_tile(&mut self) {
        let Some((start_x, start_y)) = self.last_canvas_tile else {
            self.status = "Hover or click a tile before using flood fill.".to_string();
            return;
        };
        if self.active_layer_locked() {
            self.status = "Active layer is locked.".to_string();
            return;
        }

        let selected_layer = self.selected_layer;
        let replacement = self.selected_tile as u16;
        self.capture_history();
        if let Some(scene) = self.current_scene_mut() {
            let width = scene.size_tiles.width as usize;
            let height = scene.size_tiles.height as usize;
            let layer_index = selected_layer.min(scene.layers.len().saturating_sub(1));
            if let Some(layer) = scene.layers.get_mut(layer_index) {
                let target_index = start_y * width + start_x;
                let Some(source_tile) = layer.tiles.get(target_index).copied() else {
                    return;
                };
                if source_tile == replacement {
                    self.status = "Flood fill skipped because the target tile already matches."
                        .to_string();
                    return;
                }

                let mut queue = vec![(start_x, start_y)];
                let mut visited = BTreeSet::new();
                while let Some((tile_x, tile_y)) = queue.pop() {
                    if tile_x >= width || tile_y >= height || !visited.insert((tile_x, tile_y)) {
                        continue;
                    }
                    let index = tile_y * width + tile_x;
                    if layer.tiles.get(index).copied().unwrap_or_default() != source_tile {
                        continue;
                    }
                    layer.tiles[index] = replacement;
                    if tile_x > 0 {
                        queue.push((tile_x - 1, tile_y));
                    }
                    if tile_x + 1 < width {
                        queue.push((tile_x + 1, tile_y));
                    }
                    if tile_y > 0 {
                        queue.push((tile_x, tile_y - 1));
                    }
                    if tile_y + 1 < height {
                        queue.push((tile_x, tile_y + 1));
                    }
                }
            }
        }
        self.mark_edited("Flood-filled the current layer");
    }

    fn project_sprite_source_dir(&self) -> Utf8PathBuf {
        self.project_root.join(PROJECT_SPRITE_SOURCE_DIR)
    }

    fn project_sprite_source_relative_path(&self, path: &Utf8Path) -> String {
        path.strip_prefix(&self.project_root)
            .map(|relative| relative.as_str().to_string())
            .unwrap_or_else(|_| path.as_str().to_string())
    }

    fn list_project_sprite_sources(&self) -> Result<Vec<Utf8PathBuf>> {
        let directory = self.project_sprite_source_dir();
        if !directory.exists() {
            return Ok(Vec::new());
        }

        let mut files = fs::read_dir(&directory)
            .with_context(|| format!("failed to read sprite source directory {}", directory))?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = Utf8PathBuf::from_path_buf(entry.path()).ok()?;
                let is_png = path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("png"));
                is_png.then_some(path)
            })
            .collect::<Vec<_>>();
        files.sort();
        Ok(files)
    }

    fn available_visual_ids(&self) -> Vec<String> {
        let Some(bundle) = &self.bundle else {
            return Vec::new();
        };

        let mut ids = bundle
            .animations
            .iter()
            .map(|animation| animation.id.clone())
            .chain(
                bundle
                    .metasprites
                    .iter()
                    .map(|metasprite| metasprite.id.clone()),
            )
            .collect::<Vec<_>>();
        ids.sort();
        ids.dedup();
        ids
    }

    fn asset_usage_summary(&self, kind: &str, id: &str) -> String {
        let Some(bundle) = &self.bundle else {
            return "No project loaded.".to_string();
        };

        match kind {
            "scene" => {
                let entry = (bundle.manifest.gameplay.entry_scene == id)
                    .then_some("entry scene")
                    .into_iter()
                    .collect::<Vec<_>>();
                let load_refs = bundle
                    .scenes
                    .iter()
                    .flat_map(|scene| scene.scripts.iter())
                    .flat_map(|script| script.commands.iter())
                    .filter(|command| {
                        matches!(
                            command,
                            snesmaker_events::EventCommand::LoadScene { scene_id, .. } if scene_id == id
                        )
                    })
                    .count();
                let mut parts = entry.into_iter().map(str::to_string).collect::<Vec<_>>();
                if load_refs > 0 {
                    parts.push(format!("loaded by {} script command(s)", load_refs));
                }
                if parts.is_empty() {
                    "No inbound references".to_string()
                } else {
                    parts.join(" | ")
                }
            }
            "palette" => {
                let tilesets = bundle
                    .tilesets
                    .iter()
                    .filter(|tileset| tileset.palette_id == id)
                    .count();
                let metasprites = bundle
                    .metasprites
                    .iter()
                    .filter(|metasprite| metasprite.palette_id == id)
                    .count();
                format!("{} tileset(s) | {} metasprite(s)", tilesets, metasprites)
            }
            "tileset" => {
                let layers = bundle
                    .scenes
                    .iter()
                    .flat_map(|scene| scene.layers.iter())
                    .filter(|layer| layer.tileset_id == id)
                    .count();
                format!("Used by {} layer(s)", layers)
            }
            "metasprite" => {
                let animations = bundle
                    .animations
                    .iter()
                    .filter(|animation| animation.frames.iter().any(|frame| frame.metasprite_id == id))
                    .count();
                let entities = bundle
                    .scenes
                    .iter()
                    .flat_map(|scene| scene.entities.iter())
                    .filter(|entity| entity.archetype == id)
                    .count();
                format!("{} animation(s) | {} entity placement(s)", animations, entities)
            }
            "animation" => {
                let entities = bundle
                    .scenes
                    .iter()
                    .flat_map(|scene| scene.entities.iter())
                    .filter(|entity| entity.archetype == id)
                    .count();
                format!("Used by {} entity placement(s)", entities)
            }
            "dialogue" => {
                let scripts = bundle
                    .scenes
                    .iter()
                    .flat_map(|scene| scene.scripts.iter())
                    .filter(|script| {
                        script.commands.iter().any(|command| {
                            matches!(
                                command,
                                snesmaker_events::EventCommand::ShowDialogue { dialogue_id, .. }
                                    if dialogue_id == id
                            )
                        })
                    })
                    .count();
                format!("Referenced by {} script(s)", scripts)
            }
            _ => "No usage metadata".to_string(),
        }
    }

    fn copy_project_sprite_sources_to(&self, export_root: &Utf8Path) -> Result<()> {
        let source_dir = self.project_sprite_source_dir();
        let export_dir = export_root.join(PROJECT_SPRITE_SOURCE_DIR);
        fs::create_dir_all(&export_dir)
            .with_context(|| format!("failed to create {}", export_dir))?;

        if !source_dir.exists() {
            return Ok(());
        }

        for path in self.list_project_sprite_sources()? {
            let Some(file_name) = path.file_name() else {
                continue;
            };
            fs::copy(&path, export_dir.join(file_name)).with_context(|| {
                format!("failed to copy sprite source {} into {}", path, export_dir)
            })?;
        }

        Ok(())
    }

    fn seed_import_ids_from_path(&mut self, path: &Path) {
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(slugify)
            .unwrap_or_else(|| "imported_sprite".to_string());
        let should_seed_base =
            self.import_state.base_id.is_empty() || self.import_state.base_id == "player_run";
        let should_seed_animation = self.import_state.animation_id.is_empty()
            || self.import_state.animation_id == "player_run";

        if should_seed_base {
            self.import_state.base_id = stem.clone();
        }
        if should_seed_animation {
            self.import_state.animation_id = stem;
        }
    }

    fn load_import_preview_from_path(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
        display_path: String,
    ) {
        match load_sheet_preview(ctx, path) {
            Ok(preview) => {
                self.seed_import_ids_from_path(path);
                self.import_state.source_path = display_path;
                self.import_state.preview = Some(preview);
                self.import_state.status = "Loaded sprite sheet.".to_string();
            }
            Err(error) => {
                self.import_state.status = error.to_string();
            }
        }
    }

    fn save_project(&mut self) {
        let Some(bundle) = &self.bundle else {
            self.status = "No project loaded.".to_string();
            return;
        };

        match bundle.save(&self.project_root) {
            Ok(()) => {
                self.dirty = false;
                self.status = format!("Saved project to {}", self.project_root);
            }
            Err(error) => {
                self.status = error.to_string();
            }
        }
    }

    fn export_project(&mut self) {
        let Some(bundle) = &self.bundle else {
            self.status = "No project loaded.".to_string();
            return;
        };

        let Some(path) = FileDialog::new()
            .set_title("Export project copy")
            .pick_folder()
        else {
            return;
        };

        let Ok(export_root) = Utf8PathBuf::from_path_buf(path) else {
            self.status = "Export path must be utf-8.".to_string();
            return;
        };

        match bundle
            .save(&export_root)
            .and_then(|_| self.copy_project_sprite_sources_to(&export_root))
            .and_then(|_| copy_workspace_file(&self.project_root, &export_root))
        {
            Ok(()) => self.status = format!("Exported project to {}", export_root),
            Err(error) => self.status = error.to_string(),
        }
    }

    fn build_report_path(&self) -> Utf8PathBuf {
        if let Some(bundle) = &self.bundle {
            self.project_root
                .join(&bundle.manifest.build.output_dir)
                .join("build-report.json")
        } else {
            self.project_root.join("build/build-report.json")
        }
    }

    fn refresh_last_build_report(&mut self) {
        let path = self.build_report_path();
        if !path.exists() {
            self.last_build_outcome = None;
            return;
        }

        match fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path))
            .and_then(|text| {
                serde_json::from_str::<BuildOutcome>(&text)
                    .with_context(|| format!("failed to parse {}", path))
            })
        {
            Ok(outcome) => {
                self.last_build_outcome = Some(outcome);
            }
            Err(error) => {
                self.last_build_outcome = None;
                self.status = format!("Failed to load build report: {}", error);
            }
        }
    }

    fn save_before_build(&mut self) -> bool {
        if self.bundle.is_none() {
            self.status = "No project loaded.".to_string();
            return false;
        }

        if self.dirty {
            self.save_project();
            if self.dirty {
                return false;
            }
        }

        true
    }

    fn build_current_rom(&mut self) {
        if !self.save_before_build() {
            return;
        }

        match build_rom(&self.project_root, None) {
            Ok(outcome) => {
                self.last_build_outcome = Some(outcome.clone());
                self.report = outcome.validation;
                self.status = if outcome.rom_built {
                    format!("Built ROM at {}", outcome.rom_path)
                } else {
                    format!("Generated build assets at {}", outcome.build_dir)
                };
            }
            Err(error) => {
                self.refresh_last_build_report();
                self.status = error.to_string();
            }
        }
    }

    fn build_and_launch_playtest(&mut self) {
        if !self.save_before_build() {
            return;
        }

        let emulator = self
            .bundle
            .as_ref()
            .and_then(|bundle| bundle.manifest.editor.preferred_emulator.clone())
            .unwrap_or_else(|| "ares".to_string());
        if self.bundle.is_none() {
            self.status = "No project loaded.".to_string();
            return;
        }

        match build_rom(&self.project_root, None) {
            Ok(outcome) => {
                self.last_build_outcome = Some(outcome.clone());
                self.report = outcome.validation.clone();
                if !outcome.rom_built {
                    let message = format!(
                        "Generated build assets at {}. Configure ca65/ld65 to enable playtest launches.",
                        outcome.build_dir
                    );
                    self.playtest_state.last_status = message.clone();
                    self.status = message;
                    return;
                }

                match Command::new(&emulator).arg(&outcome.rom_path).spawn() {
                    Ok(_) => {
                        let message = format!(
                            "Launched '{}' with {}",
                            emulator, outcome.rom_path
                        );
                        self.playtest_state.last_status = message.clone();
                        self.status = message;
                    }
                    Err(error) => {
                        let message =
                            format!("Failed to launch emulator '{}': {}", emulator, error);
                        self.playtest_state.last_status = message.clone();
                        self.status = message;
                    }
                }
            }
            Err(error) => {
                self.refresh_last_build_report();
                let message = error.to_string();
                self.playtest_state.last_status = message.clone();
                self.status = message;
            }
        }
    }

    fn selected_physics_profile(&self) -> Option<PhysicsProfile> {
        let bundle = self.bundle.as_ref()?;
        if self.playtest_state.selected_physics_id.is_empty() {
            return bundle.manifest.gameplay.physics_presets.first().cloned();
        }
        bundle
            .manifest
            .gameplay
            .physics_presets
            .iter()
            .find(|preset| preset.id == self.playtest_state.selected_physics_id)
            .cloned()
            .or_else(|| bundle.manifest.gameplay.physics_presets.first().cloned())
    }

    fn reset_playtest_session(&mut self) {
        let Some(scene) = self.current_scene().cloned() else {
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
                if let Some(index) = self.selected_spawn.and_then(|index| scene.spawns.get(index)) {
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
                    self.playtest_state.last_status = format!(
                        "Started '{}' from checkpoint '{}'",
                        scene.id, index.id
                    );
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

    fn step_playtest_session(&mut self, input: InputFrame) {
        if self.playtest_state.session.is_none() {
            self.reset_playtest_session();
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

    fn undo(&mut self) {
        let undone = if let Some(bundle) = &mut self.bundle {
            self.history.undo(bundle)
        } else {
            false
        };
        if undone {
            self.dirty = true;
            self.active_canvas_cell = None;
            self.clear_selection();
            self.last_canvas_tile = None;
            self.status = "Undo".to_string();
            self.refresh_report();
        }
    }

    fn redo(&mut self) {
        let redone = if let Some(bundle) = &mut self.bundle {
            self.history.redo(bundle)
        } else {
            false
        };
        if redone {
            self.dirty = true;
            self.active_canvas_cell = None;
            self.clear_selection();
            self.last_canvas_tile = None;
            self.status = "Redo".to_string();
            self.refresh_report();
        }
    }

    fn open_project_dialog(&mut self) {
        let Some(path) = FileDialog::new().set_title("Open project").pick_folder() else {
            return;
        };
        let Ok(root) = Utf8PathBuf::from_path_buf(path) else {
            self.status = "Project path must be utf-8.".to_string();
            return;
        };
        self.open_project(root);
    }

    fn choose_new_project_destination(&mut self) {
        if let Some(path) = FileDialog::new()
            .set_title("Choose project folder")
            .pick_folder()
        {
            self.new_project_state.destination = path.display().to_string();
        }
    }

    fn create_template_project(&mut self) {
        let project_name = self.new_project_state.project_name.trim();
        if project_name.is_empty() {
            self.status = "Enter a project name first.".to_string();
            return;
        }
        if self.new_project_state.destination.trim().is_empty() {
            self.status = "Choose a destination folder first.".to_string();
            return;
        }

        let root_path =
            PathBuf::from(self.new_project_state.destination.trim()).join(slugify(project_name));
        let Ok(root) = Utf8PathBuf::from_path_buf(root_path) else {
            self.status = "Project destination must be utf-8.".to_string();
            return;
        };

        match ProjectBundle::write_template_project(&root, project_name) {
            Ok(()) => {
                self.new_project_state.open = false;
                self.open_project(root);
            }
            Err(error) => {
                self.status = error.to_string();
            }
        }
    }

    fn select_import_file(&mut self, ctx: &egui::Context) {
        let Some(path) = FileDialog::new()
            .add_filter("PNG image", &["png"])
            .set_title("Import sprite sheet")
            .pick_file()
        else {
            return;
        };

        self.load_import_preview_from_path(ctx, &path, path.display().to_string());
    }

    fn import_sprite_source_into_project(&mut self, ctx: &egui::Context) {
        let Some(source_path) = FileDialog::new()
            .add_filter("PNG image", &["png"])
            .set_title("Copy sprite sheet into project")
            .pick_file()
        else {
            return;
        };

        let source_directory = self.project_sprite_source_dir();
        if let Err(error) = fs::create_dir_all(&source_directory) {
            self.import_state.status = format!(
                "failed to create project sprite directory {}: {}",
                source_directory, error
            );
            return;
        }

        let destination = unique_project_sprite_destination(&source_directory, &source_path);
        if let Err(error) = fs::copy(&source_path, &destination) {
            self.import_state.status = format!(
                "failed to copy sprite sheet {} into {}: {}",
                source_path.display(),
                destination,
                error
            );
            return;
        }

        let display_path = self.project_sprite_source_relative_path(&destination);
        self.load_import_preview_from_path(ctx, destination.as_std_path(), display_path);
    }

    fn import_sprite_sheet(&mut self) {
        let Some(bundle) = &self.bundle else {
            self.import_state.status = "No project loaded.".to_string();
            return;
        };
        let Some(preview) = &self.import_state.preview else {
            self.import_state.status = "Choose a PNG sprite sheet first.".to_string();
            return;
        };

        let mut edited_bundle = bundle.clone();
        let result = import_sprite_sheet_into_bundle(
            &mut edited_bundle,
            &self.import_state,
            &preview.rgba,
            preview.size,
        );

        match result {
            Ok(summary) => {
                self.capture_history();
                self.bundle = Some(edited_bundle);
                self.mark_edited(summary);
                self.import_state.status = "Imported sprite sheet.".to_string();
                self.sync_selection();
            }
            Err(error) => self.import_state.status = error.to_string(),
        }
    }

    fn run_shortcuts(&mut self, ctx: &egui::Context) {
        let save = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
        let open = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
        let build = KeyboardShortcut::new(Modifiers::COMMAND, Key::B);
        let reload = KeyboardShortcut::new(Modifiers::COMMAND, Key::R);
        let undo = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
        let redo_shift = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
        let redo = KeyboardShortcut::new(Modifiers::COMMAND, Key::Y);
        let import = KeyboardShortcut::new(Modifiers::COMMAND, Key::I);
        let copy = KeyboardShortcut::new(Modifiers::COMMAND, Key::C);
        let paste = KeyboardShortcut::new(Modifiers::COMMAND, Key::V);
        let fill_selection = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::F);
        let escape = KeyboardShortcut::new(Modifiers::NONE, Key::Escape);

        if ctx.input_mut(|input| input.consume_shortcut(&save)) {
            self.save_project();
        }
        if ctx.input_mut(|input| input.consume_shortcut(&open)) {
            self.open_project_dialog();
        }
        if ctx.input_mut(|input| input.consume_shortcut(&build)) {
            self.build_current_rom();
        }
        if ctx.input_mut(|input| input.consume_shortcut(&reload)) {
            self.reload();
        }
        if ctx.input_mut(|input| input.consume_shortcut(&redo_shift))
            || ctx.input_mut(|input| input.consume_shortcut(&redo))
        {
            self.redo();
        }
        if ctx.input_mut(|input| input.consume_shortcut(&undo)) {
            self.undo();
        }
        if ctx.input_mut(|input| input.consume_shortcut(&import)) {
            self.import_state.open = true;
        }
        if ctx.input_mut(|input| input.consume_shortcut(&copy)) {
            self.copy_selection_to_clipboard();
        }
        if ctx.input_mut(|input| input.consume_shortcut(&paste)) {
            self.paste_clipboard();
        }
        if ctx.input_mut(|input| input.consume_shortcut(&fill_selection)) {
            self.apply_selection_action(SelectionAction::PaintTile(self.selected_tile as u16));
        }
        if ctx.input_mut(|input| input.consume_shortcut(&escape))
            && (self.selection.is_some() || self.selection_drag_anchor.is_some())
        {
            self.clear_selection();
            self.status = "Selection cleared".to_string();
        }
    }

    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if ctx.input(|input| input.viewport().close_requested()) && self.dirty {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.confirm_exit = true;
        }
    }

    fn draw_menu_bar(&mut self, ctx: &egui::Context) {
        let mut next_workspace_preset = None;
        let mut next_saved_layout = None;
        let mut delete_saved_layout = None;
        let mut open_save_dialog = false;
        let mut workspace_changed = false;
        let mut pending_tab_visibility = Vec::new();

        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New Template Project").clicked() {
                        self.new_project_state.open = true;
                        ui.close();
                    }
                    if ui.button("Open Project...").clicked() {
                        self.open_project_dialog();
                        ui.close();
                    }
                    if ui.button("Save").clicked() {
                        self.save_project();
                        ui.close();
                    }
                    if ui.button("Export Project Copy...").clicked() {
                        self.export_project();
                        ui.close();
                    }
                    if ui.button("Import Sprite Sheet...").clicked() {
                        self.import_state.open = true;
                        ui.close();
                    }
                    if ui.button("Reload from Disk").clicked() {
                        self.reload();
                        ui.close();
                    }
                    if ui.button("Exit").clicked() {
                        if self.dirty {
                            self.confirm_exit = true;
                        } else {
                            ctx.send_viewport_cmd(ViewportCommand::Close);
                        }
                        ui.close();
                    }
                });

                ui.menu_button("Edit", |ui| {
                    if ui.button("Undo").clicked() {
                        self.undo();
                        ui.close();
                    }
                    if ui.button("Redo").clicked() {
                        self.redo();
                        ui.close();
                    }
                    if ui.button("Fill Selection With Tile").clicked() {
                        self.apply_selection_action(SelectionAction::PaintTile(
                            self.selected_tile as u16,
                        ));
                        ui.close();
                    }
                });

                ui.menu_button("Build", |ui| {
                    if ui.button("Validate").clicked() {
                        self.refresh_report();
                        self.status = format!(
                            "Validated: {} error(s), {} warning(s)",
                            self.report.errors.len(),
                            self.report.warnings.len()
                        );
                        ui.close();
                    }
                    if ui.button("Build ROM").clicked() {
                        self.build_current_rom();
                        ui.close();
                    }
                    if ui.button("Build && Launch Playtest").clicked() {
                        self.build_and_launch_playtest();
                        ui.close();
                    }
                });

                ui.menu_button("Workspace", |ui| {
                    for preset in [
                        WorkspacePreset::LevelDesign,
                        WorkspacePreset::Animation,
                        WorkspacePreset::Eventing,
                        WorkspacePreset::Debug,
                    ] {
                        if ui
                            .selectable_label(self.workspace_preset == preset, preset.label())
                            .clicked()
                        {
                            next_workspace_preset = Some(preset);
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button("Save Current Layout...").clicked() {
                        open_save_dialog = true;
                        ui.close();
                    }
                    ui.menu_button("Load Saved Layout", |ui| {
                        if self.workspace.saved_layouts.is_empty() {
                            ui.label("No saved layouts yet.");
                        }
                        for layout in &self.workspace.saved_layouts {
                            if ui.button(&layout.name).clicked() {
                                next_saved_layout = Some(layout.name.clone());
                                ui.close();
                            }
                        }
                    });
                    ui.menu_button("Delete Saved Layout", |ui| {
                        if self.workspace.saved_layouts.is_empty() {
                            ui.label("No saved layouts yet.");
                        }
                        for layout in &self.workspace.saved_layouts {
                            if ui.button(&layout.name).clicked() {
                                delete_saved_layout = Some(layout.name.clone());
                                ui.close();
                            }
                        }
                    });
                });

                ui.menu_button("View", |ui| {
                    ui.checkbox(&mut self.show_grid, "Show Grid");
                    ui.checkbox(&mut self.show_collision, "Show Collision Overlay");
                    ui.add(
                        egui::Slider::new(&mut self.scene_zoom, SCENE_MIN_ZOOM..=SCENE_MAX_ZOOM)
                            .text("Scene Zoom"),
                    );
                    ui.separator();
                    ui.label("Workspace");
                    workspace_changed |= ui
                        .checkbox(&mut self.workspace.layout.show_status_bar, "Show Status Bar")
                        .changed();
                    ui.separator();
                    ui.label("Dock Tabs");
                    for tab in DockTab::ALL {
                        let mut visible = self.workspace.layout.contains(tab);
                        if ui.checkbox(&mut visible, tab.label()).changed() {
                            pending_tab_visibility.push((tab, visible));
                        }
                    }
                });

                ui.menu_button("Help", |ui| {
                    if ui.button("Controls & Workflow").clicked() {
                        self.show_help = true;
                        ui.close();
                    }
                });

                ui.separator();
                ui.strong("SNES Maker");
                let active_workspace_label = self
                    .workspace
                    .active_saved_layout
                    .as_deref()
                    .unwrap_or_else(|| self.workspace_preset.label());
                ui.label(format!("Workspace: {}", active_workspace_label));
                ui.label(self.project_root.as_str());
                if self.dirty {
                    ui.colored_label(Color32::from_rgb(222, 168, 32), "Unsaved changes");
                }
            });
        });

        if let Some(preset) = next_workspace_preset {
            self.set_workspace_preset(preset);
        }
        if let Some(name) = next_saved_layout {
            self.load_saved_workspace(&name);
        }
        if let Some(name) = delete_saved_layout {
            self.delete_saved_workspace(&name);
        }
        if open_save_dialog {
            self.save_layout_state.open = true;
            if self.save_layout_state.name.trim().is_empty() {
                self.save_layout_state.name = self
                    .workspace
                    .active_saved_layout
                    .clone()
                    .unwrap_or_else(|| self.workspace_preset.label().to_string());
            }
        }
        for (tab, visible) in pending_tab_visibility {
            self.set_dock_tab_visibility(tab, visible);
        }
        if workspace_changed {
            self.mark_workspace_custom();
        }

        if self.workspace.layout.show_status_bar {
            egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(&self.status);
                    ui.separator();
                    ui.label(format!(
                        "{} error(s), {} warning(s)",
                        self.report.errors.len(),
                        self.report.warnings.len()
                    ));
                    ui.separator();
                    ui.label(format!(
                        "Budgets: {} scene(s), {} tile(s), {} color(s), ~{} bank(s)",
                        self.report.budgets.scene_count,
                        self.report.budgets.unique_tiles,
                        self.report.budgets.palette_colors,
                        self.report.budgets.estimated_rom_banks
                    ));
                });
            });
        }
    }

    fn draw_toolbox_tab(&mut self, ui: &mut egui::Ui) {
        let mut pending_scene_selection = None;
        let mut pending_layer_selection = None;
        let mut pending_layer_visibility = None;
        let mut pending_layer_lock = None;
        let mut pending_tile_selection = None;
        let Some(bundle) = &self.bundle else {
            ui.heading("Project");
            ui.label("No project loaded.");
            if ui.button("Open Project...").clicked() {
                self.open_project_dialog();
            }
            if ui.button("Create Template Project").clicked() {
                self.new_project_state.open = true;
            }
            return;
        };

        ui.heading("Scenes");
        for (index, scene) in bundle.scenes.iter().enumerate() {
            if ui
                .selectable_label(self.selected_scene == index, &scene.id)
                .clicked()
            {
                pending_scene_selection = Some(index);
            }
        }

        ui.separator();
        ui.heading("Layers");
        if let Some(scene) = bundle.scenes.get(self.selected_scene) {
            if scene.layers.is_empty() {
                ui.label("This scene has no layers.");
            } else {
                for (index, layer) in scene.layers.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(self.selected_layer == index, &layer.id)
                            .clicked()
                        {
                            pending_layer_selection = Some(index);
                        }
                        if ui
                            .small_button(if layer.visible { "Hide" } else { "Show" })
                            .clicked()
                        {
                            pending_layer_visibility = Some(index);
                        }
                        if ui
                            .small_button(
                                if self.is_layer_locked(self.selected_scene, index) {
                                    "Unlock"
                                } else {
                                    "Lock"
                                },
                            )
                            .clicked()
                        {
                            pending_layer_lock = Some(index);
                        }
                    });
                }
            }
        }

        ui.separator();
        ui.heading("Tools");
        ui.horizontal_wrapped(|ui| {
            for tool in [
                EditorTool::Select,
                EditorTool::Paint,
                EditorTool::Erase,
                EditorTool::Solid,
                EditorTool::Ladder,
                EditorTool::Hazard,
                EditorTool::Spawn,
                EditorTool::Checkpoint,
                EditorTool::Entity,
                EditorTool::Trigger,
            ] {
                ui.selectable_value(&mut self.tool, tool, tool.label());
            }
        });

        ui.separator();
        ui.heading("Tiles");
        let active_layer_label = self.current_layer().map(|active_layer| {
            format!(
                "Layer: {}{}{}",
                active_layer.id,
                if active_layer.visible { "" } else { " [hidden]" },
                if self.active_layer_locked() {
                    " [locked]"
                } else {
                    ""
                }
            )
        });
        let active_tileset = self
            .active_tileset_and_palette()
            .map(|(tileset, palette)| (tileset.name.clone(), tileset.tiles.clone(), palette.clone()));
        if let Some(label) = active_layer_label {
            ui.label(label);
        }
        if let Some((tileset_name, tiles, palette)) = active_tileset {
            ui.label(format!("Tileset: {}", tileset_name));
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (index, tile) in tiles.iter().enumerate() {
                            let response =
                                draw_tile_button(ui, tile, &palette, 2.0, self.selected_tile == index);
                            if response.clicked() {
                                pending_tile_selection = Some(index);
                            }
                        }
                    });
                });
        } else {
            ui.label("Active layer tileset or palette is missing.");
        }

        ui.separator();
        ui.heading("Animations");
        for (index, animation) in bundle.animations.iter().enumerate() {
            if ui
                .selectable_label(self.selected_animation == index, &animation.id)
                .clicked()
            {
                self.selected_animation = index;
                self.preview_focus = PreviewFocus::Animation;
            }
        }

        ui.separator();
        if ui.button("Import Sprite Sheet...").clicked() {
            self.import_state.open = true;
        }
        if let Some(index) = pending_scene_selection {
            self.selected_scene = index;
            self.clear_selection();
            self.preview_focus = PreviewFocus::None;
            self.scene_scroll_offset = Vec2::ZERO;
            self.selected_layer = 0;
            self.sync_selection();
        }
        if let Some(index) = pending_layer_selection {
            self.select_layer(self.selected_scene, index);
        }
        if let Some(index) = pending_layer_visibility {
            self.toggle_layer_visibility(self.selected_scene, index);
        }
        if let Some(index) = pending_layer_lock {
            self.toggle_layer_lock(self.selected_scene, index);
        }
        if let Some(index) = pending_tile_selection {
            self.selected_tile = index;
            self.tool = EditorTool::Paint;
            self.preview_focus = PreviewFocus::None;
        }
    }

    fn draw_inspector_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let scene_snapshot = self
            .bundle
            .as_ref()
            .and_then(|bundle| bundle.scenes.get(self.selected_scene).cloned());
        let animation_snapshot = self
            .bundle
            .as_ref()
            .and_then(|bundle| bundle.animations.get(self.selected_animation).cloned());
        let metasprite_ids = self
            .bundle
            .as_ref()
            .map(|bundle| {
                bundle
                    .metasprites
                    .iter()
                    .map(|metasprite| metasprite.id.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let tileset_ids = self
            .bundle
            .as_ref()
            .map(|bundle| {
                bundle
                    .tilesets
                    .iter()
                    .map(|tileset| tileset.id.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let Some(scene_snapshot) = scene_snapshot else {
            ui.heading("Inspector");
            ui.label("Load a project to inspect it.");
            return;
        };

        ui.heading("Inspector");
        ui.label(format!(
            "Scene: {} ({}x{} tiles)",
            scene_snapshot.id, scene_snapshot.size_tiles.width, scene_snapshot.size_tiles.height
        ));
        ui.label(format!(
            "Chunk: {}x{}  |  Scripts: {}",
            scene_snapshot.chunk_size_tiles.width,
            scene_snapshot.chunk_size_tiles.height,
            scene_snapshot.scripts.len()
        ));

        ui.separator();
        self.draw_layer_inspector(ui, &scene_snapshot, &tileset_ids);
        ui.separator();
        self.draw_project_settings(ui);
        ui.separator();
        self.draw_animation_inspector(ui, animation_snapshot.as_ref(), &metasprite_ids);
        ui.separator();
        self.draw_spawn_inspector(ui, &scene_snapshot);
        ui.separator();
        self.draw_checkpoint_inspector(ui, &scene_snapshot);
        ui.separator();
        self.draw_entity_inspector(ui, &scene_snapshot);
        if self.has_context_preview() {
            ui.separator();
            self.draw_context_preview(ui, ctx.input(|input| input.time) as f32);
        }
        ui.separator();
        self.draw_trigger_inspector(ui, &scene_snapshot);
        ui.separator();
        self.draw_tile_editor(ui);
    }

    fn draw_layer_inspector(
        &mut self,
        ui: &mut egui::Ui,
        scene_snapshot: &SceneResource,
        tileset_ids: &[String],
    ) {
        ui.collapsing("Layers", |ui| {
            ui.horizontal(|ui| {
                if ui.button("+ Layer").clicked() {
                    self.add_layer_to_current_scene();
                }
                if ui.button("- Remove").clicked() {
                    self.remove_selected_layer();
                }
                if ui.button("Move Up").clicked() {
                    self.move_selected_layer(-1);
                }
                if ui.button("Move Down").clicked() {
                    self.move_selected_layer(1);
                }
            });

            for (index, layer) in scene_snapshot.layers.iter().enumerate() {
                if ui
                    .selectable_label(
                        self.selected_layer == index,
                        format!(
                            "{}{}{}",
                            layer.id,
                            if layer.visible { "" } else { " [hidden]" },
                            if self.is_layer_locked(self.selected_scene, index) {
                                " [locked]"
                            } else {
                                ""
                            }
                        ),
                    )
                    .clicked()
                {
                    self.select_layer(self.selected_scene, index);
                }
            }

            let Some(layer_snapshot) = scene_snapshot.layers.get(self.selected_layer) else {
                ui.label("No active layer selected.");
                return;
            };

            let mut edited = layer_snapshot.clone();
            let mut changed = false;

            ui.separator();
            ui.label(format!("Editing '{}'", layer_snapshot.id));
            changed |= ui.text_edit_singleline(&mut edited.id).changed();
            changed |= ui.checkbox(&mut edited.visible, "Visible").changed();

            if tileset_ids.is_empty() {
                ui.label("No tilesets are available yet.");
            } else {
                egui::ComboBox::from_label("Tileset")
                    .selected_text(&edited.tileset_id)
                    .show_ui(ui, |ui| {
                        for tileset_id in tileset_ids {
                            changed |= ui
                                .selectable_value(
                                    &mut edited.tileset_id,
                                    tileset_id.clone(),
                                    tileset_id,
                                )
                                .changed();
                        }
                    });
            }

            ui.horizontal(|ui| {
                ui.label("Parallax X");
                changed |= ui
                    .add(egui::DragValue::new(&mut edited.parallax_x).range(0..=8))
                    .changed();
                ui.label("Parallax Y");
                changed |= ui
                    .add(egui::DragValue::new(&mut edited.parallax_y).range(0..=8))
                    .changed();
            });
            ui.label(format!("Tile count: {}", edited.tiles.len()));

            ui.horizontal(|ui| {
                if ui
                    .button(if self.active_layer_locked() {
                        "Unlock Active Layer"
                    } else {
                        "Lock Active Layer"
                    })
                    .clicked()
                {
                    self.toggle_layer_lock(self.selected_scene, self.selected_layer);
                }
                if ui
                    .button(if edited.visible {
                        "Hide Active Layer"
                    } else {
                        "Show Active Layer"
                    })
                    .clicked()
                {
                    self.toggle_layer_visibility(self.selected_scene, self.selected_layer);
                }
            });

            if changed {
                let layer_id = edited.id.clone();
                self.capture_history();
                if let Some(target) = self.current_layer_mut() {
                    *target = edited;
                }
                self.sync_selection();
                self.mark_edited(format!("Updated layer '{}'", layer_id));
            }
        });
    }

    fn draw_project_settings(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("Player & HUD", |ui| {
            let Some(bundle) = &self.bundle else {
                return;
            };

            let mut player = bundle.manifest.gameplay.player.clone();
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.label("Max Health");
                changed |= ui
                    .add(egui::DragValue::new(&mut player.max_health).range(1..=16))
                    .changed();
            });
            ui.horizontal(|ui| {
                ui.label("Starting Health");
                changed |= ui
                    .add(egui::DragValue::new(&mut player.starting_health).range(1..=16))
                    .changed();
            });

            egui::ComboBox::from_label("Health HUD")
                .selected_text(match player.health_hud {
                    HealthHudStyle::MegaPipsTopLeft => "Mega Pips (Top Left)",
                    HealthHudStyle::HeartsTopRight => "Hearts (Top Right)",
                    HealthHudStyle::CellsTopCenter => "Cells (Top Center)",
                })
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(
                            &mut player.health_hud,
                            HealthHudStyle::MegaPipsTopLeft,
                            "Mega Pips (Top Left)",
                        )
                        .changed();
                    changed |= ui
                        .selectable_value(
                            &mut player.health_hud,
                            HealthHudStyle::HeartsTopRight,
                            "Hearts (Top Right)",
                        )
                        .changed();
                    changed |= ui
                        .selectable_value(
                            &mut player.health_hud,
                            HealthHudStyle::CellsTopCenter,
                            "Cells (Top Center)",
                        )
                        .changed();
                });

            if player.starting_health > player.max_health {
                player.starting_health = player.max_health;
                changed = true;
            }

            if changed {
                self.capture_history();
                if let Some(bundle) = &mut self.bundle {
                    bundle.manifest.gameplay.player = player;
                }
                self.mark_edited("Updated player HUD settings");
            }
        });
    }

    fn draw_animation_inspector(
        &mut self,
        ui: &mut egui::Ui,
        animation_snapshot: Option<&AnimationResource>,
        metasprite_ids: &[String],
    ) {
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

            let mut edited = animation_snapshot.clone();
            let mut changed = false;
            let mut remove_index = None;

            for (index, frame) in edited.frames.iter_mut().enumerate() {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(format!("Frame {}", index + 1));
                        egui::ComboBox::from_id_salt((
                            "animation_frame",
                            self.selected_animation,
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
                        if ui.button("Remove").clicked() {
                            remove_index = Some(index);
                        }
                    });
                });
            }

            if let Some(index) = remove_index {
                edited.frames.remove(index);
                changed = true;
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
                    self.preview_focus = PreviewFocus::Animation;
                }
            });

            if changed {
                self.capture_history();
                if let Some(bundle) = &mut self.bundle {
                    if let Some(animation) = bundle.animations.get_mut(self.selected_animation) {
                        *animation = edited;
                    }
                }
                self.mark_edited(format!("Updated animation '{}'", animation_snapshot.id));
            }
        });
    }

    fn draw_spawn_inspector(&mut self, ui: &mut egui::Ui, scene_snapshot: &SceneResource) {
        ui.collapsing("Spawns", |ui| {
            ui.horizontal(|ui| {
                if ui.button("+ Spawn").clicked() {
                    self.capture_history();
                    if let Some(scene) = self.current_scene_mut() {
                        let next_index = scene.spawns.len() + 1;
                        scene.spawns.push(SpawnPoint {
                            id: format!("spawn_{}", next_index),
                            position: PointI16 { x: 8, y: 8 },
                        });
                        self.selected_spawn = Some(scene.spawns.len() - 1);
                    }
                    self.tool = EditorTool::Spawn;
                    self.preview_focus = PreviewFocus::None;
                    self.mark_edited("Added spawn");
                }
                if ui.button("- Remove").clicked() {
                    if let Some(index) = self.selected_spawn {
                        self.capture_history();
                        if let Some(scene) = self.current_scene_mut() {
                            if index < scene.spawns.len() {
                                scene.spawns.remove(index);
                            }
                        }
                        self.selected_spawn = None;
                        self.preview_focus = PreviewFocus::None;
                        self.mark_edited("Removed spawn");
                    }
                }
            });

            for (index, spawn) in scene_snapshot.spawns.iter().enumerate() {
                if ui
                    .selectable_label(self.selected_spawn == Some(index), &spawn.id)
                    .clicked()
                {
                    self.selected_spawn = Some(index);
                    self.tool = EditorTool::Spawn;
                    self.preview_focus = PreviewFocus::None;
                }
            }

            if let Some(index) = self.selected_spawn {
                if let Some(spawn) = scene_snapshot.spawns.get(index) {
                    let mut edited = spawn.clone();
                    let mut changed = false;
                    changed |= ui.text_edit_singleline(&mut edited.id).changed();
                    changed |= ui
                        .add(egui::DragValue::new(&mut edited.position.x).speed(1))
                        .changed();
                    changed |= ui
                        .add(egui::DragValue::new(&mut edited.position.y).speed(1))
                        .changed();
                    if ui.button("Place Spawn In Scene").clicked() {
                        self.tool = EditorTool::Spawn;
                        self.preview_focus = PreviewFocus::None;
                    }
                    if changed {
                        self.capture_history();
                        if let Some(scene) = self.current_scene_mut() {
                            if let Some(target) = scene.spawns.get_mut(index) {
                                *target = edited;
                            }
                        }
                        self.mark_edited("Updated spawn");
                    }
                }
            }
        });
    }

    fn draw_checkpoint_inspector(&mut self, ui: &mut egui::Ui, scene_snapshot: &SceneResource) {
        ui.collapsing("Checkpoints", |ui| {
            ui.horizontal(|ui| {
                if ui.button("+ Checkpoint").clicked() {
                    self.capture_history();
                    if let Some(scene) = self.current_scene_mut() {
                        let next_index = scene.checkpoints.len() + 1;
                        scene.checkpoints.push(Checkpoint {
                            id: format!("checkpoint_{}", next_index),
                            position: PointI16 { x: 8, y: 8 },
                        });
                        self.selected_checkpoint = Some(scene.checkpoints.len() - 1);
                    }
                    self.tool = EditorTool::Checkpoint;
                    self.preview_focus = PreviewFocus::None;
                    self.mark_edited("Added checkpoint");
                }
                if ui.button("- Remove").clicked() {
                    if let Some(index) = self.selected_checkpoint {
                        self.capture_history();
                        if let Some(scene) = self.current_scene_mut() {
                            if index < scene.checkpoints.len() {
                                scene.checkpoints.remove(index);
                            }
                        }
                        self.selected_checkpoint = None;
                        self.preview_focus = PreviewFocus::None;
                        self.mark_edited("Removed checkpoint");
                    }
                }
            });

            for (index, checkpoint) in scene_snapshot.checkpoints.iter().enumerate() {
                if ui
                    .selectable_label(self.selected_checkpoint == Some(index), &checkpoint.id)
                    .clicked()
                {
                    self.selected_checkpoint = Some(index);
                    self.tool = EditorTool::Checkpoint;
                    self.preview_focus = PreviewFocus::None;
                }
            }

            if let Some(index) = self.selected_checkpoint {
                if let Some(checkpoint) = scene_snapshot.checkpoints.get(index) {
                    let mut edited = checkpoint.clone();
                    let mut changed = false;
                    changed |= ui.text_edit_singleline(&mut edited.id).changed();
                    changed |= ui
                        .add(egui::DragValue::new(&mut edited.position.x).speed(1))
                        .changed();
                    changed |= ui
                        .add(egui::DragValue::new(&mut edited.position.y).speed(1))
                        .changed();
                    if ui.button("Place Checkpoint In Scene").clicked() {
                        self.tool = EditorTool::Checkpoint;
                        self.preview_focus = PreviewFocus::None;
                    }
                    if changed {
                        self.capture_history();
                        if let Some(scene) = self.current_scene_mut() {
                            if let Some(target) = scene.checkpoints.get_mut(index) {
                                *target = edited;
                            }
                        }
                        self.mark_edited("Updated checkpoint");
                    }
                }
            }
        });
    }

    fn draw_entity_inspector(&mut self, ui: &mut egui::Ui, scene_snapshot: &SceneResource) {
        ui.collapsing("Entities", |ui| {
            let visual_ids = self.available_visual_ids();
            ui.horizontal(|ui| {
                if ui.button("+ Entity").clicked() {
                    self.capture_history();
                    let archetype = visual_ids
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "entity".to_string());
                    if let Some(scene) = self.current_scene_mut() {
                        let next_index = scene.entities.len() + 1;
                        scene.entities.push(EntityPlacement {
                            id: format!("entity_{}", next_index),
                            archetype,
                            position: PointI16 { x: 8, y: 8 },
                            facing: Facing::Right,
                            kind: EntityKind::Prop,
                            hitbox: default_entity_hitbox(),
                            movement: MovementPattern::None,
                            combat: CombatProfile::default(),
                            action: EntityAction::None,
                            active: true,
                            one_shot: false,
                        });
                        self.selected_entity = Some(scene.entities.len() - 1);
                    }
                    self.tool = EditorTool::Entity;
                    self.preview_focus = PreviewFocus::Entity;
                    self.mark_edited("Added entity");
                }
                if ui.button("- Remove").clicked() {
                    if let Some(index) = self.selected_entity {
                        self.capture_history();
                        if let Some(scene) = self.current_scene_mut() {
                            if index < scene.entities.len() {
                                scene.entities.remove(index);
                            }
                        }
                        self.selected_entity = None;
                        self.preview_focus = PreviewFocus::None;
                        self.mark_edited("Removed entity");
                    }
                }
            });

            for (index, entity) in scene_snapshot.entities.iter().enumerate() {
                if ui
                    .selectable_label(
                        self.selected_entity == Some(index),
                        format!("{} ({})", entity.id, entity.archetype),
                    )
                    .clicked()
                {
                    self.selected_entity = Some(index);
                    self.tool = EditorTool::Entity;
                    self.preview_focus = PreviewFocus::Entity;
                }
            }

            if let Some(index) = self.selected_entity {
                if let Some(entity) = scene_snapshot.entities.get(index) {
                    let mut edited = entity.clone();
                    let mut changed = false;
                    let target_entity_ids = scene_snapshot
                        .entities
                        .iter()
                        .enumerate()
                        .filter_map(|(entity_index, entity)| {
                            (entity_index != index).then_some(entity.id.clone())
                        })
                        .collect::<Vec<_>>();
                    changed |= ui.text_edit_singleline(&mut edited.id).changed();
                    egui::ComboBox::from_label("Visual")
                        .selected_text(&edited.archetype)
                        .show_ui(ui, |ui| {
                            for visual_id in &visual_ids {
                                changed |= ui
                                    .selectable_value(
                                        &mut edited.archetype,
                                        visual_id.clone(),
                                        visual_id,
                                    )
                                    .changed();
                            }
                        });
                    changed |= ui
                        .add(egui::DragValue::new(&mut edited.position.x).speed(1))
                        .changed();
                    changed |= ui
                        .add(egui::DragValue::new(&mut edited.position.y).speed(1))
                        .changed();
                    egui::ComboBox::from_label("Kind")
                        .selected_text(match edited.kind {
                            EntityKind::Prop => "Prop",
                            EntityKind::Pickup => "Pickup",
                            EntityKind::Enemy => "Enemy",
                            EntityKind::Switch => "Switch",
                            EntityKind::Solid => "Solid",
                        })
                        .show_ui(ui, |ui| {
                            changed |= ui
                                .selectable_value(&mut edited.kind, EntityKind::Prop, "Prop")
                                .changed();
                            changed |= ui
                                .selectable_value(&mut edited.kind, EntityKind::Pickup, "Pickup")
                                .changed();
                            changed |= ui
                                .selectable_value(&mut edited.kind, EntityKind::Enemy, "Enemy")
                                .changed();
                            changed |= ui
                                .selectable_value(&mut edited.kind, EntityKind::Switch, "Switch")
                                .changed();
                            changed |= ui
                                .selectable_value(&mut edited.kind, EntityKind::Solid, "Solid")
                                .changed();
                        });
                    match edited.kind {
                        EntityKind::Enemy => {
                            if edited.combat.max_health == 0 {
                                edited.combat.max_health = 1;
                                changed = true;
                            }
                            if edited.combat.contact_damage == 0 {
                                edited.combat.contact_damage = 1;
                                changed = true;
                            }
                        }
                        EntityKind::Pickup => {
                            if matches!(edited.action, EntityAction::None) {
                                edited.action = EntityAction::HealPlayer { amount: 1 };
                                changed = true;
                            }
                        }
                        EntityKind::Switch => {
                            if matches!(edited.action, EntityAction::None) {
                                edited.action = EntityAction::SetEntityActive {
                                    target_entity_id: target_entity_ids
                                        .first()
                                        .cloned()
                                        .unwrap_or_default(),
                                    active: false,
                                };
                                changed = true;
                            }
                        }
                        EntityKind::Prop | EntityKind::Solid => {}
                    }
                    egui::ComboBox::from_label("Facing")
                        .selected_text(match edited.facing {
                            Facing::Left => "Left",
                            Facing::Right => "Right",
                        })
                        .show_ui(ui, |ui| {
                            changed |= ui
                                .selectable_value(&mut edited.facing, Facing::Left, "Left")
                                .changed();
                            changed |= ui
                                .selectable_value(&mut edited.facing, Facing::Right, "Right")
                                .changed();
                        });
                    changed |= ui
                        .checkbox(&mut edited.active, "Initially Active")
                        .changed();
                    changed |= ui
                        .checkbox(&mut edited.one_shot, "Deactivate After Trigger")
                        .changed();

                    ui.separator();
                    ui.label("Hitbox");
                    ui.horizontal(|ui| {
                        ui.label("x");
                        changed |= ui
                            .add(egui::DragValue::new(&mut edited.hitbox.x).speed(1))
                            .changed();
                        ui.label("y");
                        changed |= ui
                            .add(egui::DragValue::new(&mut edited.hitbox.y).speed(1))
                            .changed();
                    });
                    ui.horizontal(|ui| {
                        ui.label("w");
                        changed |= ui
                            .add(egui::DragValue::new(&mut edited.hitbox.width).range(1..=255))
                            .changed();
                        ui.label("h");
                        changed |= ui
                            .add(egui::DragValue::new(&mut edited.hitbox.height).range(1..=255))
                            .changed();
                    });

                    ui.separator();
                    egui::ComboBox::from_label("Movement")
                        .selected_text(match edited.movement {
                            MovementPattern::None => "None",
                            MovementPattern::Patrol { .. } => "Patrol",
                        })
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(
                                    matches!(edited.movement, MovementPattern::None),
                                    "None",
                                )
                                .clicked()
                            {
                                edited.movement = MovementPattern::None;
                                changed = true;
                            }
                            if ui
                                .selectable_label(
                                    matches!(edited.movement, MovementPattern::Patrol { .. }),
                                    "Patrol",
                                )
                                .clicked()
                            {
                                edited.movement = MovementPattern::Patrol {
                                    left_offset: -24,
                                    right_offset: 24,
                                    speed: 1,
                                };
                                changed = true;
                            }
                        });
                    if let MovementPattern::Patrol {
                        left_offset,
                        right_offset,
                        speed,
                    } = &mut edited.movement
                    {
                        ui.horizontal(|ui| {
                            ui.label("Left");
                            changed |= ui.add(egui::DragValue::new(left_offset).speed(1)).changed();
                            ui.label("Right");
                            changed |= ui
                                .add(egui::DragValue::new(right_offset).speed(1))
                                .changed();
                            ui.label("Speed");
                            changed |= ui.add(egui::DragValue::new(speed).range(1..=8)).changed();
                        });
                    }

                    if matches!(edited.kind, EntityKind::Enemy) {
                        ui.separator();
                        ui.label("Combat");
                        ui.horizontal(|ui| {
                            ui.label("HP");
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut edited.combat.max_health)
                                        .range(1..=16),
                                )
                                .changed();
                            ui.label("Touch Damage");
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut edited.combat.contact_damage)
                                        .range(1..=8),
                                )
                                .changed();
                        });
                    }

                    ui.separator();
                    egui::ComboBox::from_label("Action")
                        .selected_text(match &edited.action {
                            EntityAction::None => "None",
                            EntityAction::HealPlayer { .. } => "Heal Player",
                            EntityAction::SetEntityActive { .. } => "Set Entity Active",
                        })
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(
                                    matches!(edited.action, EntityAction::None),
                                    "None",
                                )
                                .clicked()
                            {
                                edited.action = EntityAction::None;
                                changed = true;
                            }
                            if ui
                                .selectable_label(
                                    matches!(edited.action, EntityAction::HealPlayer { .. }),
                                    "Heal Player",
                                )
                                .clicked()
                            {
                                edited.action = EntityAction::HealPlayer { amount: 1 };
                                changed = true;
                            }
                            if ui
                                .selectable_label(
                                    matches!(edited.action, EntityAction::SetEntityActive { .. }),
                                    "Set Entity Active",
                                )
                                .clicked()
                            {
                                edited.action = EntityAction::SetEntityActive {
                                    target_entity_id: target_entity_ids
                                        .first()
                                        .cloned()
                                        .unwrap_or_default(),
                                    active: false,
                                };
                                changed = true;
                            }
                        });
                    match &mut edited.action {
                        EntityAction::None => {}
                        EntityAction::HealPlayer { amount } => {
                            ui.horizontal(|ui| {
                                ui.label("Heal");
                                changed |=
                                    ui.add(egui::DragValue::new(amount).range(1..=8)).changed();
                            });
                        }
                        EntityAction::SetEntityActive {
                            target_entity_id,
                            active,
                        } => {
                            if target_entity_ids.is_empty() {
                                ui.label("No other entities are available to target yet.");
                            } else {
                                egui::ComboBox::from_label("Target Entity")
                                    .selected_text(if target_entity_id.is_empty() {
                                        "Choose target"
                                    } else {
                                        target_entity_id.as_str()
                                    })
                                    .show_ui(ui, |ui| {
                                        for candidate in &target_entity_ids {
                                            changed |= ui
                                                .selectable_value(
                                                    target_entity_id,
                                                    candidate.clone(),
                                                    candidate,
                                                )
                                                .changed();
                                        }
                                    });
                            }
                            changed |= ui.checkbox(active, "Set Target Active").changed();
                        }
                    }
                    if ui.button("Place Entity In Scene").clicked() {
                        self.tool = EditorTool::Entity;
                        self.preview_focus = PreviewFocus::Entity;
                    }
                    if changed {
                        self.capture_history();
                        if let Some(scene) = self.current_scene_mut() {
                            if let Some(target) = scene.entities.get_mut(index) {
                                *target = edited;
                            }
                        }
                        self.mark_edited("Updated entity");
                    }
                }
            }
        });
    }

    fn draw_context_preview(&mut self, ui: &mut egui::Ui, time_seconds: f32) {
        let Some(bundle) = &self.bundle else {
            return;
        };

        match self.preview_focus {
            PreviewFocus::Animation => {
                let Some(animation) = bundle.animations.get(self.selected_animation) else {
                    return;
                };
                ui.collapsing("Animation Preview", |ui| {
                    ui.label(format!("Selected animation: {}", animation.id));
                    draw_animation_preview(ui, bundle, self.selected_animation, time_seconds);
                });
            }
            PreviewFocus::Entity => {
                let Some(scene) = bundle.scenes.get(self.selected_scene) else {
                    return;
                };
                let Some(index) = self.selected_entity else {
                    return;
                };
                let Some(entity) = scene.entities.get(index) else {
                    return;
                };
                if metasprite_for_entity(bundle, entity, time_seconds).is_none() {
                    return;
                }
                ui.collapsing("Animation Preview", |ui| {
                    ui.label(format!(
                        "Selected entity: {} ({})",
                        entity.id, entity.archetype
                    ));
                    draw_entity_preview(ui, bundle, entity, time_seconds);
                });
            }
            PreviewFocus::None => {}
        }
    }

    fn has_context_preview(&self) -> bool {
        let Some(bundle) = &self.bundle else {
            return false;
        };

        match self.preview_focus {
            PreviewFocus::Animation => bundle.animations.get(self.selected_animation).is_some(),
            PreviewFocus::Entity => bundle
                .scenes
                .get(self.selected_scene)
                .and_then(|scene| {
                    self.selected_entity
                        .and_then(|index| scene.entities.get(index))
                })
                .is_some_and(|entity| entity_has_animation(bundle, entity)),
            PreviewFocus::None => false,
        }
    }

    fn needs_animation_repaint(&self) -> bool {
        let Some(bundle) = &self.bundle else {
            return false;
        };

        self.has_context_preview()
            || bundle.scenes.get(self.selected_scene).is_some_and(|scene| {
                scene
                    .entities
                    .iter()
                    .any(|entity| entity_has_animation(bundle, entity))
            })
    }

    fn draw_trigger_inspector(&mut self, ui: &mut egui::Ui, scene_snapshot: &SceneResource) {
        ui.collapsing("Triggers", |ui| {
            ui.horizontal(|ui| {
                if ui.button("+ Trigger").clicked() {
                    self.capture_history();
                    if let Some(scene) = self.current_scene_mut() {
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
                            script_id: "start_dialogue".to_string(),
                        });
                        self.selected_trigger = Some(scene.triggers.len() - 1);
                    }
                    self.tool = EditorTool::Trigger;
                    self.preview_focus = PreviewFocus::None;
                    self.mark_edited("Added trigger");
                }
                if ui.button("- Remove").clicked() {
                    if let Some(index) = self.selected_trigger {
                        self.capture_history();
                        if let Some(scene) = self.current_scene_mut() {
                            if index < scene.triggers.len() {
                                scene.triggers.remove(index);
                            }
                        }
                        self.selected_trigger = None;
                        self.preview_focus = PreviewFocus::None;
                        self.mark_edited("Removed trigger");
                    }
                }
            });

            for (index, trigger) in scene_snapshot.triggers.iter().enumerate() {
                if ui
                    .selectable_label(
                        self.selected_trigger == Some(index),
                        format!("{} ({:?})", trigger.id, trigger.kind),
                    )
                    .clicked()
                {
                    self.selected_trigger = Some(index);
                    self.tool = EditorTool::Trigger;
                    self.preview_focus = PreviewFocus::None;
                }
            }

            if let Some(index) = self.selected_trigger {
                if let Some(trigger) = scene_snapshot.triggers.get(index) {
                    let mut edited = trigger.clone();
                    let mut changed = false;
                    changed |= ui.text_edit_singleline(&mut edited.id).changed();
                    changed |= ui.text_edit_singleline(&mut edited.script_id).changed();
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
                                .selectable_value(
                                    &mut edited.kind,
                                    TriggerKind::Interact,
                                    "Interact",
                                )
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
                        self.tool = EditorTool::Trigger;
                        self.preview_focus = PreviewFocus::None;
                    }
                    if changed {
                        self.capture_history();
                        if let Some(scene) = self.current_scene_mut() {
                            if let Some(target) = scene.triggers.get_mut(index) {
                                *target = edited;
                            }
                        }
                        self.mark_edited("Updated trigger");
                    }
                }
            }
        });
    }

    fn draw_tile_editor(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("Tile Editor", |ui| {
            let Some(layer) = self.current_layer().cloned() else {
                return;
            };
            let Some(bundle) = &self.bundle else {
                return;
            };
            let Some(tileset) = bundle.tileset(&layer.tileset_id) else {
                ui.label("Active layer tileset is missing.");
                return;
            };
            let Some(palette) = bundle.palette(&tileset.palette_id) else {
                ui.label("Active layer palette is missing.");
                return;
            };
            let Some(tile) = tileset.tiles.get(self.selected_tile) else {
                return;
            };

            ui.label(format!(
                "Layer '{}'  |  Tile {}",
                layer.id, self.selected_tile
            ));
            let mut edited = tile.clone();
            let changed = draw_tile_editor_grid(ui, &mut edited, palette);
            if changed {
                self.capture_history();
                if let Some(bundle) = &mut self.bundle {
                    let Some(tileset_id) = bundle
                        .scenes
                        .get(self.selected_scene)
                        .and_then(|scene| scene.layers.get(self.selected_layer))
                        .map(|layer| layer.tileset_id.clone())
                    else {
                        return;
                    };
                    if let Some(tileset) = bundle
                        .tilesets
                        .iter_mut()
                        .find(|tileset| tileset.id == tileset_id)
                    {
                        if let Some(tile) = tileset.tiles.get_mut(self.selected_tile) {
                            *tile = edited;
                        }
                    }
                }
                self.mark_edited("Edited tile");
            }
        });
    }

    fn draw_diagnostics(&mut self, ui: &mut egui::Ui) {
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
            let palette_fraction =
                (self.report.budgets.palette_colors as f32 / total_palette_capacity as f32)
                    .clamp(0.0, 1.0);
            let rom_fraction = (self.report.budgets.estimated_rom_banks as f32
                / max_rom_banks as f32)
                .clamp(0.0, 1.0);
            let metasprite_fraction = (self.report.budgets.metasprite_piece_peak as f32
                / MAX_METASPRITE_TILES_HARD as f32)
                .clamp(0.0, 1.0);
            let rom_bytes_capacity = max_rom_banks * ROM_BANK_SIZE;
            let rom_bytes_fraction =
                (self.report.budgets.estimated_rom_bytes as f32 / rom_bytes_capacity as f32)
                    .clamp(0.0, 1.0);
            let metasprite_warning = self.report.budgets.metasprite_piece_peak
                >= MAX_METASPRITE_TILES_WARN;

            ui.label("Tileset peak");
            ui.add(
                egui::ProgressBar::new(tile_fraction).text(format!(
                    "{} / {} tiles",
                    tile_peak, MAX_TILESET_TILES
                )),
            );
            ui.label("Palette colors");
            ui.add(
                egui::ProgressBar::new(palette_fraction).text(format!(
                    "{} / {} colors",
                    self.report.budgets.palette_colors, total_palette_capacity
                )),
            );
            ui.label("Metasprite peak");
            ui.add(
                egui::ProgressBar::new(metasprite_fraction).text(format!(
                    "{} / {} pieces{}",
                    self.report.budgets.metasprite_piece_peak,
                    MAX_METASPRITE_TILES_HARD,
                    if metasprite_warning { " (warning zone)" } else { "" }
                )),
            );
            ui.label("ROM banks");
            ui.add(
                egui::ProgressBar::new(rom_fraction).text(format!(
                    "{} / {} bank(s)",
                    self.report.budgets.estimated_rom_banks, max_rom_banks
                )),
            );
            ui.label("ROM bytes");
            ui.add(
                egui::ProgressBar::new(rom_bytes_fraction).text(format!(
                    "{} / {} bytes",
                    self.report.budgets.estimated_rom_bytes, rom_bytes_capacity
                )),
            );
        });

        let diagnostics = self.filtered_diagnostics();
        if diagnostics.is_empty() {
            ui.separator();
            ui.label("No diagnostics match the current filters.");
        } else {
            ui.separator();
            match self.diagnostics_view.grouping {
                DiagnosticGrouping::Severity => {
                    for (heading, severity) in [
                        ("Errors", Severity::Error),
                        ("Warnings", Severity::Warning),
                    ] {
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
                                    self.diagnostic_has_quick_fix(diagnostic),
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
                                    self.diagnostic_has_quick_fix(diagnostic),
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
                                    self.diagnostic_has_quick_fix(diagnostic),
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
            self.apply_diagnostic_quick_fix(&diagnostic);
        }
    }

    fn draw_scene_outliner(&mut self, ui: &mut egui::Ui) {
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
                label: String,
            },
            ScriptIsolate {
                scene_index: usize,
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
                                .small_button(if self.is_layer_soloed(scene_index, self.selected_layer)
                                {
                                    "Clear Solo"
                                } else {
                                    "Solo Active"
                                })
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
                                    pending_action =
                                        Some(PendingOutlinerAction::LayerVisibility {
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
                                    .small_button(if self.is_layer_soloed(scene_index, layer_index) {
                                        "Unsolo"
                                    } else {
                                        "Solo"
                                    })
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
                            ui.label(format!("{} ({})", SceneObjectGroup::Spawns.label(), scene.spawns.len()));
                            if ui
                                .small_button(if self.is_group_soloed(SceneObjectGroup::Spawns) {
                                    "Unsolo"
                                } else {
                                    "Solo"
                                })
                                .clicked()
                            {
                                pending_action =
                                    Some(PendingOutlinerAction::GroupSolo(SceneObjectGroup::Spawns));
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
                                    pending_action =
                                        Some(PendingOutlinerAction::SpawnDuplicate {
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
                                .small_button(if self.is_group_soloed(SceneObjectGroup::Checkpoints)
                                {
                                    "Unsolo"
                                } else {
                                    "Solo"
                                })
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
                                    pending_action = Some(
                                        PendingOutlinerAction::CheckpointDuplicate {
                                            scene_index,
                                            index,
                                        },
                                    );
                                }
                                if ui.small_button("Isolate").clicked() {
                                    pending_action = Some(
                                        PendingOutlinerAction::CheckpointIsolate {
                                            scene_index,
                                            index,
                                        },
                                    );
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
                                    pending_action =
                                        Some(PendingOutlinerAction::EntityDuplicate {
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
                                    .small_button(if self.is_group_soloed(SceneObjectGroup::Scripts)
                                    {
                                        "Unsolo"
                                    } else {
                                        "Solo"
                                    })
                                    .clicked()
                                {
                                    pending_action = Some(PendingOutlinerAction::GroupSolo(
                                        SceneObjectGroup::Scripts,
                                    ));
                                }
                            });
                            for script in &scene.scripts {
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
                                            label: script.id.clone(),
                                        });
                                    }
                                    if ui.small_button("Isolate").clicked() {
                                        pending_action = Some(
                                            PendingOutlinerAction::ScriptIsolate {
                                                scene_index,
                                                label: script.id.clone(),
                                            },
                                        );
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
                                    scene.spawns.iter().map(|entry| entry.id.clone()).collect::<BTreeSet<_>>()
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
                                    scene.checkpoints
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
                                    scene.entities.iter().map(|entry| entry.id.clone()).collect::<BTreeSet<_>>()
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
                                    scene.triggers.iter().map(|entry| entry.id.clone()).collect::<BTreeSet<_>>()
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
                PendingOutlinerAction::Script { scene_index, label } => {
                    self.selected_scene = scene_index;
                    self.scene_scroll_offset = Vec2::ZERO;
                    self.sync_selection();
                    self.preview_focus = PreviewFocus::None;
                    self.status = format!(
                        "Selected script '{}'. A visual event editor can plug in here.",
                        label
                    );
                }
                PendingOutlinerAction::ScriptIsolate { scene_index, label } => {
                    self.selected_scene = scene_index;
                    self.solo_group = Some(SceneObjectGroup::Scripts);
                    self.sync_selection();
                    self.preview_focus = PreviewFocus::None;
                    self.status = format!("Isolated script group around '{}'", label);
                }
            }
        }
    }

    fn draw_asset_browser(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
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
            Animation {
                animation_index: usize,
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
        }

        let filter = self.asset_browser_filter.trim().to_ascii_lowercase();
        let sprite_sources = self.list_project_sprite_sources().unwrap_or_default();
        let time_seconds = ctx.input(|input| input.time) as f32;
        let mut pending_action = None;

        ui.label(format!(
            "{} scene(s) | {} tileset(s) | {} palette(s) | {} metasprite(s) | {} animation(s) | {} dialogue(s)",
            bundle.scenes.len(),
            bundle.tilesets.len(),
            bundle.palettes.len(),
            bundle.metasprites.len(),
            bundle.animations.len(),
            bundle.dialogues.len()
        ));
        ui.label(format!(
            "{} favorite(s) | {} snippet(s) | {} brush(es)",
            self.workspace_addons.editor_favorites.scenes.len()
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
                            ui.small(format!(
                                "{}",
                                self.asset_usage_summary("scene", &scene.id)
                            ));
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

            ui.collapsing(format!("Animations ({})", bundle.animations.len()), |ui| {
                for (animation_index, animation) in bundle.animations.iter().enumerate() {
                    if !filter_matches(&filter, &animation.id) && !filter.is_empty() {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        draw_animation_thumbnail(ui, &bundle, animation_index, time_seconds);
                        ui.vertical(|ui| {
                            if ui
                                .selectable_label(
                                    self.selected_animation == animation_index,
                                    format!("{} ({} frame(s))", animation.id, animation.frames.len()),
                                )
                                .clicked()
                            {
                                pending_action = Some(PendingAssetAction::Animation {
                                    animation_index,
                                    label: animation.id.clone(),
                                });
                            }
                            ui.small(self.asset_usage_summary("animation", &animation.id));
                            if ui
                                .small_button(if self.is_asset_favorite("animation", &animation.id) {
                                    "Unstar"
                                } else {
                                    "Star"
                                })
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

            ui.collapsing(format!("Metasprites ({})", bundle.metasprites.len()), |ui| {
                for metasprite in &bundle.metasprites {
                    if !filter_matches(&filter, &metasprite.id) && !filter.is_empty() {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        draw_metasprite_thumbnail(ui, &bundle, metasprite);
                        ui.vertical(|ui| {
                            if ui
                                .selectable_label(
                                    false,
                                    format!("{} ({} piece(s))", metasprite.id, metasprite.pieces.len()),
                                )
                                .clicked()
                            {
                                pending_action = Some(PendingAssetAction::Status(format!(
                                    "Metasprite '{}' selected. Use the Animation tab for visual editing.",
                                    metasprite.id
                                )));
                            }
                            ui.small(self.asset_usage_summary("metasprite", &metasprite.id));
                            if ui
                                .small_button(if self.is_asset_favorite("metasprite", &metasprite.id) {
                                    "Unstar"
                                } else {
                                    "Star"
                                })
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
            });

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
                for dialogue in &bundle.dialogues {
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
                                false,
                                format!("{} ({} node(s))", dialogue.id, dialogue.nodes.len()),
                            )
                            .clicked()
                        {
                            pending_action = Some(PendingAssetAction::Status(format!(
                                "Dialogue '{}' selected. A graph editor would fit naturally here.",
                                dialogue.id
                            )));
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
                ui.collapsing(format!("Scripts in '{}' ({})", scene.id, scene.scripts.len()), |ui| {
                    for script in &scene.scripts {
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
                                if ui
                                    .selectable_label(
                                        false,
                                        format!("{} ({} command(s))", script.id, script.commands.len()),
                                    )
                                    .clicked()
                                {
                                    pending_action = Some(PendingAssetAction::Status(format!(
                                        "Script '{}' selected. A visual event editor would make this much faster to author.",
                                        script.id
                                    )));
                                }
                                ui.small(format!("Scene: {}", scene.id));
                            });
                        });
                    }
                });
            }

            ui.collapsing(
                format!("Sprite Sources ({})", sprite_sources.len()),
                |ui| match sprite_sources.is_empty() {
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
                                if let Some(texture) = self.sprite_source_preview_texture(ctx, path) {
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
                                    .small_button(if self.is_asset_favorite("sprite_source", &label)
                                    {
                                        "Unstar"
                                    } else {
                                        "Star"
                                    })
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
                },
            );

            ui.collapsing(
                format!(
                    "Scene Snippets ({})",
                    self.workspace_addons.scene_library.snippets.len()
                ),
                |ui| {
                    if self.workspace_addons.scene_library.snippets.is_empty() {
                        ui.label("Save a selection as a snippet from the Scene tab.");
                    } else {
                        for snippet in &self.workspace_addons.scene_library.snippets {
                            if !filter_matches(&filter, &snippet.name) && !filter.is_empty() {
                                continue;
                            }
                            ui.horizontal(|ui| {
                                draw_scene_snippet_thumbnail(ui, &bundle, snippet);
                                ui.vertical(|ui| {
                                    if ui
                                        .selectable_label(
                                            false,
                                            format!(
                                                "{} ({}x{} tiles, {} object(s))",
                                                snippet.name,
                                                snippet.size_tiles.width,
                                                snippet.size_tiles.height,
                                                snippet.spawns.len()
                                                    + snippet.checkpoints.len()
                                                    + snippet.entities.len()
                                                    + snippet.triggers.len()
                                            ),
                                        )
                                        .clicked()
                                    {
                                        pending_action = Some(PendingAssetAction::LoadSnippet(
                                            snippet.name.clone(),
                                        ));
                                    }
                                    if ui.small_button("Load").clicked() {
                                        pending_action = Some(PendingAssetAction::LoadSnippet(
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
                format!("Tile Brushes ({})", self.workspace_addons.scene_library.brushes.len()),
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
                                    if ui
                                        .selectable_label(
                                            false,
                                            format!(
                                                "{} ({}x{} tiles)",
                                                brush.name,
                                                brush.size_tiles.width,
                                                brush.size_tiles.height
                                            ),
                                        )
                                        .clicked()
                                    {
                                        pending_action = Some(PendingAssetAction::LoadBrush(
                                            brush.name.clone(),
                                        ));
                                    }
                                    if ui.small_button("Load").clicked() {
                                        pending_action = Some(PendingAssetAction::LoadBrush(
                                            brush.name.clone(),
                                        ));
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
                PendingAssetAction::Animation {
                    animation_index,
                    label,
                } => {
                    self.selected_animation = animation_index;
                    self.preview_focus = PreviewFocus::Animation;
                    self.status = format!("Selected animation '{}'", label);
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
            }
        }
    }

    fn draw_animation_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(bundle) = &self.bundle else {
            ui.heading("Animation");
            ui.label("Load a project to preview animations.");
            return;
        };

        let metasprite_ids = bundle
            .metasprites
            .iter()
            .map(|metasprite| metasprite.id.clone())
            .collect::<Vec<_>>();
        let animation_snapshot = bundle.animations.get(self.selected_animation).cloned();

        ui.heading("Animation");
        ui.label("Preview the active animation or entity context while editing frame data.");
        ui.separator();
        ui.label("Animations");
        egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
            for (index, animation) in bundle.animations.iter().enumerate() {
                if ui
                    .selectable_label(
                        self.selected_animation == index,
                        format!("{} ({} frame(s))", animation.id, animation.frames.len()),
                    )
                    .clicked()
                {
                    self.selected_animation = index;
                    self.preview_focus = PreviewFocus::Animation;
                }
            }
        });
        ui.separator();
        if self.has_context_preview() {
            self.draw_context_preview(ui, ctx.input(|input| input.time) as f32);
        } else if bundle.animations.get(self.selected_animation).is_some() {
            self.preview_focus = PreviewFocus::Animation;
            self.draw_context_preview(ui, ctx.input(|input| input.time) as f32);
        } else {
            ui.label("Select an animation to preview it here.");
        }
        ui.separator();
        self.draw_animation_inspector(ui, animation_snapshot.as_ref(), &metasprite_ids);
    }

    fn draw_build_report_tab(&mut self, ui: &mut egui::Ui) {
        let report_path = self.build_report_path();
        let mut refresh_report = false;
        let mut build = false;

        ui.heading("Build Report");
        ui.label(format!("Report file: {}", report_path));
        ui.horizontal(|ui| {
            if ui.button("Build ROM").clicked() {
                build = true;
            }
            if ui.button("Refresh Report").clicked() {
                refresh_report = true;
            }
        });

        if build {
            self.build_current_rom();
        }
        if refresh_report {
            self.refresh_last_build_report();
        }

        let Some(outcome) = &self.last_build_outcome else {
            ui.separator();
            ui.label("No build report loaded yet. Build the ROM or refresh from disk.");
            return;
        };

        ui.separator();
        ui.label(format!(
            "ROM: {}",
            if outcome.rom_built {
                "built successfully"
            } else {
                "build assets only"
            }
        ));
        ui.label(format!("Build directory: {}", outcome.build_dir));
        ui.label(format!("ROM path: {}", outcome.rom_path));
        ui.label(format!(
            "Validation: {} error(s), {} warning(s)",
            outcome.validation.errors.len(),
            outcome.validation.warnings.len()
        ));
        ui.label(format!(
            "Assembler: ca65={} ld65={}",
            outcome.assembler_status.ca65_found, outcome.assembler_status.ld65_found
        ));

        if !outcome.assembler_status.warnings.is_empty() {
            ui.separator();
            ui.label("Assembler warnings");
            for warning in &outcome.assembler_status.warnings {
                ui.label(warning);
            }
        }

        ui.separator();
        ui.label("Compiled scenes");
        egui::ScrollArea::vertical().show(ui, |ui| {
            for scene in &outcome.compiled_scenes {
                ui.collapsing(
                    format!("{} ({} byte(s))", scene.scene_id, scene.byte_len),
                    |ui| {
                        if scene.metadata.is_empty() {
                            ui.label("No scene metadata recorded.");
                        } else {
                            for (key, value) in &scene.metadata {
                                ui.label(format!("{}: {}", key, value));
                            }
                        }
                    },
                );
            }
        });
    }

    fn draw_playtest_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
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
                        .add(
                            egui::DragValue::new(&mut edited.jump_buffer_frames).range(0..=16),
                        )
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
                    bundle.manifest.gameplay.physics_presets.push(duplicate.clone());
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
                        *target =
                            template_physics_profile(target.family, target.id.clone());
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
            self.reset_playtest_session();
        }

        let input = input_frame_from_context(ctx);
        if self.playtest_state.playing {
            self.playtest_state.accumulated_seconds +=
                ctx.input(|input| input.stable_dt) * self.playtest_state.speed_multiplier;
            while self.playtest_state.accumulated_seconds >= (1.0 / 60.0) {
                self.step_playtest_session(input);
                self.playtest_state.accumulated_seconds -= 1.0 / 60.0;
            }
            ctx.request_repaint();
        } else if step_session {
            self.step_playtest_session(input);
        }

        if !self.playtest_state.last_status.is_empty() {
            ui.separator();
            ui.label(&self.playtest_state.last_status);
        }

        if let Some(profile) = self.selected_physics_profile() {
            ui.separator();
            ui.collapsing("Movement Trace", |ui| {
                let trace = simulate_trace(
                    &profile,
                    &sample_platformer_trace_input(),
                );
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
            bundle_snapshot.scenes.get(self.selected_scene),
            self.playtest_state.session.as_ref().map(|session| session.state()),
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

            if let Some(scene) = bundle_snapshot.scenes.get(self.selected_scene) {
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
                    scene.entities.iter().filter(|entity| !entity.active).count()
                ));
            }
        } else {
            ui.label("Restart the session to begin simulating the current scene.");
        }

        if let Some(outcome) = &self.last_build_outcome {
            ui.separator();
            ui.label(format!("Last ROM path: {}", outcome.rom_path));
            if !outcome.rom_built {
                ui.label("A playable ROM was not produced yet. Build assets are still available for inspection.");
            }
        } else {
            ui.separator();
            ui.label("Build the project to generate a ROM and launch a playtest session.");
        }
    }

    fn draw_workspace_slot(&mut self, ui: &mut egui::Ui, area: DockArea, ctx: &egui::Context) {
        let slot = self.workspace.layout.slot(area).clone();
        let mut pending_active = None;
        let mut pending_show_tab = None;
        let mut pending_hide_tab = None;
        let mut pending_move_tab = None;
        let mut pending_reorder = None;

        ui.vertical(|ui| {
            ui.horizontal_wrapped(|ui| {
                for (index, tab) in slot.tabs.iter().enumerate() {
                    if ui
                        .selectable_label(slot.active == index, tab.label())
                        .clicked()
                    {
                        pending_active = Some(index);
                    }
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.menu_button("+", |ui| {
                        let mut any_hidden = false;
                        for tab in DockTab::ALL {
                            if self.workspace.layout.contains(tab) {
                                continue;
                            }
                            any_hidden = true;
                            if ui.button(tab.label()).clicked() {
                                pending_show_tab = Some(tab);
                                ui.close();
                            }
                        }
                        if !any_hidden {
                            ui.label("All dock tabs are visible.");
                        }
                    });

                    if let Some(active_tab) = slot.active_tab() {
                        ui.menu_button("Dock", |ui| {
                            for target in DockArea::ALL {
                                if ui
                                    .button(format!("Move to {}", target.label()))
                                    .clicked()
                                {
                                    pending_move_tab = Some((active_tab, target));
                                    ui.close();
                                }
                            }
                            ui.separator();
                            if ui.button("Hide Tab").clicked() {
                                pending_hide_tab = Some(active_tab);
                                ui.close();
                            }
                        });

                        if slot.tabs.len() > 1 {
                            if ui.small_button(">").clicked() {
                                pending_reorder = Some(1);
                            }
                            if ui.small_button("<").clicked() {
                                pending_reorder = Some(-1);
                            }
                        }
                    }
                });
            });
            ui.separator();

            match slot.active_tab() {
                Some(DockTab::Toolbox) => self.draw_toolbox_tab(ui),
                Some(DockTab::Scene) => self.draw_scene_tab(ui, ctx),
                Some(DockTab::Inspector) => self.draw_inspector_tab(ui, ctx),
                Some(DockTab::Outliner) => self.draw_scene_outliner(ui),
                Some(DockTab::Assets) => self.draw_asset_browser(ui, ctx),
                Some(DockTab::Animation) => self.draw_animation_tab(ui, ctx),
                Some(DockTab::Diagnostics) => self.draw_diagnostics(ui),
                Some(DockTab::BuildReport) => self.draw_build_report_tab(ui),
                Some(DockTab::Playtest) => self.draw_playtest_tab(ui, ctx),
                None => {
                    ui.with_layout(Layout::top_down_justified(Align::Min), |ui| {
                        ui.add_space(16.0);
                        ui.label(format!(
                            "{} dock is empty. Use View or the + menu to show a tab.",
                            area.label()
                        ));
                    });
                }
            }
        });

        if let Some(index) = pending_active {
            self.workspace.layout.set_active_tab(area, index);
            self.mark_workspace_custom();
        }
        if let Some(tab) = pending_show_tab {
            self.set_dock_tab_visibility(tab, true);
        }
        if let Some(tab) = pending_hide_tab {
            self.set_dock_tab_visibility(tab, false);
        }
        if let Some((tab, target)) = pending_move_tab {
            self.move_dock_tab(tab, target);
        }
        if let Some(direction) = pending_reorder {
            self.move_active_dock_tab_within_slot(area, direction);
        }
    }

    fn draw_workspace(&mut self, ctx: &egui::Context) {
        let previous_left = self.workspace.layout.left.size;
        let previous_right = self.workspace.layout.right.size;
        let previous_bottom = self.workspace.layout.bottom.size;
        let mut left_size = None;
        let mut right_size = None;
        let mut bottom_size = None;

        egui::CentralPanel::default().show(ctx, |ui| {
            if !self.workspace.layout.left.tabs.is_empty() {
                let panel = egui::SidePanel::left("workspace_left_panel")
                    .resizable(true)
                    .min_width(220.0)
                    .default_width(self.workspace.layout.left.size)
                    .show_inside(ui, |ui| {
                        self.draw_workspace_slot(ui, DockArea::Left, ctx);
                    });
                left_size = Some(panel.response.rect.width());
            }

            if !self.workspace.layout.right.tabs.is_empty() {
                let panel = egui::SidePanel::right("workspace_right_panel")
                    .resizable(true)
                    .min_width(260.0)
                    .default_width(self.workspace.layout.right.size)
                    .show_inside(ui, |ui| {
                        self.draw_workspace_slot(ui, DockArea::Right, ctx);
                    });
                right_size = Some(panel.response.rect.width());
            }

            if !self.workspace.layout.bottom.tabs.is_empty() {
                let panel = egui::TopBottomPanel::bottom("workspace_bottom_panel")
                    .resizable(true)
                    .min_height(180.0)
                    .default_height(self.workspace.layout.bottom.size)
                    .show_inside(ui, |ui| {
                        self.draw_workspace_slot(ui, DockArea::Bottom, ctx);
                    });
                bottom_size = Some(panel.response.rect.height());
            }

            self.draw_workspace_slot(ui, DockArea::Center, ctx);
        });

        let mut size_changed = false;
        if let Some(size) = left_size {
            if (size - previous_left).abs() > 0.5 {
                self.workspace.layout.set_slot_size(DockArea::Left, size);
                size_changed = true;
            }
        }
        if let Some(size) = right_size {
            if (size - previous_right).abs() > 0.5 {
                self.workspace.layout.set_slot_size(DockArea::Right, size);
                size_changed = true;
            }
        }
        if let Some(size) = bottom_size {
            if (size - previous_bottom).abs() > 0.5 {
                self.workspace.layout.set_slot_size(DockArea::Bottom, size);
                size_changed = true;
            }
        }
        if size_changed {
            self.mark_workspace_custom();
        }
    }

    fn draw_scene_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.bundle.is_none() {
            ui.vertical_centered(|ui| {
                ui.add_space(120.0);
                ui.heading("SNES Maker");
                ui.label("Open a project or create a new template to start building a SNES demo.");
                if ui.button("Open Project").clicked() {
                    self.open_project_dialog();
                }
                if ui.button("Create Template Project").clicked() {
                    self.new_project_state.open = true;
                }
            });
            return;
        }

        let (scene_label, entry_scene, active_layer_label, active_layer_locked, active_layer_hidden) =
            {
                let bundle = self.bundle.as_ref().expect("bundle");
                let scene = bundle.scenes.get(self.selected_scene);
                (
                    scene
                        .map(|scene| scene.id.clone())
                        .unwrap_or_else(|| "no_scene".to_string()),
                    bundle.manifest.gameplay.entry_scene.clone(),
                    scene
                        .and_then(|scene| scene.layers.get(self.selected_layer))
                        .map(|layer| layer.id.clone())
                        .unwrap_or_else(|| "no_layer".to_string()),
                    self.active_layer_locked(),
                    scene
                        .and_then(|scene| scene.layers.get(self.selected_layer))
                        .is_some_and(|layer| !layer.visible),
                )
            };

        ui.horizontal(|ui| {
            ui.heading("Scene Preview");
            ui.label(format!("{}  |  entry scene: {}", scene_label, entry_scene));
            ui.separator();
            ui.label(format!(
                "Layer: {}{}{}",
                active_layer_label,
                if active_layer_hidden { " [hidden]" } else { "" },
                if active_layer_locked { " [locked]" } else { "" }
            ));
            ui.separator();
            ui.label(format!("Tool: {}", self.tool.label()));
            ui.separator();
            ui.label("Select drags a box. Alt-click samples the active layer tile. Two-finger scroll pans. Pinch zooms. Cmd+C / Cmd+V copies and pastes selections.");
            ui.separator();
            if ui.button("Build ROM").clicked() {
                self.build_current_rom();
            }
        });

        ui.add_space(8.0);
        let viewport_height = (ui.available_height() - 96.0).max(220.0);
        let viewport_size = Vec2::new(ui.available_width(), viewport_height);
        let bundle = self.bundle.as_ref().expect("bundle");
        let content_size = scene_content_size(bundle, self.selected_scene, self.scene_zoom);
        if let Some(focus_rect) = self.pending_focus_rect.take() {
            self.scene_scroll_offset = focus_offset_for_rect(
                focus_rect,
                self.scene_zoom,
                viewport_size,
                content_size,
            );
        }
        self.scene_scroll_offset =
            clamp_scene_scroll_offset(self.scene_scroll_offset, content_size, viewport_size);
        let outcome = draw_scene_canvas(
            ui,
            bundle,
            self.selected_scene,
            self.scene_zoom,
            self.scene_scroll_offset,
            viewport_size,
            self.show_grid,
            self.show_collision,
            self.selected_layer,
            self.selected_tile,
            self.selected_spawn,
            self.selected_checkpoint,
            self.selected_entity,
            self.selected_trigger,
            self.selection.as_ref(),
            self.selection_drag_anchor,
            self.tool == EditorTool::Select,
            self.solo_layer,
            self.solo_group,
            true,
            true,
            true,
            true,
            ctx.input(|input| input.time) as f32,
        );
        self.apply_scene_view_gestures(ctx, outcome.viewport_rect, content_size);
        let clicked_outside_view = self.tool == EditorTool::Select
            && ctx.input(|input| input.pointer.primary_clicked())
            && ctx
                .input(|input| input.pointer.interact_pos())
                .is_some_and(|position| !outcome.viewport_rect.contains(position));
        if clicked_outside_view && (self.selection.is_some() || self.selection_drag_anchor.is_some())
        {
            self.clear_selection();
            self.status = "Selection cleared".to_string();
        }

        if let Some((x, y)) = outcome.hovered_tile {
            ui.label(format!("Hovered tile: ({}, {})", x, y));
        }

        if let Some(selection) = &self.selection {
            ui.add_space(6.0);
            ui.label(format!(
                "Selection Actions for {}x{} region",
                selection.rect.width_tiles(),
                selection.rect.height_tiles()
            ));
            ui.horizontal_wrapped(|ui| {
                if ui.button("Fill Tiles").clicked() {
                    self.apply_selection_action(SelectionAction::PaintTile(self.selected_tile as u16));
                }
                if ui.button("Clear Tiles").clicked() {
                    self.apply_selection_action(SelectionAction::PaintTile(0));
                }
                if ui.button("Solid On").clicked() {
                    self.apply_selection_action(SelectionAction::SetSolid(true));
                }
                if ui.button("Solid Off").clicked() {
                    self.apply_selection_action(SelectionAction::SetSolid(false));
                }
                if ui.button("Ladder On").clicked() {
                    self.apply_selection_action(SelectionAction::SetLadder(true));
                }
                if ui.button("Ladder Off").clicked() {
                    self.apply_selection_action(SelectionAction::SetLadder(false));
                }
                if ui.button("Hazard On").clicked() {
                    self.apply_selection_action(SelectionAction::SetHazard(true));
                }
                if ui.button("Hazard Off").clicked() {
                    self.apply_selection_action(SelectionAction::SetHazard(false));
                }
                if ui.button("Line").clicked() {
                    self.draw_line_in_selection();
                }
                if ui.button("Mirror H").clicked() {
                    self.mirror_selection(true);
                }
                if ui.button("Mirror V").clicked() {
                    self.mirror_selection(false);
                }
                if ui.button("Stamp Clipboard").clicked() {
                    self.paste_clipboard();
                }
                if ui.button("Flood Fill From Hover").clicked() {
                    self.flood_fill_from_hovered_tile();
                }
                if ui.button("Save Brush").clicked() {
                    self.save_selection_as_brush();
                }
                if ui.button("Save Snippet").clicked() {
                    self.save_selection_as_snippet();
                }
            });
        }

        ui.add_space(12.0);
        ui.collapsing("Workflow", |ui| {
            ui.label("1. Use Select to drag a region and copy or paste tiles and objects.");
            ui.label("2. Paint the stage with tiles, use selection actions for bulk edits, and mark solids, ladders, and hazards.");
            ui.label("3. Add spawns, checkpoints, entities, and triggers from the inspector.");
            ui.label("4. Import a sprite sheet to create new metasprites and animations.");
            ui.label("5. Save, then build the ROM.");
        });
        self.apply_canvas_outcome(outcome);
    }

    fn apply_scene_view_gestures(
        &mut self,
        ctx: &egui::Context,
        viewport_rect: Rect,
        content_size: Vec2,
    ) {
        let zoom_delta = ctx.input(|input| input.zoom_delta());
        let pan_delta = ctx.input(|input| input.smooth_scroll_delta);
        let Some(pointer_pos) = ctx.input(|input| input.pointer.latest_pos()) else {
            self.scene_scroll_offset = clamp_scene_scroll_offset(
                self.scene_scroll_offset,
                content_size,
                viewport_rect.size(),
            );
            return;
        };

        if !viewport_rect.contains(pointer_pos) {
            self.scene_scroll_offset = clamp_scene_scroll_offset(
                self.scene_scroll_offset,
                content_size,
                viewport_rect.size(),
            );
            return;
        }

        let mut next_offset = self.scene_scroll_offset - pan_delta;
        let previous_zoom = self.scene_zoom.max(SCENE_MIN_ZOOM);
        let next_zoom = (previous_zoom * zoom_delta).clamp(SCENE_MIN_ZOOM, SCENE_MAX_ZOOM);
        let mut next_content_size = content_size;

        if (next_zoom - previous_zoom).abs() >= f32::EPSILON {
            let pointer_local = pointer_pos - viewport_rect.min;
            let zoom_ratio = next_zoom / previous_zoom;
            next_content_size = content_size * zoom_ratio;
            next_offset = (next_offset + pointer_local) * zoom_ratio - pointer_local;
            self.scene_zoom = next_zoom;
            self.status = format!("Scene zoom: {:.1}x", self.scene_zoom);
            ctx.request_repaint();
        }

        self.scene_scroll_offset =
            clamp_scene_scroll_offset(next_offset, next_content_size, viewport_rect.size());
    }

    fn apply_canvas_outcome(&mut self, outcome: SceneCanvasOutcome) {
        self.last_canvas_tile = outcome.hovered_tile;

        if let Some(cell_index) = outcome.sampled_cell {
            self.sample_tile_from_cell(cell_index);
            self.active_canvas_cell = None;
            return;
        }

        if self.tool == EditorTool::Select {
            if let Some(start) = outcome.selection_started {
                self.selection_drag_anchor = Some(start);
            }
            if let Some(end) = outcome.selection_finished {
                let anchor = self.selection_drag_anchor.unwrap_or(end);
                self.commit_selection(TileSelectionRect::from_points(anchor, end));
            } else if let Some(tile) = outcome.selection_clicked {
                self.commit_selection(TileSelectionRect::from_points(tile, tile));
            }
            self.active_canvas_cell = None;
            return;
        }

        let Some(cell_index) = outcome.primary_cell.or(outcome.secondary_cell) else {
            self.active_canvas_cell = None;
            return;
        };

        if self.active_canvas_cell == Some(cell_index) {
            return;
        }

        let tool = self.tool;
        let selected_tile = self.selected_tile as u16;
        let selected_spawn = self.selected_spawn;
        let selected_checkpoint = self.selected_checkpoint;
        let selected_entity = self.selected_entity;
        let selected_trigger = self.selected_trigger;
        let scene_pos = outcome.world_cell_position;
        let collision_value = outcome.primary_cell.is_some();
        let active_layer_locked = self.active_layer_locked();

        enum PendingSceneEdit {
            PaintTile(u16),
            SetSolid(bool),
            SetLadder(bool),
            SetHazard(bool),
            MoveSpawn(usize, PointI16),
            MoveCheckpoint(usize, PointI16),
            MoveEntity(usize, PointI16),
            MoveTrigger(usize, PointI16),
        }

        let pending = match tool {
            EditorTool::Select => None,
            EditorTool::Paint => Some(PendingSceneEdit::PaintTile(selected_tile)),
            EditorTool::Erase => Some(PendingSceneEdit::PaintTile(0)),
            EditorTool::Solid => Some(PendingSceneEdit::SetSolid(collision_value)),
            EditorTool::Ladder => Some(PendingSceneEdit::SetLadder(collision_value)),
            EditorTool::Hazard => Some(PendingSceneEdit::SetHazard(collision_value)),
            EditorTool::Spawn => {
                selected_spawn.map(|index| PendingSceneEdit::MoveSpawn(index, scene_pos))
            }
            EditorTool::Checkpoint => {
                selected_checkpoint.map(|index| PendingSceneEdit::MoveCheckpoint(index, scene_pos))
            }
            EditorTool::Entity => {
                selected_entity.map(|index| PendingSceneEdit::MoveEntity(index, scene_pos))
            }
            EditorTool::Trigger => {
                selected_trigger.map(|index| PendingSceneEdit::MoveTrigger(index, scene_pos))
            }
        };

        if active_layer_locked && matches!(pending, Some(PendingSceneEdit::PaintTile(_))) {
            self.active_canvas_cell = None;
            self.status = self
                .current_layer()
                .map(|layer| format!("Layer '{}' is locked.", layer.id))
                .unwrap_or_else(|| "Active layer is locked.".to_string());
            return;
        }

        let mut edited = false;
        let mut status = None;
        if let Some(pending) = pending {
            self.capture_history();
            match pending {
                PendingSceneEdit::PaintTile(tile_index) => {
                    if let Some(layer) = self.current_layer_mut() {
                        if cell_index < layer.tiles.len() {
                            layer.tiles[cell_index] = tile_index;
                            edited = true;
                            status = Some(if tile_index == 0 {
                                format!("Erased tile from '{}'", layer.id)
                            } else {
                                format!("Painted tile {} into '{}'", tile_index, layer.id)
                            });
                        }
                    }
                }
                PendingSceneEdit::SetSolid(value) => {
                    if let Some(scene) = self.current_scene_mut() {
                        if cell_index < scene.collision.solids.len() {
                            scene.collision.solids[cell_index] = value;
                            edited = true;
                            status = Some("Updated solid collision".to_string());
                        }
                    }
                }
                PendingSceneEdit::SetLadder(value) => {
                    if let Some(scene) = self.current_scene_mut() {
                        if cell_index < scene.collision.ladders.len() {
                            scene.collision.ladders[cell_index] = value;
                            edited = true;
                            status = Some("Updated ladder collision".to_string());
                        }
                    }
                }
                PendingSceneEdit::SetHazard(value) => {
                    if let Some(scene) = self.current_scene_mut() {
                        if cell_index < scene.collision.hazards.len() {
                            scene.collision.hazards[cell_index] = value;
                            edited = true;
                            status = Some("Updated hazard collision".to_string());
                        }
                    }
                }
                PendingSceneEdit::MoveSpawn(index, position) => {
                    if let Some(scene) = self.current_scene_mut() {
                        if let Some(spawn) = scene.spawns.get_mut(index) {
                            spawn.position = position;
                            edited = true;
                            status = Some(format!("Moved spawn '{}'", spawn.id));
                        }
                    }
                }
                PendingSceneEdit::MoveCheckpoint(index, position) => {
                    if let Some(scene) = self.current_scene_mut() {
                        if let Some(checkpoint) = scene.checkpoints.get_mut(index) {
                            checkpoint.position = position;
                            edited = true;
                            status = Some(format!("Moved checkpoint '{}'", checkpoint.id));
                        }
                    }
                }
                PendingSceneEdit::MoveEntity(index, position) => {
                    if let Some(scene) = self.current_scene_mut() {
                        if let Some(entity) = scene.entities.get_mut(index) {
                            entity.position = position;
                            edited = true;
                            status = Some(format!("Moved entity '{}'", entity.id));
                        }
                    }
                }
                PendingSceneEdit::MoveTrigger(index, position) => {
                    if let Some(scene) = self.current_scene_mut() {
                        if let Some(trigger) = scene.triggers.get_mut(index) {
                            trigger.rect.x = position.x;
                            trigger.rect.y = position.y;
                            edited = true;
                            status = Some(format!("Moved trigger '{}'", trigger.id));
                        }
                    }
                }
            }
        }

        if edited {
            self.active_canvas_cell = Some(cell_index);
            self.mark_edited(status.unwrap_or_else(|| "Updated scene".to_string()));
        } else {
            self.active_canvas_cell = None;
        }
    }

    fn current_scene_mut(&mut self) -> Option<&mut SceneResource> {
        self.bundle
            .as_mut()
            .and_then(|bundle| bundle.scenes.get_mut(self.selected_scene))
    }

    fn draw_windows(&mut self, ctx: &egui::Context) {
        if self.show_help {
            let mut open = self.show_help;
            egui::Window::new("Help")
                .open(&mut open)
                .resizable(true)
                .show(ctx, |ui| {
                    ui.heading("Editor Shortcuts");
                    ui.label("Cmd+O: open project");
                    ui.label("Cmd+S: save");
                    ui.label("Cmd+C / Cmd+V: copy and paste the current selection");
                    ui.label("Cmd+Shift+F: fill the current selection with the selected tile");
                    ui.label("Cmd+I: import sprite sheet");
                    ui.label("Cmd+B: build ROM");
                    ui.label("Cmd+R: reload from disk");
                    ui.label("Cmd+Z / Cmd+Shift+Z: undo / redo");
                    ui.separator();
                    ui.heading("Scene Editing");
                    ui.label("Use Select to drag a rubber-band region around tiles and objects.");
                    ui.label("Alt-click the canvas to sample a tile from the active layer.");
                    ui.label("Use two-finger horizontal or vertical scrolling to pan around larger scenes, and pinch to zoom in or out.");
                    ui.label("Use the Paint tool with the tile browser to build the stage.");
                    ui.label("Use the selection action bar to fill or clear tiles and collision in bulk.");
                    ui.label("Use Solid, Ladder, and Hazard to mark collision directly on the map.");
                    ui.label("Use Spawn, Checkpoint, Entity, and Trigger to place gameplay markers.");
                    ui.separator();
                    ui.heading("Sprite Sheet Import");
                    ui.label("Store PNG sheets in the project-local sprite library, then load one into the importer and slice it into frames.");
                    ui.label("Imported frames become metasprites, and selected animations can now swap or reorder those metasprite frames in the inspector.");
                    ui.separator();
                    ui.heading("Current Scope");
                    ui.label("The editor now supports project loading, save/export, preview, tile painting, collision painting, object placement, tile pixel editing, sprite-sheet import, validation, and ROM builds.");
                    ui.label("Dialogue graphs and cutscene scripting are still authored in files for now.");
                });
            self.show_help = open;
        }

        if self.save_layout_state.open {
            let mut open = self.save_layout_state.open;
            let mut save_layout = false;
            let mut cancel = false;
            egui::Window::new("Save Workspace Layout")
                .open(&mut open)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Layout name");
                    ui.text_edit_singleline(&mut self.save_layout_state.name);
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            save_layout = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
            if save_layout {
                self.save_current_workspace_layout();
                open = false;
            }
            if cancel {
                open = false;
            }
            self.save_layout_state.open = open;
        }

        if self.import_state.open {
            let mut open = self.import_state.open;
            egui::Window::new("Import Sprite Sheet")
                .open(&mut open)
                .default_size([560.0, 680.0])
                .show(ctx, |ui| {
                    self.draw_import_window(ui, ctx);
                });
            self.import_state.open = open;
        }

        if self.new_project_state.open {
            let mut open = self.new_project_state.open;
            egui::Window::new("New Template Project")
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label("Project name");
                    ui.text_edit_singleline(&mut self.new_project_state.project_name);
                    ui.label("Destination");
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut self.new_project_state.destination);
                        if ui.button("Choose...").clicked() {
                            self.choose_new_project_destination();
                        }
                    });
                    if ui.button("Create").clicked() {
                        self.create_template_project();
                    }
                });
            self.new_project_state.open = open;
        }

        if self.confirm_exit {
            egui::Window::new("Unsaved Changes")
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label("This project has unsaved changes.");
                    ui.label("Do you want to save before closing?");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Save & Exit").clicked() {
                            self.save_project();
                            if !self.dirty {
                                self.confirm_exit = false;
                                ctx.send_viewport_cmd(ViewportCommand::Close);
                            }
                        }
                        if ui.button("Discard Changes").clicked() {
                            self.confirm_exit = false;
                            ctx.send_viewport_cmd(ViewportCommand::Close);
                        }
                        if ui.button("Cancel").clicked() {
                            self.confirm_exit = false;
                        }
                    });
                });
        }
    }

    fn draw_import_window(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
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

impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.run_shortcuts(ctx);
        self.handle_close_request(ctx);
        self.draw_menu_bar(ctx);
        self.draw_workspace(ctx);
        self.draw_windows(ctx);
        if self.needs_animation_repaint() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }
}

struct SceneCanvasOutcome {
    viewport_rect: Rect,
    hovered_tile: Option<(usize, usize)>,
    sampled_cell: Option<usize>,
    primary_cell: Option<usize>,
    secondary_cell: Option<usize>,
    world_cell_position: PointI16,
    selection_started: Option<(usize, usize)>,
    selection_finished: Option<(usize, usize)>,
    selection_clicked: Option<(usize, usize)>,
}

impl Default for SceneCanvasOutcome {
    fn default() -> Self {
        Self {
            viewport_rect: Rect::NOTHING,
            hovered_tile: None,
            sampled_cell: None,
            primary_cell: None,
            secondary_cell: None,
            world_cell_position: PointI16 { x: 0, y: 0 },
            selection_started: None,
            selection_finished: None,
            selection_clicked: None,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_scene_canvas(
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
    let Some(scene) = bundle.scenes.get(scene_index) else {
        ui.label("No scene selected.");
        return SceneCanvasOutcome::default();
    };
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
    let painter = ui.painter_at(viewport_rect);
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

    if let Some(selection) = current_selection {
        draw_scene_selection_overlay(&painter, rect, zoom, scene, selection);
    }

    let selected_rect =
        selected_tile_preview_rect(rect, scene, selected_layer, selected_tile, zoom);
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
        world_tile_from_pos(position, rect, scene, zoom).map(|(x, y, _)| (x, y))
    });
    let sampling_mode = !selection_mode && ui.input(|input| input.modifiers.alt);
    let sampled_cell = if sampling_mode && response.clicked_by(egui::PointerButton::Primary) {
        response.interact_pointer_pos().and_then(|position| {
            world_tile_from_pos(position, rect, scene, zoom).map(|(_, _, index)| index)
        })
    } else {
        None
    };

    let interact_tile = response.interact_pointer_pos().and_then(|position| {
        world_tile_from_pos(position, rect, scene, zoom).map(|(x, y, _)| (x, y))
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
                world_tile_from_pos(position, rect, scene, zoom).map(|(_, _, index)| index)
            })
        } else {
            None
        };

    let secondary_cell = if !selection_mode && ui.input(|input| input.pointer.secondary_down()) {
        hover_pos.and_then(|position| {
            world_tile_from_pos(position, rect, scene, zoom).map(|(_, _, index)| index)
        })
    } else {
        None
    };

    let world_cell_position = hover_pos
        .and_then(|position| world_tile_from_pos(position, rect, scene, zoom))
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

fn draw_animation_preview(
    ui: &mut egui::Ui,
    bundle: &ProjectBundle,
    animation_index: usize,
    time_seconds: f32,
) {
    let Some(animation) = bundle.animations.get(animation_index) else {
        ui.label("No animation selected.");
        return;
    };
    let Some(metasprite) = metasprite_for_animation_frame(bundle, animation, time_seconds) else {
        ui.label("Animation has no renderable frames.");
        return;
    };

    let Some(tileset) = find_tileset_for_metasprite(bundle, metasprite) else {
        ui.label("No tileset found for the selected metasprite.");
        return;
    };
    let Some(palette) = bundle.palette(&metasprite.palette_id) else {
        ui.label("No palette found for the selected metasprite.");
        return;
    };

    draw_metasprite_preview_canvas(ui, metasprite, tileset, palette, Facing::Right);
}

fn draw_entity_preview(
    ui: &mut egui::Ui,
    bundle: &ProjectBundle,
    entity: &EntityPlacement,
    time_seconds: f32,
) {
    let Some(metasprite) = metasprite_for_entity(bundle, entity, time_seconds) else {
        ui.label("Selected entity has no animation or metasprite preview.");
        return;
    };
    let Some(tileset) = find_tileset_for_metasprite(bundle, metasprite) else {
        ui.label("No tileset found for the selected entity preview.");
        return;
    };
    let Some(palette) = bundle.palette(&metasprite.palette_id) else {
        ui.label("No palette found for the selected entity preview.");
        return;
    };

    draw_metasprite_preview_canvas(ui, metasprite, tileset, palette, entity.facing);
}

fn draw_metasprite_preview_canvas(
    ui: &mut egui::Ui,
    metasprite: &MetaspriteResource,
    tileset: &TilesetResource,
    palette: &PaletteResource,
    facing: Facing,
) {
    let desired = Vec2::new(192.0, 192.0);
    let (response, painter) = ui.allocate_painter(desired, Sense::hover());
    painter.rect_filled(response.rect, 6.0, Color32::from_rgb(18, 26, 34));
    draw_metasprite(
        &painter,
        response.rect.center() - Vec2::new(32.0, 32.0),
        metasprite,
        tileset,
        palette,
        4.0,
        facing,
        true,
    );
}

fn draw_text_thumbnail(
    ui: &mut egui::Ui,
    title: &str,
    body: &str,
    accent: Color32,
) {
    let desired = Vec2::new(96.0, 56.0);
    let (response, painter) = ui.allocate_painter(desired, Sense::hover());
    painter.rect_filled(response.rect, 6.0, Color32::from_rgb(18, 26, 34));
    let accent_rect = Rect::from_min_size(response.rect.min, Vec2::new(4.0, response.rect.height()));
    painter.rect_filled(accent_rect, 4.0, accent);
    painter.text(
        response.rect.min + Vec2::new(10.0, 8.0),
        Align2::LEFT_TOP,
        title,
        FontId::proportional(12.0),
        Color32::WHITE,
    );
    painter.text(
        response.rect.min + Vec2::new(10.0, 26.0),
        Align2::LEFT_TOP,
        truncate_preview_text(body, 40),
        FontId::proportional(11.0),
        Color32::from_gray(190),
    );
}

fn dialogue_preview_text(dialogue: &DialogueGraph) -> String {
    dialogue
        .nodes
        .iter()
        .find(|node| node.id == dialogue.opening_node)
        .or_else(|| dialogue.nodes.first())
        .map(|node| {
            if node.speaker.trim().is_empty() {
                node.text.clone()
            } else {
                format!("{}: {}", node.speaker, node.text)
            }
        })
        .unwrap_or_else(|| "Empty dialogue".to_string())
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

fn script_preview_text(script: &EventScript) -> String {
    script
        .commands
        .first()
        .map(event_command_label)
        .map(|label| format!("{} command(s), starts with {}", script.commands.len(), label))
        .unwrap_or_else(|| "No commands".to_string())
}

fn truncate_preview_text(value: &str, max_chars: usize) -> String {
    let value = value.trim().replace('\n', " ");
    let mut chars = value.chars();
    let preview = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{}...", preview)
    } else if preview.is_empty() {
        "Empty".to_string()
    } else {
        preview
    }
}

fn draw_tile_button(
    ui: &mut egui::Ui,
    tile: &Tile8,
    palette: &PaletteResource,
    scale: f32,
    selected: bool,
) -> egui::Response {
    let size = Vec2::splat(8.0 * scale + 8.0);
    let (response, painter) = ui.allocate_painter(size, Sense::click());
    let tile_rect = response.rect.shrink(4.0);
    painter.rect_filled(
        response.rect,
        4.0,
        if selected {
            Color32::from_rgb(38, 56, 74)
        } else {
            Color32::from_rgb(22, 30, 40)
        },
    );
    draw_tile_pixels(&painter, tile_rect, tile, palette);
    painter.rect_stroke(
        response.rect,
        4.0,
        (
            1.0,
            if selected {
                Color32::from_rgb(244, 214, 92)
            } else {
                Color32::from_gray(48)
            },
        ),
        StrokeKind::Inside,
    );
    response
}

fn draw_asset_thumbnail_frame(ui: &mut egui::Ui, size: Vec2) -> (egui::Response, egui::Painter) {
    let (response, painter) = ui.allocate_painter(size, Sense::hover());
    painter.rect_filled(response.rect, 6.0, Color32::from_rgb(20, 28, 38));
    painter.rect_stroke(
        response.rect,
        6.0,
        (1.0, Color32::from_rgb(60, 74, 92)),
        StrokeKind::Inside,
    );
    (response, painter)
}

fn draw_palette_thumbnail(ui: &mut egui::Ui, palette: &PaletteResource) {
    let (response, painter) = draw_asset_thumbnail_frame(ui, Vec2::new(92.0, 32.0));
    let rect = response.rect.shrink2(Vec2::splat(4.0));
    let swatch_count = palette.colors.len().clamp(1, 8);
    let swatch_width = rect.width() / swatch_count as f32;

    for (index, color) in palette.colors.iter().take(swatch_count).enumerate() {
        let swatch = Rect::from_min_size(
            Pos2::new(rect.left() + swatch_width * index as f32, rect.top()),
            Vec2::new(swatch_width + 1.0, rect.height()),
        );
        painter.rect_filled(swatch, 2.0, to_color32(color));
    }

    if palette.colors.is_empty() {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Empty",
            FontId::proportional(12.0),
            Color32::from_rgb(160, 170, 180),
        );
    }
}

fn draw_tileset_thumbnail(ui: &mut egui::Ui, tileset: &TilesetResource, palette: &PaletteResource) {
    let (response, painter) = draw_asset_thumbnail_frame(ui, Vec2::new(92.0, 54.0));
    let rect = response.rect.shrink2(Vec2::splat(4.0));
    let columns = 2;
    let rows = 2;
    let cell_size = Vec2::new(rect.width() / columns as f32, rect.height() / rows as f32);

    for index in 0..columns * rows {
        let x = index % columns;
        let y = index / columns;
        let cell = Rect::from_min_size(
            Pos2::new(
                rect.left() + cell_size.x * x as f32,
                rect.top() + cell_size.y * y as f32,
            ),
            cell_size - Vec2::splat(2.0),
        );
        if let Some(tile) = tileset.tiles.get(index) {
            draw_tile_pixels(&painter, cell, tile, palette);
            painter.rect_stroke(
                cell,
                2.0,
                (1.0, Color32::from_rgb(64, 78, 96)),
                StrokeKind::Inside,
            );
        } else {
            painter.rect_filled(cell, 2.0, Color32::from_rgb(30, 38, 50));
        }
    }
}

fn draw_metasprite_thumbnail(
    ui: &mut egui::Ui,
    bundle: &ProjectBundle,
    metasprite: &MetaspriteResource,
) {
    let (response, painter) = draw_asset_thumbnail_frame(ui, Vec2::new(92.0, 54.0));
    let rect = response.rect.shrink(6.0);

    let Some(tileset) = find_tileset_for_metasprite(bundle, metasprite) else {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "No tileset",
            FontId::proportional(12.0),
            Color32::from_rgb(180, 160, 160),
        );
        return;
    };
    let Some(palette) = bundle.palette(&metasprite.palette_id) else {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "No palette",
            FontId::proportional(12.0),
            Color32::from_rgb(180, 160, 160),
        );
        return;
    };

    let width_pixels = metasprite
        .pieces
        .iter()
        .map(|piece| piece.x + 8)
        .max()
        .unwrap_or(8) as f32;
    let height_pixels = metasprite
        .pieces
        .iter()
        .map(|piece| piece.y + 8)
        .max()
        .unwrap_or(8) as f32;
    let zoom = (rect.width() / width_pixels.max(1.0)).min(rect.height() / height_pixels.max(1.0));
    let zoom = zoom.clamp(2.0, 4.0);
    let origin = rect.center() - Vec2::new(width_pixels * zoom * 0.5, height_pixels * zoom * 0.5);
    draw_metasprite(
        &painter,
        origin,
        metasprite,
        tileset,
        palette,
        zoom,
        Facing::Right,
        false,
    );
}

fn draw_animation_thumbnail(
    ui: &mut egui::Ui,
    bundle: &ProjectBundle,
    animation_index: usize,
    time_seconds: f32,
) {
    let (response, painter) = draw_asset_thumbnail_frame(ui, Vec2::new(92.0, 54.0));
    let rect = response.rect.shrink(6.0);
    let Some(animation) = bundle.animations.get(animation_index) else {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "No animation",
            FontId::proportional(12.0),
            Color32::from_rgb(180, 160, 160),
        );
        return;
    };

    let Some(metasprite) = metasprite_for_animation_frame(bundle, animation, time_seconds) else {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "No frames",
            FontId::proportional(12.0),
            Color32::from_rgb(180, 160, 160),
        );
        return;
    };
    let Some(tileset) = find_tileset_for_metasprite(bundle, metasprite) else {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "No tileset",
            FontId::proportional(12.0),
            Color32::from_rgb(180, 160, 160),
        );
        return;
    };
    let Some(palette) = bundle.palette(&metasprite.palette_id) else {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "No palette",
            FontId::proportional(12.0),
            Color32::from_rgb(180, 160, 160),
        );
        return;
    };

    let width_pixels = metasprite
        .pieces
        .iter()
        .map(|piece| piece.x + 8)
        .max()
        .unwrap_or(8) as f32;
    let height_pixels = metasprite
        .pieces
        .iter()
        .map(|piece| piece.y + 8)
        .max()
        .unwrap_or(8) as f32;
    let zoom = (rect.width() / width_pixels.max(1.0)).min(rect.height() / height_pixels.max(1.0));
    let zoom = zoom.clamp(2.0, 4.0);
    let origin = rect.center() - Vec2::new(width_pixels * zoom * 0.5, height_pixels * zoom * 0.5);
    draw_metasprite(
        &painter,
        origin,
        metasprite,
        tileset,
        palette,
        zoom,
        Facing::Right,
        false,
    );
}

fn draw_scene_snippet_thumbnail(
    ui: &mut egui::Ui,
    bundle: &ProjectBundle,
    snippet: &SavedSceneSnippet,
) {
    let (response, painter) = draw_asset_thumbnail_frame(ui, Vec2::new(92.0, 54.0));
    let rect = response.rect.shrink(4.0);
    let Some(layer) = snippet.layers.first() else {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Empty",
            FontId::proportional(12.0),
            Color32::from_rgb(170, 180, 190),
        );
        return;
    };
    let Some(tileset) = bundle.tileset(&layer.tileset_id) else {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "No tileset",
            FontId::proportional(12.0),
            Color32::from_rgb(180, 160, 160),
        );
        return;
    };
    let Some(palette) = bundle.palette(&tileset.palette_id) else {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "No palette",
            FontId::proportional(12.0),
            Color32::from_rgb(180, 160, 160),
        );
        return;
    };

    let grid_width = snippet.size_tiles.width.max(1) as usize;
    let grid_height = snippet.size_tiles.height.max(1) as usize;
    let tile_scale = Vec2::new(
        rect.width() / grid_width as f32 / 8.0,
        rect.height() / grid_height as f32 / 8.0,
    );
    let scale = tile_scale.x.min(tile_scale.y).clamp(1.0, 4.0);
    let tile_size = Vec2::splat(8.0 * scale);

    for y in 0..grid_height.min(4) {
        for x in 0..grid_width.min(4) {
            let index = y * grid_width + x;
            let Some(tile) = layer.tiles.get(index).and_then(|tile_index| {
                tileset.tiles.get(*tile_index as usize)
            }) else {
                continue;
            };
            let cell = Rect::from_min_size(
                Pos2::new(rect.left() + x as f32 * tile_size.x, rect.top() + y as f32 * tile_size.y),
                tile_size,
            );
            draw_tile_pixels(&painter, cell, tile, palette);
        }
    }
}

fn draw_tile_brush_thumbnail(ui: &mut egui::Ui, brush: &SavedTileBrush) {
    let (response, painter) = draw_asset_thumbnail_frame(ui, Vec2::new(92.0, 54.0));
    let rect = response.rect.shrink(4.0);
    if brush.tiles.is_empty() {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Empty",
            FontId::proportional(12.0),
            Color32::from_rgb(170, 180, 190),
        );
        return;
    }

    let cols = brush.size_tiles.width.max(1) as usize;
    let rows = brush.size_tiles.height.max(1) as usize;
    let cell_w = rect.width() / cols as f32;
    let cell_h = rect.height() / rows as f32;
    for y in 0..rows.min(4) {
        for x in 0..cols.min(4) {
            let index = y * cols + x;
            let value = brush.tiles.get(index).copied().unwrap_or_default();
            let hue = ((value as u32 * 37) % 255) as u8;
            let cell = Rect::from_min_size(
                Pos2::new(rect.left() + x as f32 * cell_w, rect.top() + y as f32 * cell_h),
                Vec2::new(cell_w - 2.0, cell_h - 2.0),
            );
            painter.rect_filled(cell, 2.0, Color32::from_rgb(60 + hue / 3, 70 + hue / 4, 100 + hue / 5));
            painter.rect_stroke(
                cell,
                2.0,
                (1.0, Color32::from_rgb(36, 48, 62)),
                StrokeKind::Inside,
            );
        }
    }
}

fn draw_sprite_source_thumbnail(
    ui: &mut egui::Ui,
    texture: &TextureHandle,
    label: &str,
) {
    let (response, painter) = draw_asset_thumbnail_frame(ui, Vec2::new(92.0, 54.0));
    let rect = response.rect.shrink(4.0);
    let image_rect = Rect::from_center_size(rect.center(), Vec2::new(rect.width(), rect.height()));
    painter.image(
        texture.id(),
        image_rect,
        Rect::from_min_size(Pos2::ZERO, texture.size_vec2()),
        Color32::WHITE,
    );
    painter.rect_stroke(
        rect,
        4.0,
        (1.0, Color32::from_rgb(74, 88, 108)),
        StrokeKind::Inside,
    );
    painter.text(
        rect.left_bottom() + Vec2::new(4.0, -4.0),
        Align2::LEFT_BOTTOM,
        label,
        FontId::proportional(10.0),
        Color32::from_white_alpha(220),
    );
}

fn draw_scene_thumbnail(ui: &mut egui::Ui, bundle: &ProjectBundle, scene: &SceneResource) {
    let (response, painter) = draw_asset_thumbnail_frame(ui, Vec2::new(92.0, 54.0));
    let rect = response.rect.shrink(4.0);
    let Some(layer) = scene.layers.first() else {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Empty",
            FontId::proportional(12.0),
            Color32::from_rgb(170, 180, 190),
        );
        return;
    };
    let Some(tileset) = bundle.tileset(&layer.tileset_id) else {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "No tileset",
            FontId::proportional(12.0),
            Color32::from_rgb(180, 160, 160),
        );
        return;
    };
    let Some(palette) = bundle.palette(&tileset.palette_id) else {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "No palette",
            FontId::proportional(12.0),
            Color32::from_rgb(180, 160, 160),
        );
        return;
    };

    let grid_width = scene.size_tiles.width.max(1) as usize;
    let grid_height = scene.size_tiles.height.max(1) as usize;
    let scale = (rect.width() / grid_width as f32 / 8.0)
        .min(rect.height() / grid_height as f32 / 8.0)
        .clamp(1.0, 3.0);
    let tile_size = Vec2::splat(8.0 * scale);
    for y in 0..grid_height.min(4) {
        for x in 0..grid_width.min(4) {
            let index = y * grid_width + x;
            let Some(tile) = layer.tiles.get(index).and_then(|tile_index| {
                tileset.tiles.get(*tile_index as usize)
            }) else {
                continue;
            };
            let cell = Rect::from_min_size(
                Pos2::new(rect.left() + x as f32 * tile_size.x, rect.top() + y as f32 * tile_size.y),
                tile_size,
            );
            draw_tile_pixels(&painter, cell, tile, palette);
        }
    }
}

fn draw_diagnostic_row(
    ui: &mut egui::Ui,
    diagnostic: &Diagnostic,
    has_quick_fix: bool,
    navigate_to: &mut Option<String>,
    quick_fix: &mut Option<Diagnostic>,
) {
    let color = match diagnostic.severity {
        Severity::Error => Color32::from_rgb(196, 72, 72),
        Severity::Warning => Color32::from_rgb(208, 160, 40),
    };

    ui.group(|ui| {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(color, format!("[{}]", diagnostic.code));
            if let Some(path) = &diagnostic.path {
                ui.monospace(path);
                if ui.small_button("Go").clicked() {
                    *navigate_to = Some(path.clone());
                }
            }
            if has_quick_fix && ui.small_button("Quick Fix").clicked() {
                *quick_fix = Some(diagnostic.clone());
            }
        });
        ui.label(&diagnostic.message);
    });
}

fn draw_tile_editor_grid(ui: &mut egui::Ui, tile: &mut Tile8, palette: &PaletteResource) -> bool {
    let cell_size = 22.0;
    let desired_size = Vec2::new(cell_size * 8.0, cell_size * 8.0);
    let (response, painter) = ui.allocate_painter(desired_size, Sense::click());
    let rect = response.rect;
    let mut changed = false;

    for y in 0..8 {
        for x in 0..8 {
            let index = y * 8 + x;
            let palette_index = tile.pixels.get(index).copied().unwrap_or_default() as usize;
            let color = palette
                .colors
                .get(palette_index)
                .map(to_color32)
                .unwrap_or(Color32::BLACK);
            let cell = Rect::from_min_size(
                rect.min + Vec2::new(x as f32 * cell_size, y as f32 * cell_size),
                Vec2::splat(cell_size),
            );
            painter.rect_filled(cell, 0.0, color);
            painter.rect_stroke(cell, 0.0, (1.0, Color32::from_gray(26)), StrokeKind::Inside);

            if response.clicked()
                && response
                    .interact_pointer_pos()
                    .is_some_and(|position| cell.contains(position))
            {
                let max_index = palette.colors.len().saturating_sub(1) as u8;
                if let Some(pixel) = tile.pixels.get_mut(index) {
                    *pixel = if max_index == 0 {
                        0
                    } else {
                        (*pixel + 1) % (max_index + 1)
                    };
                    changed = true;
                }
            }
        }
    }

    changed
}

fn draw_tile_pixels(painter: &egui::Painter, rect: Rect, tile: &Tile8, palette: &PaletteResource) {
    let pixel_size = Vec2::new(rect.width() / 8.0, rect.height() / 8.0);
    for y in 0..8 {
        for x in 0..8 {
            let index = y * 8 + x;
            let palette_index = tile.pixels.get(index).copied().unwrap_or_default() as usize;
            let color = palette
                .colors
                .get(palette_index)
                .map(to_color32)
                .unwrap_or(Color32::BLACK);
            let pixel_rect = Rect::from_min_size(
                rect.min + Vec2::new(pixel_size.x * x as f32, pixel_size.y * y as f32),
                pixel_size,
            );
            painter.rect_filled(pixel_rect, 0.0, color);
        }
    }
}

fn draw_spawns(
    painter: &egui::Painter,
    rect: Rect,
    zoom: f32,
    spawns: &[SpawnPoint],
    selected: Option<usize>,
    color: Color32,
) {
    for (index, spawn) in spawns.iter().enumerate() {
        let center = rect.min
            + Vec2::new(
                spawn.position.x as f32 * zoom,
                spawn.position.y as f32 * zoom,
            );
        let radius = if selected == Some(index) { 7.0 } else { 5.0 };
        painter.circle_filled(center, radius, color);
        painter.text(
            center + Vec2::new(8.0, -10.0),
            Align2::LEFT_TOP,
            &spawn.id,
            FontId::proportional(12.0),
            color,
        );
    }
}

fn draw_checkpoints(
    painter: &egui::Painter,
    rect: Rect,
    zoom: f32,
    checkpoints: &[Checkpoint],
    selected: Option<usize>,
    color: Color32,
) {
    for (index, checkpoint) in checkpoints.iter().enumerate() {
        let min = rect.min
            + Vec2::new(
                checkpoint.position.x as f32 * zoom,
                checkpoint.position.y as f32 * zoom,
            );
        let marker = Rect::from_min_size(
            min,
            Vec2::splat(if selected == Some(index) { 14.0 } else { 10.0 }),
        );
        painter.rect_filled(marker, 0.0, color);
        painter.text(
            marker.max + Vec2::new(4.0, -10.0),
            Align2::LEFT_TOP,
            &checkpoint.id,
            FontId::proportional(12.0),
            color,
        );
    }
}

fn draw_triggers(
    painter: &egui::Painter,
    rect: Rect,
    zoom: f32,
    triggers: &[TriggerVolume],
    selected: Option<usize>,
) {
    for (index, trigger) in triggers.iter().enumerate() {
        let trigger_rect = Rect::from_min_size(
            rect.min + Vec2::new(trigger.rect.x as f32 * zoom, trigger.rect.y as f32 * zoom),
            Vec2::new(
                trigger.rect.width as f32 * zoom,
                trigger.rect.height as f32 * zoom,
            ),
        );
        let fill = if selected == Some(index) {
            Color32::from_rgba_premultiplied(255, 208, 72, 44)
        } else {
            Color32::from_rgba_premultiplied(148, 208, 255, 36)
        };
        let stroke = if selected == Some(index) {
            Color32::from_rgb(255, 220, 96)
        } else {
            Color32::from_rgb(112, 176, 232)
        };
        painter.rect_filled(trigger_rect, 0.0, fill);
        painter.rect_stroke(trigger_rect, 0.0, (2.0, stroke), StrokeKind::Inside);
        painter.text(
            trigger_rect.min + Vec2::new(4.0, 4.0),
            Align2::LEFT_TOP,
            &trigger.id,
            FontId::proportional(12.0),
            stroke,
        );
    }
}

fn draw_entities(
    painter: &egui::Painter,
    rect: Rect,
    zoom: f32,
    bundle: &ProjectBundle,
    entities: &[EntityPlacement],
    selected: Option<usize>,
    time_seconds: f32,
) {
    for (index, entity) in entities.iter().enumerate() {
        let origin = rect.min
            + Vec2::new(
                entity.position.x as f32 * zoom,
                entity.position.y as f32 * zoom,
            );
        if let Some(metasprite) = metasprite_for_entity(bundle, entity, time_seconds) {
            if let Some(tileset) = find_tileset_for_metasprite(bundle, metasprite) {
                if let Some(palette) = bundle.palette(&metasprite.palette_id) {
                    draw_metasprite(
                        painter,
                        origin,
                        metasprite,
                        tileset,
                        palette,
                        zoom,
                        entity.facing,
                        selected == Some(index),
                    );
                    continue;
                }
            }
        }

        let fallback = Rect::from_min_size(origin, Vec2::new(16.0 * zoom / 2.0, 16.0 * zoom / 2.0));
        let fill = if selected == Some(index) {
            Color32::from_rgb(255, 200, 80)
        } else {
            Color32::from_rgb(132, 168, 212)
        };
        painter.rect_filled(fallback, 2.0, fill);
        painter.text(
            fallback.max + Vec2::new(4.0, -10.0),
            Align2::LEFT_TOP,
            &entity.id,
            FontId::proportional(12.0),
            fill,
        );
    }
}

fn draw_scene_selection_overlay(
    painter: &egui::Painter,
    rect: Rect,
    zoom: f32,
    scene: &SceneResource,
    selection: &SceneSelection,
) {
    draw_tile_selection_rect(
        painter,
        rect,
        zoom,
        selection.rect,
        Color32::from_rgba_premultiplied(255, 224, 96, 28),
        Color32::from_rgb(255, 232, 120),
    );

    for index in &selection.spawns {
        if let Some(spawn) = scene.spawns.get(*index) {
            let center = rect.min
                + Vec2::new(
                    spawn.position.x as f32 * zoom,
                    spawn.position.y as f32 * zoom,
                );
            painter.circle_stroke(center, 9.0, (2.0, Color32::WHITE));
        }
    }

    for index in &selection.checkpoints {
        if let Some(checkpoint) = scene.checkpoints.get(*index) {
            let marker = Rect::from_min_size(
                rect.min
                    + Vec2::new(
                        checkpoint.position.x as f32 * zoom,
                        checkpoint.position.y as f32 * zoom,
                    ),
                Vec2::splat(16.0),
            );
            painter.rect_stroke(marker, 0.0, (2.0, Color32::WHITE), StrokeKind::Outside);
        }
    }

    for index in &selection.entities {
        if let Some(entity) = scene.entities.get(*index) {
            let marker = Rect::from_min_size(
                rect.min
                    + Vec2::new(
                        entity.position.x as f32 * zoom,
                        entity.position.y as f32 * zoom,
                    ),
                Vec2::splat(16.0 * zoom.max(1.0) / 2.0),
            );
            painter.rect_stroke(marker, 2.0, (2.0, Color32::WHITE), StrokeKind::Outside);
        }
    }

    for index in &selection.triggers {
        if let Some(trigger) = scene.triggers.get(*index) {
            let trigger_rect = Rect::from_min_size(
                rect.min + Vec2::new(trigger.rect.x as f32 * zoom, trigger.rect.y as f32 * zoom),
                Vec2::new(
                    trigger.rect.width as f32 * zoom,
                    trigger.rect.height as f32 * zoom,
                ),
            );
            painter.rect_stroke(
                trigger_rect,
                0.0,
                (2.5, Color32::WHITE),
                StrokeKind::Outside,
            );
        }
    }
}

fn draw_tile_selection_rect(
    painter: &egui::Painter,
    rect: Rect,
    zoom: f32,
    selection_rect: TileSelectionRect,
    fill: Color32,
    stroke: Color32,
) {
    let selection_screen_rect = Rect::from_min_size(
        rect.min
            + Vec2::new(
                selection_rect.min_x as f32 * 8.0 * zoom,
                selection_rect.min_y as f32 * 8.0 * zoom,
            ),
        Vec2::new(
            selection_rect.width_tiles() as f32 * 8.0 * zoom,
            selection_rect.height_tiles() as f32 * 8.0 * zoom,
        ),
    );
    painter.rect_filled(selection_screen_rect, 0.0, fill);
    painter.rect_stroke(
        selection_screen_rect,
        0.0,
        (2.0, stroke),
        StrokeKind::Outside,
    );
}

fn draw_metasprite(
    painter: &egui::Painter,
    origin: Pos2,
    metasprite: &MetaspriteResource,
    tileset: &TilesetResource,
    palette: &PaletteResource,
    zoom: f32,
    facing: Facing,
    highlight: bool,
) {
    let width_pixels = metasprite
        .pieces
        .iter()
        .map(|piece| piece.x + 8)
        .max()
        .unwrap_or(8) as f32;

    for piece in &metasprite.pieces {
        let Some(tile) = tileset.tiles.get(piece.tile_index as usize) else {
            continue;
        };

        let flip_h = piece.h_flip ^ matches!(facing, Facing::Left);
        let draw_x = if flip_h {
            width_pixels - (piece.x as f32 + 8.0)
        } else {
            piece.x as f32
        };
        let tile_rect = Rect::from_min_size(
            origin + Vec2::new(draw_x * zoom, piece.y as f32 * zoom),
            Vec2::splat(8.0 * zoom),
        );
        draw_sprite_tile_pixels(painter, tile_rect, tile, palette, flip_h, piece.v_flip);
        if highlight {
            painter.rect_stroke(
                tile_rect,
                0.0,
                (1.0, Color32::from_white_alpha(80)),
                StrokeKind::Inside,
            );
        }
    }
}

fn template_physics_profile(
    family: snesmaker_project::PhysicsFamily,
    id: String,
) -> PhysicsProfile {
    match family {
        snesmaker_project::PhysicsFamily::MegaManLike
        | snesmaker_project::PhysicsFamily::Custom => PhysicsProfile {
            id,
            family,
            gravity_fp: snesmaker_project::fp(0.28),
            max_fall_speed_fp: snesmaker_project::fp(4.0),
            ground_accel_fp: snesmaker_project::fp(0.35),
            air_accel_fp: snesmaker_project::fp(0.22),
            max_run_speed_fp: snesmaker_project::fp(1.75),
            jump_velocity_fp: snesmaker_project::fp(-4.1),
            coyote_frames: 4,
            jump_buffer_frames: 4,
            ladder_speed_fp: snesmaker_project::fp(1.0),
        },
        snesmaker_project::PhysicsFamily::MarioLike => PhysicsProfile {
            id,
            family,
            gravity_fp: snesmaker_project::fp(0.22),
            max_fall_speed_fp: snesmaker_project::fp(4.8),
            ground_accel_fp: snesmaker_project::fp(0.45),
            air_accel_fp: snesmaker_project::fp(0.28),
            max_run_speed_fp: snesmaker_project::fp(2.2),
            jump_velocity_fp: snesmaker_project::fp(-4.8),
            coyote_frames: 6,
            jump_buffer_frames: 5,
            ladder_speed_fp: snesmaker_project::fp(1.2),
        },
    }
}

fn input_frame_from_context(ctx: &egui::Context) -> InputFrame {
    ctx.input(|input| InputFrame {
        left: input.key_down(Key::ArrowLeft) || input.key_down(Key::A),
        right: input.key_down(Key::ArrowRight) || input.key_down(Key::D),
        jump_pressed: input.key_pressed(Key::Space),
        jump_held: input.key_down(Key::Space),
        climb_up: input.key_down(Key::ArrowUp) || input.key_down(Key::W),
        climb_down: input.key_down(Key::ArrowDown) || input.key_down(Key::S),
    })
}

fn sample_platformer_trace_input() -> Vec<InputFrame> {
    let mut frames = Vec::new();
    for index in 0..48 {
        frames.push(InputFrame {
            right: index < 36,
            jump_pressed: index == 8,
            jump_held: (8..18).contains(&index),
            ..InputFrame::default()
        });
    }
    frames
}

fn draw_trace_chart(
    ui: &mut egui::Ui,
    trace: &[snesmaker_platformer::TraceFrame],
    label: &str,
    sample: impl Fn(&snesmaker_platformer::TraceFrame) -> i32,
) {
    ui.label(label);
    let desired = Vec2::new(ui.available_width(), 120.0);
    let (response, painter) = ui.allocate_painter(desired, Sense::hover());
    painter.rect_filled(response.rect, 6.0, Color32::from_rgb(18, 26, 34));
    if trace.len() < 2 {
        return;
    }

    let values = trace.iter().map(sample).collect::<Vec<_>>();
    let min = *values.iter().min().unwrap_or(&0) as f32;
    let max = *values.iter().max().unwrap_or(&0) as f32;
    let range = (max - min).max(1.0);

    for index in 1..values.len() {
        let prev_t = (index - 1) as f32 / (values.len().saturating_sub(1)) as f32;
        let next_t = index as f32 / (values.len().saturating_sub(1)) as f32;
        let prev_y = (values[index - 1] as f32 - min) / range;
        let next_y = (values[index] as f32 - min) / range;
        let prev = Pos2::new(
            egui::lerp(response.rect.x_range(), prev_t),
            egui::lerp(response.rect.y_range(), 1.0 - prev_y),
        );
        let next = Pos2::new(
            egui::lerp(response.rect.x_range(), next_t),
            egui::lerp(response.rect.y_range(), 1.0 - next_y),
        );
        painter.line_segment([prev, next], (2.0, Color32::from_rgb(96, 208, 255)));
    }
}

fn rects_overlap_pixels(a: RectI16, b: RectI16) -> bool {
    let a_right = a.x.saturating_add(a.width as i16);
    let a_bottom = a.y.saturating_add(a.height as i16);
    let b_right = b.x.saturating_add(b.width as i16);
    let b_bottom = b.y.saturating_add(b.height as i16);
    a.x < b_right && a_right > b.x && a.y < b_bottom && a_bottom > b.y
}

fn draw_sprite_tile_pixels(
    painter: &egui::Painter,
    rect: Rect,
    tile: &Tile8,
    palette: &PaletteResource,
    flip_h: bool,
    flip_v: bool,
) {
    let pixel_size = Vec2::new(rect.width() / 8.0, rect.height() / 8.0);
    for dest_y in 0..8 {
        for dest_x in 0..8 {
            let src_x = if flip_h { 7 - dest_x } else { dest_x };
            let src_y = if flip_v { 7 - dest_y } else { dest_y };
            let index = src_y * 8 + src_x;
            let palette_index = tile.pixels.get(index).copied().unwrap_or_default() as usize;
            if palette_index == 0 {
                continue;
            }
            let color = palette
                .colors
                .get(palette_index)
                .map(to_color32)
                .unwrap_or(Color32::TRANSPARENT);
            let pixel_rect = Rect::from_min_size(
                rect.min + Vec2::new(dest_x as f32 * pixel_size.x, dest_y as f32 * pixel_size.y),
                pixel_size,
            );
            painter.rect_filled(pixel_rect, 0.0, color);
        }
    }
}

fn build_scene_selection(scene: &SceneResource, rect: TileSelectionRect) -> SceneSelection {
    SceneSelection {
        rect,
        spawns: scene
            .spawns
            .iter()
            .enumerate()
            .filter_map(|(index, spawn)| {
                rect.contains_point_pixels(spawn.position).then_some(index)
            })
            .collect(),
        checkpoints: scene
            .checkpoints
            .iter()
            .enumerate()
            .filter_map(|(index, checkpoint)| {
                rect.contains_point_pixels(checkpoint.position)
                    .then_some(index)
            })
            .collect(),
        entities: scene
            .entities
            .iter()
            .enumerate()
            .filter_map(|(index, entity)| {
                rect.contains_point_pixels(entity.position).then_some(index)
            })
            .collect(),
        triggers: scene
            .triggers
            .iter()
            .enumerate()
            .filter_map(|(index, trigger)| {
                rect.intersects_rect_pixels(trigger.rect).then_some(index)
            })
            .collect(),
    }
}

fn layer_bounds_pixels(scene: &SceneResource, layer: &TileLayer) -> Option<RectI16> {
    let width = scene.size_tiles.width as usize;
    if width == 0 {
        return None;
    }

    let mut min_x = usize::MAX;
    let mut min_y = usize::MAX;
    let mut max_x = 0_usize;
    let mut max_y = 0_usize;
    let mut found = false;

    for (index, tile) in layer.tiles.iter().enumerate() {
        if *tile == 0 {
            continue;
        }
        let tile_x = index % width;
        let tile_y = index / width;
        min_x = min_x.min(tile_x);
        min_y = min_y.min(tile_y);
        max_x = max_x.max(tile_x);
        max_y = max_y.max(tile_y);
        found = true;
    }

    found.then_some(RectI16 {
        x: (min_x * 8) as i16,
        y: (min_y * 8) as i16,
        width: ((max_x - min_x + 1) * 8) as u16,
        height: ((max_y - min_y + 1) * 8) as u16,
    })
}

fn bresenham_line(start: (i32, i32), end: (i32, i32)) -> Vec<(i32, i32)> {
    let mut points = Vec::new();
    let (mut x0, mut y0) = start;
    let (x1, y1) = end;
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut error = dx + dy;

    loop {
        points.push((x0, y0));
        if x0 == x1 && y0 == y1 {
            break;
        }
        let doubled = 2 * error;
        if doubled >= dy {
            error += dy;
            x0 += sx;
        }
        if doubled <= dx {
            error += dx;
            y0 += sy;
        }
    }

    points
}

fn next_unique_layer_id(existing: &BTreeSet<String>, base: &str) -> String {
    let stem = format!("{}_copy", slugify(base));
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

fn next_unique_copy_id(existing: &mut BTreeSet<String>, base: &str) -> String {
    let stem = format!("{}_copy", slugify(base));
    if existing.insert(stem.clone()) {
        return stem;
    }

    let mut suffix = 2;
    loop {
        let candidate = format!("{}_{}", stem, suffix);
        if existing.insert(candidate.clone()) {
            return candidate;
        }
        suffix += 1;
    }
}

fn clamp_scene_scroll_offset(offset: Vec2, content_size: Vec2, viewport_size: Vec2) -> Vec2 {
    let max_x = (content_size.x - viewport_size.x).max(0.0);
    let max_y = (content_size.y - viewport_size.y).max(0.0);
    Vec2::new(offset.x.clamp(0.0, max_x), offset.y.clamp(0.0, max_y))
}

fn focus_offset_for_rect(
    rect: RectI16,
    zoom: f32,
    viewport_size: Vec2,
    content_size: Vec2,
) -> Vec2 {
    let center = Vec2::new(
        rect.x as f32 * zoom + rect.width as f32 * zoom * 0.5,
        rect.y as f32 * zoom + rect.height as f32 * zoom * 0.5,
    );
    clamp_scene_scroll_offset(center - viewport_size * 0.5, content_size, viewport_size)
}

fn scene_content_size(bundle: &ProjectBundle, scene_index: usize, zoom: f32) -> Vec2 {
    let Some(scene) = bundle.scenes.get(scene_index) else {
        return Vec2::ZERO;
    };
    let cell_size = 8.0 * zoom;
    Vec2::new(
        scene.size_tiles.width as f32 * cell_size,
        scene.size_tiles.height as f32 * cell_size,
    )
}

fn world_tile_from_pos(
    pos: Pos2,
    rect: Rect,
    scene: &SceneResource,
    zoom: f32,
) -> Option<(usize, usize, usize)> {
    if !rect.contains(pos) {
        return None;
    }
    let local = pos - rect.min;
    let tile_x = (local.x / (8.0 * zoom)).floor() as usize;
    let tile_y = (local.y / (8.0 * zoom)).floor() as usize;
    if tile_x >= scene.size_tiles.width as usize || tile_y >= scene.size_tiles.height as usize {
        return None;
    }
    let index = tile_y * scene.size_tiles.width as usize + tile_x;
    Some((tile_x, tile_y, index))
}

fn selected_tile_preview_rect(
    rect: Rect,
    scene: &SceneResource,
    selected_layer: usize,
    selected_tile: usize,
    zoom: f32,
) -> Option<Rect> {
    let layer = scene.layers.get(selected_layer)?;
    let index = layer
        .tiles
        .iter()
        .position(|tile| *tile as usize == selected_tile)?;
    let width = scene.size_tiles.width as usize;
    let tile_x = index % width;
    let tile_y = index / width;
    Some(Rect::from_min_size(
        rect.min + Vec2::new(tile_x as f32 * 8.0 * zoom, tile_y as f32 * 8.0 * zoom),
        Vec2::splat(8.0 * zoom),
    ))
}

fn metasprite_for_animation_frame<'a>(
    bundle: &'a ProjectBundle,
    animation: &'a AnimationResource,
    time_seconds: f32,
) -> Option<&'a MetaspriteResource> {
    if animation.frames.is_empty() {
        return None;
    }
    let total_frames = animation
        .frames
        .iter()
        .map(|frame| frame.duration_frames.max(1) as u32)
        .sum::<u32>()
        .max(1);
    let tick = ((time_seconds * 60.0) as u32) % total_frames;
    let mut cursor = 0_u32;
    for frame in &animation.frames {
        let duration = frame.duration_frames.max(1) as u32;
        if tick < cursor + duration {
            return bundle.metasprite(&frame.metasprite_id);
        }
        cursor += duration;
    }
    bundle
        .animation(&animation.id)
        .and_then(|animation| animation.frames.first())
        .and_then(|frame| bundle.metasprite(&frame.metasprite_id))
}

fn metasprite_for_entity<'a>(
    bundle: &'a ProjectBundle,
    entity: &'a EntityPlacement,
    time_seconds: f32,
) -> Option<&'a MetaspriteResource> {
    if let Some(animation) = bundle.animation(&entity.archetype) {
        return metasprite_for_animation_frame(bundle, animation, time_seconds);
    }
    if let Some(metasprite) = bundle.metasprite(&entity.archetype) {
        return Some(metasprite);
    }
    let idle_id = format!("{}_idle", entity.archetype);
    bundle
        .animation(&idle_id)
        .and_then(|animation| metasprite_for_animation_frame(bundle, animation, time_seconds))
}

fn entity_has_animation(bundle: &ProjectBundle, entity: &EntityPlacement) -> bool {
    bundle.animation(&entity.archetype).is_some()
        || bundle
            .animation(&format!("{}_idle", entity.archetype))
            .is_some()
}

fn find_tileset_for_metasprite<'a>(
    bundle: &'a ProjectBundle,
    metasprite: &MetaspriteResource,
) -> Option<&'a TilesetResource> {
    let max_tile_index = metasprite
        .pieces
        .iter()
        .map(|piece| piece.tile_index)
        .max()?;
    bundle.tilesets.iter().find(|tileset| {
        tileset.palette_id == metasprite.palette_id && tileset.tiles.len() > max_tile_index as usize
    })
}

fn sanitize_optional_index(index: Option<usize>, len: usize) -> Option<usize> {
    if len == 0 {
        None
    } else {
        Some(index.unwrap_or(0).min(len - 1))
    }
}

fn unique_project_sprite_destination(project_dir: &Utf8Path, source_path: &Path) -> Utf8PathBuf {
    let stem = source_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(slugify)
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| "sprite_sheet".to_string());
    let extension = source_path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .filter(|extension| !extension.is_empty())
        .unwrap_or_else(|| "png".to_string());

    let mut counter = 0_usize;
    loop {
        let file_name = if counter == 0 {
            format!("{stem}.{extension}")
        } else {
            format!("{stem}_{counter:02}.{extension}")
        };
        let candidate = project_dir.join(file_name);
        if !candidate.exists() {
            return candidate;
        }
        counter += 1;
    }
}

fn load_sheet_preview(ctx: &egui::Context, path: &Path) -> Result<LoadedSheetPreview> {
    let image = image::open(path)
        .with_context(|| format!("failed to open sprite sheet {}", path.display()))?
        .to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let rgba = image.into_raw();
    let texture = ctx.load_texture(
        format!("sheet-preview-{}", path.display()),
        ColorImage::from_rgba_unmultiplied(size, &rgba),
        TextureOptions::NEAREST,
    );

    Ok(LoadedSheetPreview {
        rgba,
        size,
        texture,
    })
}

fn import_sprite_sheet_into_bundle(
    bundle: &mut ProjectBundle,
    state: &SpriteSheetImportState,
    rgba: &[u8],
    size: [usize; 2],
) -> Result<String> {
    if state.base_id.trim().is_empty() || state.animation_id.trim().is_empty() {
        bail!("Base id and animation id are required.");
    }
    if state.frame_width_px == 0
        || state.frame_height_px == 0
        || state.frame_width_px % 8 != 0
        || state.frame_height_px % 8 != 0
    {
        bail!("Frame width and height must be non-zero multiples of 8 pixels.");
    }
    if state.frame_count == 0 || state.columns == 0 {
        bail!("Frame count and columns must be greater than zero.");
    }

    let sheet = RgbaImage::from_raw(size[0] as u32, size[1] as u32, rgba.to_vec())
        .ok_or_else(|| anyhow!("failed to decode sprite sheet pixels"))?;
    let frame_width_px = state.frame_width_px;
    let frame_height_px = state.frame_height_px;

    let mut reserved_ids = bundle
        .unique_ids()
        .into_iter()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    if reserved_ids.contains(state.animation_id.as_str()) {
        bail!(
            "Animation id '{}' already exists. Choose a unique id before importing.",
            state.animation_id
        );
    }

    let tileset_index = bundle
        .tilesets
        .iter()
        .position(|tileset| tileset.id == state.target_tileset_id)
        .ok_or_else(|| anyhow!("target tileset '{}' is missing", state.target_tileset_id))?;
    let palette_index = bundle
        .palettes
        .iter()
        .position(|palette| palette.id == state.target_palette_id)
        .ok_or_else(|| anyhow!("target palette '{}' is missing", state.target_palette_id))?;

    reserved_ids.insert(state.animation_id.clone());
    let palette_id = bundle
        .palettes
        .get(palette_index)
        .ok_or_else(|| anyhow!("palette index out of range"))?
        .id
        .clone();

    let (new_metasprites, animation_frames) = {
        let palette = bundle
            .palettes
            .get_mut(palette_index)
            .ok_or_else(|| anyhow!("palette index out of range"))?;
        let tileset = bundle
            .tilesets
            .get_mut(tileset_index)
            .ok_or_else(|| anyhow!("tileset index out of range"))?;

        let mut metasprites = Vec::with_capacity(state.frame_count);
        let mut frames = Vec::with_capacity(state.frame_count);
        for frame_index in 0..state.frame_count {
            let metasprite_id = if state.frame_count == 1 {
                state.base_id.clone()
            } else {
                format!("{}_{:02}", state.base_id, frame_index + 1)
            };
            if reserved_ids.contains(metasprite_id.as_str()) {
                bail!(
                    "Metasprite id '{}' already exists. Choose a different base id.",
                    metasprite_id
                );
            }
            reserved_ids.insert(metasprite_id.clone());

            let frame_x = (frame_index % state.columns) as u32 * frame_width_px;
            let frame_y = (frame_index / state.columns) as u32 * frame_height_px;
            if frame_x + frame_width_px > sheet.width()
                || frame_y + frame_height_px > sheet.height()
            {
                bail!(
                    "Frame {} exceeds the sprite sheet bounds. Check the frame size, count, or column count.",
                    frame_index + 1
                );
            }

            let frame_tiles_x = frame_width_px / 8;
            let frame_tiles_y = frame_height_px / 8;
            let mut pieces = Vec::with_capacity((frame_tiles_x * frame_tiles_y) as usize);

            for tile_y in 0..frame_tiles_y {
                for tile_x in 0..frame_tiles_x {
                    let tile = extract_tile_from_sheet(
                        &sheet,
                        palette,
                        frame_x + tile_x * 8,
                        frame_y + tile_y * 8,
                    )?;
                    let tile_index = tileset.tiles.len() as u16;
                    tileset.tiles.push(tile);
                    pieces.push(SpriteTileRef {
                        tile_index,
                        x: (tile_x * 8) as i16,
                        y: (tile_y * 8) as i16,
                        palette_slot: 0,
                        h_flip: false,
                        v_flip: false,
                    });
                }
            }

            metasprites.push(MetaspriteResource {
                id: metasprite_id.clone(),
                palette_id: palette_id.clone(),
                pieces,
            });
            frames.push(AnimationFrame {
                metasprite_id,
                duration_frames: state.frame_duration.max(1),
            });
        }

        (metasprites, frames)
    };

    bundle.metasprites.extend(new_metasprites);
    bundle.animations.push(AnimationResource {
        id: state.animation_id.clone(),
        frames: animation_frames,
    });

    Ok(format!(
        "Imported {} frame(s) into '{}' and created animation '{}'",
        state.frame_count, state.target_tileset_id, state.animation_id
    ))
}

fn extract_tile_from_sheet(
    image: &RgbaImage,
    palette: &mut PaletteResource,
    start_x: u32,
    start_y: u32,
) -> Result<Tile8> {
    let mut pixels = Vec::with_capacity(64);
    for y in 0..8 {
        for x in 0..8 {
            let rgba = image.get_pixel(start_x + x, start_y + y).0;
            pixels.push(palette_index_for_rgba(palette, rgba));
        }
    }
    Ok(Tile8 { pixels })
}

fn palette_index_for_rgba(palette: &mut PaletteResource, rgba: [u8; 4]) -> u8 {
    if rgba[3] < 16 {
        return 0;
    }

    let color = RgbaColor {
        r: rgba[0],
        g: rgba[1],
        b: rgba[2],
        a: rgba[3],
    };

    if let Some(index) = palette
        .colors
        .iter()
        .position(|existing| *existing == color)
    {
        return index as u8;
    }

    if palette.colors.len() < 16 {
        palette.colors.push(color);
        return (palette.colors.len() - 1) as u8;
    }

    palette
        .colors
        .iter()
        .enumerate()
        .skip(1)
        .min_by_key(|(_, existing)| color_distance_squared(existing, &color))
        .map(|(index, _)| index as u8)
        .unwrap_or(0)
}

fn color_distance_squared(a: &RgbaColor, b: &RgbaColor) -> u32 {
    let dr = a.r as i32 - b.r as i32;
    let dg = a.g as i32 - b.g as i32;
    let db = a.b as i32 - b.b as i32;
    (dr * dr + dg * dg + db * db) as u32
}

fn fit_size(size: Vec2, max: Vec2) -> Vec2 {
    let scale = (max.x / size.x).min(max.y / size.y).min(1.0);
    size * scale
}

fn filter_matches(filter: &str, candidate: &str) -> bool {
    filter.is_empty() || candidate.to_ascii_lowercase().contains(filter)
}

fn to_color32(color: &RgbaColor) -> Color32 {
    Color32::from_rgba_premultiplied(color.r, color.g, color.b, color.a)
}

#[cfg(test)]
mod tests {
    use super::UndoHistory;
    use snesmaker_project::demo_bundle;

    #[test]
    fn undo_history_round_trips_project_state() {
        let original = demo_bundle();
        let mut current = original.clone();
        let mut history = UndoHistory::default();

        history.capture(&current);
        current.manifest.meta.name = "Changed Name".to_string();

        assert!(history.undo(&mut current));
        assert_eq!(current.manifest.meta.name, original.manifest.meta.name);

        assert!(history.redo(&mut current));
        assert_eq!(current.manifest.meta.name, "Changed Name");
    }
}
