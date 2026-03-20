use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use camino::{Utf8Path, Utf8PathBuf};
use eframe::egui::{
    self, Align2, Color32, ColorImage, FontId, Key, KeyboardShortcut, Modifiers, Pos2, Rect, Sense,
    StrokeKind, TextureHandle, TextureOptions, Vec2, ViewportCommand,
};
use image::RgbaImage;
use rfd::FileDialog;
use snesmaker_events::TriggerKind;
use snesmaker_export::build_rom;
use snesmaker_project::{
    AnimationFrame, AnimationResource, Checkpoint, CombatProfile, EntityAction, EntityKind,
    EntityPlacement, Facing, HealthHudStyle, MetaspriteResource, MovementPattern,
    PROJECT_SPRITE_SOURCE_DIR, PaletteResource, PointI16, ProjectBundle, RectI16, RgbaColor,
    SceneResource, SpawnPoint, SpriteTileRef, Tile8, TilesetResource, TriggerVolume,
    default_entity_hitbox, slugify,
};
use snesmaker_validator::{Severity, ValidationReport, validate_project};

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
        };
        app.reload();
        app
    }

    fn reload(&mut self) {
        match ProjectBundle::load(&self.project_root) {
            Ok(bundle) => {
                self.bundle = Some(bundle);
                self.report = validate_project(self.bundle.as_ref().expect("bundle"));
                self.status = format!("Loaded {}", self.project_root);
                self.dirty = false;
                self.history.clear();
                self.active_canvas_cell = None;
                self.selection = None;
                self.selection_drag_anchor = None;
                self.last_canvas_tile = None;
                self.preview_focus = PreviewFocus::None;
                self.scene_scroll_offset = Vec2::ZERO;
                self.sync_selection();
            }
            Err(error) => {
                self.bundle = None;
                self.report = ValidationReport::default();
                self.status = error.to_string();
                self.dirty = false;
                self.history.clear();
                self.active_canvas_cell = None;
                self.selection = None;
                self.selection_drag_anchor = None;
                self.last_canvas_tile = None;
                self.preview_focus = PreviewFocus::None;
                self.scene_scroll_offset = Vec2::ZERO;
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
        self.selected_tile = self.selected_tile.min(
            bundle
                .tilesets
                .first()
                .map(|tileset| tileset.tiles.len())
                .unwrap_or(1)
                .saturating_sub(1),
        );
        self.selected_animation = self
            .selected_animation
            .min(bundle.animations.len().saturating_sub(1));
        self.import_state.sync_to_bundle(bundle);

        if let Some(scene) = bundle.scenes.get(self.selected_scene) {
            self.selected_spawn = sanitize_optional_index(self.selected_spawn, scene.spawns.len());
            self.selected_checkpoint =
                sanitize_optional_index(self.selected_checkpoint, scene.checkpoints.len());
            self.selected_entity =
                sanitize_optional_index(self.selected_entity, scene.entities.len());
            self.selected_trigger =
                sanitize_optional_index(self.selected_trigger, scene.triggers.len());
        } else {
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
        let Some(layer) = scene.layers.first() else {
            self.status = "Current scene has no tile layer.".to_string();
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
        self.status = format!("Copied {}x{} selection", width_tiles, height_tiles);
    }

    fn paste_clipboard(&mut self) {
        let Some(clipboard) = self.clipboard.clone() else {
            self.status = "Clipboard is empty.".to_string();
            return;
        };
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

        if let Some(scene) = self.current_scene_mut() {
            let scene_width = scene.size_tiles.width as usize;
            let scene_height = scene.size_tiles.height as usize;
            if let Some(layer) = scene.layers.first_mut() {
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
        self.mark_edited(format!("Pasted selection at {}, {}", anchor.0, anchor.1));
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
        {
            Ok(()) => self.status = format!("Exported project to {}", export_root),
            Err(error) => self.status = error.to_string(),
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
                });

                ui.menu_button("View", |ui| {
                    ui.checkbox(&mut self.show_grid, "Show Grid");
                    ui.checkbox(&mut self.show_collision, "Show Collision Overlay");
                    ui.add(
                        egui::Slider::new(&mut self.scene_zoom, SCENE_MIN_ZOOM..=SCENE_MAX_ZOOM)
                            .text("Scene Zoom"),
                    );
                });

                ui.menu_button("Help", |ui| {
                    if ui.button("Controls & Workflow").clicked() {
                        self.show_help = true;
                        ui.close();
                    }
                });

                ui.separator();
                ui.strong("SNES Maker");
                ui.label(self.project_root.as_str());
                if self.dirty {
                    ui.colored_label(Color32::from_rgb(222, 168, 32), "Unsaved changes");
                }
            });
        });

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

    fn draw_left_panel(&mut self, ctx: &egui::Context) {
        let mut pending_scene_selection = None;
        egui::SidePanel::left("left_panel")
            .min_width(280.0)
            .resizable(true)
            .show(ctx, |ui| {
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
                if let (Some(tileset), Some(palette)) =
                    (bundle.tilesets.first(), bundle.palettes.first())
                {
                    ui.label(format!("Tileset: {}", tileset.name));
                    egui::ScrollArea::vertical()
                        .max_height(260.0)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                for (index, tile) in tileset.tiles.iter().enumerate() {
                                    let response = draw_tile_button(
                                        ui,
                                        tile,
                                        palette,
                                        2.0,
                                        self.selected_tile == index,
                                    );
                                    if response.clicked() {
                                        self.selected_tile = index;
                                        self.tool = EditorTool::Paint;
                                        self.preview_focus = PreviewFocus::None;
                                    }
                                }
                            });
                        });
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
            });
        if let Some(index) = pending_scene_selection {
            self.selected_scene = index;
            self.clear_selection();
            self.preview_focus = PreviewFocus::None;
            self.scene_scroll_offset = Vec2::ZERO;
            self.sync_selection();
        }
    }

    fn draw_right_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("right_panel")
            .min_width(340.0)
            .resizable(true)
            .show(ctx, |ui| {
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

                let Some(scene_snapshot) = scene_snapshot else {
                    ui.heading("Inspector");
                    ui.label("Load a project to inspect it.");
                    return;
                };

                ui.heading("Inspector");
                ui.label(format!(
                    "Scene: {} ({}x{} tiles)",
                    scene_snapshot.id,
                    scene_snapshot.size_tiles.width,
                    scene_snapshot.size_tiles.height
                ));
                ui.label(format!(
                    "Chunk: {}x{}  |  Scripts: {}",
                    scene_snapshot.chunk_size_tiles.width,
                    scene_snapshot.chunk_size_tiles.height,
                    scene_snapshot.scripts.len()
                ));

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
                ui.separator();
                self.draw_diagnostics(ui);
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
                .is_some_and(|entity| metasprite_for_entity(bundle, entity, 0.0).is_some()),
            PreviewFocus::None => false,
        }
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
            let Some(bundle) = &self.bundle else {
                return;
            };
            let Some(tileset) = bundle.tilesets.first() else {
                return;
            };
            let Some(palette) = bundle.palettes.first() else {
                return;
            };
            let Some(tile) = tileset.tiles.get(self.selected_tile) else {
                return;
            };

            ui.label(format!("Tile {}", self.selected_tile));
            let mut edited = tile.clone();
            let changed = draw_tile_editor_grid(ui, &mut edited, palette);
            if changed {
                self.capture_history();
                if let Some(bundle) = &mut self.bundle {
                    if let Some(tileset) = bundle.tilesets.first_mut() {
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
        ui.collapsing("Diagnostics", |ui| {
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
    }

    fn draw_central_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.bundle.is_none() {
                ui.vertical_centered(|ui| {
                    ui.add_space(120.0);
                    ui.heading("SNES Maker");
                    ui.label(
                        "Open a project or create a new template to start building a SNES demo.",
                    );
                    if ui.button("Open Project").clicked() {
                        self.open_project_dialog();
                    }
                    if ui.button("Create Template Project").clicked() {
                        self.new_project_state.open = true;
                    }
                });
                return;
            }

            let (scene_label, entry_scene) = {
                let bundle = self.bundle.as_ref().expect("bundle");
                (
                    bundle
                        .scenes
                        .get(self.selected_scene)
                        .map(|scene| scene.id.clone())
                        .unwrap_or_else(|| "no_scene".to_string()),
                    bundle.manifest.gameplay.entry_scene.clone(),
                )
            };

            ui.horizontal(|ui| {
                ui.heading("Scene Preview");
                ui.label(format!("{}  |  entry scene: {}", scene_label, entry_scene));
                ui.separator();
                ui.label(format!("Tool: {}", self.tool.label()));
                ui.separator();
                ui.label("Select drags a box. Two-finger scroll pans. Pinch zooms. Cmd+C / Cmd+V copies and pastes selections.");
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
            self.scene_scroll_offset = clamp_scene_scroll_offset(
                self.scene_scroll_offset,
                content_size,
                viewport_size,
            );
            let outcome = draw_scene_canvas(
                ui,
                bundle,
                self.selected_scene,
                self.scene_zoom,
                self.scene_scroll_offset,
                viewport_size,
                self.show_grid,
                self.show_collision,
                self.selected_tile,
                self.selected_spawn,
                self.selected_checkpoint,
                self.selected_entity,
                self.selected_trigger,
                self.selection.as_ref(),
                self.selection_drag_anchor,
                self.tool == EditorTool::Select,
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

            ui.add_space(12.0);
            ui.collapsing("Workflow", |ui| {
                ui.label("1. Use Select to drag a region and copy or paste tiles and objects.");
                ui.label("2. Paint the stage with tiles and mark solids, ladders, and hazards.");
                ui.label("3. Add spawns, checkpoints, entities, and triggers from the inspector.");
                ui.label("4. Import a sprite sheet to create new metasprites and animations.");
                ui.label("5. Save, then build the ROM.");
            });
            self.apply_canvas_outcome(outcome);
        });
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

        let mut edited = false;
        let mut status = None;
        if let Some(pending) = pending {
            self.capture_history();
            if let Some(scene) = self.current_scene_mut() {
                match pending {
                    PendingSceneEdit::PaintTile(tile_index) => {
                        if let Some(layer) = scene.layers.first_mut() {
                            if cell_index < layer.tiles.len() {
                                layer.tiles[cell_index] = tile_index;
                                edited = true;
                                status = Some(if tile_index == 0 {
                                    "Erased tile".to_string()
                                } else {
                                    format!("Painted tile {}", tile_index)
                                });
                            }
                        }
                    }
                    PendingSceneEdit::SetSolid(value) => {
                        if cell_index < scene.collision.solids.len() {
                            scene.collision.solids[cell_index] = value;
                            edited = true;
                            status = Some("Updated solid collision".to_string());
                        }
                    }
                    PendingSceneEdit::SetLadder(value) => {
                        if cell_index < scene.collision.ladders.len() {
                            scene.collision.ladders[cell_index] = value;
                            edited = true;
                            status = Some("Updated ladder collision".to_string());
                        }
                    }
                    PendingSceneEdit::SetHazard(value) => {
                        if cell_index < scene.collision.hazards.len() {
                            scene.collision.hazards[cell_index] = value;
                            edited = true;
                            status = Some("Updated hazard collision".to_string());
                        }
                    }
                    PendingSceneEdit::MoveSpawn(index, position) => {
                        if let Some(spawn) = scene.spawns.get_mut(index) {
                            spawn.position = position;
                            edited = true;
                            status = Some(format!("Moved spawn '{}'", spawn.id));
                        }
                    }
                    PendingSceneEdit::MoveCheckpoint(index, position) => {
                        if let Some(checkpoint) = scene.checkpoints.get_mut(index) {
                            checkpoint.position = position;
                            edited = true;
                            status = Some(format!("Moved checkpoint '{}'", checkpoint.id));
                        }
                    }
                    PendingSceneEdit::MoveEntity(index, position) => {
                        if let Some(entity) = scene.entities.get_mut(index) {
                            entity.position = position;
                            edited = true;
                            status = Some(format!("Moved entity '{}'", entity.id));
                        }
                    }
                    PendingSceneEdit::MoveTrigger(index, position) => {
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
                    ui.label("Cmd+I: import sprite sheet");
                    ui.label("Cmd+B: build ROM");
                    ui.label("Cmd+R: reload from disk");
                    ui.label("Cmd+Z / Cmd+Shift+Z: undo / redo");
                    ui.separator();
                    ui.heading("Scene Editing");
                    ui.label("Use Select to drag a rubber-band region around tiles and objects.");
                    ui.label("Use two-finger horizontal or vertical scrolling to pan around larger scenes, and pinch to zoom in or out.");
                    ui.label("Use the Paint tool with the tile browser to build the stage.");
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
        self.draw_left_panel(ctx);
        self.draw_right_panel(ctx);
        self.draw_central_panel(ctx);
        self.draw_windows(ctx);
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

struct SceneCanvasOutcome {
    viewport_rect: Rect,
    hovered_tile: Option<(usize, usize)>,
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
    selected_tile: usize,
    selected_spawn: Option<usize>,
    selected_checkpoint: Option<usize>,
    selected_entity: Option<usize>,
    selected_trigger: Option<usize>,
    current_selection: Option<&SceneSelection>,
    selection_drag_anchor: Option<(usize, usize)>,
    selection_mode: bool,
    time_seconds: f32,
) -> SceneCanvasOutcome {
    let Some(scene) = bundle.scenes.get(scene_index) else {
        ui.label("No scene selected.");
        return SceneCanvasOutcome::default();
    };
    let Some(layer) = scene.layers.first() else {
        ui.label("Scene has no tile layer.");
        return SceneCanvasOutcome::default();
    };
    let Some(tileset) = bundle.tileset(&layer.tileset_id) else {
        ui.label("Layer references a missing tileset.");
        return SceneCanvasOutcome::default();
    };
    let Some(palette) = bundle.palette(&tileset.palette_id) else {
        ui.label("Tileset references a missing palette.");
        return SceneCanvasOutcome::default();
    };

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

            let tile_index = layer.tiles.get(cell_index).copied().unwrap_or_default() as usize;
            if let Some(tile) = tileset.tiles.get(tile_index) {
                draw_tile_pixels(&painter, cell_rect, tile, palette);
            } else {
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

    draw_spawns(
        &painter,
        rect,
        zoom,
        &scene.spawns,
        selected_spawn,
        Color32::from_rgb(64, 212, 255),
    );
    draw_checkpoints(
        &painter,
        rect,
        zoom,
        &scene.checkpoints,
        selected_checkpoint,
        Color32::from_rgb(255, 220, 72),
    );
    draw_triggers(&painter, rect, zoom, &scene.triggers, selected_trigger);
    draw_entities(
        &painter,
        rect,
        zoom,
        bundle,
        &scene.entities,
        selected_entity,
        time_seconds,
    );

    if let Some(selection) = current_selection {
        draw_scene_selection_overlay(&painter, rect, zoom, scene, selection);
    }

    let selected_rect = selected_tile_preview_rect(rect, scene, selected_tile, zoom);
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

    let primary_cell = if !selection_mode && ui.input(|input| input.pointer.primary_down()) {
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
    selected_tile: usize,
    zoom: f32,
) -> Option<Rect> {
    let layer = scene.layers.first()?;
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
