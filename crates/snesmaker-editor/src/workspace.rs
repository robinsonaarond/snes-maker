use std::collections::BTreeSet;
use std::fs;

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use ron::ser::PrettyConfig;
use serde::{Deserialize, Serialize};
use snesmaker_project::{
    Checkpoint, CollisionLayer, EntityPlacement, GridSize, SceneKind, SpawnPoint, TileLayer,
    TriggerVolume,
};

const WORKSPACE_DIR_NAME: &str = ".snesmaker";
const WORKSPACE_FILE_NAME: &str = "editor-workspace.ron";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DockArea {
    Left,
    #[default]
    Center,
    Right,
    Bottom,
}

impl DockArea {
    pub const ALL: [Self; 4] = [Self::Left, Self::Center, Self::Right, Self::Bottom];

    pub fn label(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Center => "Center",
            Self::Right => "Right",
            Self::Bottom => "Bottom",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DockTab {
    Toolbox,
    Scene,
    Inspector,
    Outliner,
    Assets,
    Animation,
    Diagnostics,
    BuildReport,
    Playtest,
}

impl DockTab {
    pub const ALL: [Self; 9] = [
        Self::Toolbox,
        Self::Scene,
        Self::Inspector,
        Self::Outliner,
        Self::Assets,
        Self::Animation,
        Self::Diagnostics,
        Self::BuildReport,
        Self::Playtest,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Toolbox => "Toolbox",
            Self::Scene => "Scene",
            Self::Inspector => "Inspector",
            Self::Outliner => "Outliner",
            Self::Assets => "Assets",
            Self::Animation => "Animation",
            Self::Diagnostics => "Diagnostics",
            Self::BuildReport => "Build Report",
            Self::Playtest => "Playtest",
        }
    }

    pub fn default_area(self) -> DockArea {
        match self {
            Self::Toolbox | Self::Outliner => DockArea::Left,
            Self::Scene => DockArea::Center,
            Self::Inspector | Self::Animation => DockArea::Right,
            Self::Assets | Self::Diagnostics | Self::BuildReport | Self::Playtest => {
                DockArea::Bottom
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DockSlot {
    pub size: f32,
    #[serde(default)]
    pub tabs: Vec<DockTab>,
    #[serde(default)]
    pub active: usize,
}

impl DockSlot {
    pub fn new(size: f32, tabs: Vec<DockTab>, active: usize) -> Self {
        let mut slot = Self { size, tabs, active };
        slot.normalize();
        slot
    }

    pub fn normalize(&mut self) {
        self.size = self.size.max(180.0);
        if self.tabs.is_empty() {
            self.active = 0;
        } else {
            self.active = self.active.min(self.tabs.len().saturating_sub(1));
        }
    }

    pub fn active_tab(&self) -> Option<DockTab> {
        self.tabs.get(self.active).copied()
    }
}

impl Default for DockSlot {
    fn default() -> Self {
        Self::new(280.0, Vec::new(), 0)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DockLayout {
    #[serde(default = "default_show_status_bar")]
    pub show_status_bar: bool,
    #[serde(default)]
    pub left: DockSlot,
    #[serde(default = "default_center_slot")]
    pub center: DockSlot,
    #[serde(default)]
    pub right: DockSlot,
    #[serde(default)]
    pub bottom: DockSlot,
}

impl Default for DockLayout {
    fn default() -> Self {
        Self {
            show_status_bar: true,
            left: DockSlot::new(320.0, Vec::new(), 0),
            center: DockSlot::new(0.0, vec![DockTab::Scene], 0),
            right: DockSlot::new(360.0, Vec::new(), 0),
            bottom: DockSlot::new(280.0, Vec::new(), 0),
        }
    }
}

impl DockLayout {
    pub fn slot(&self, area: DockArea) -> &DockSlot {
        match area {
            DockArea::Left => &self.left,
            DockArea::Center => &self.center,
            DockArea::Right => &self.right,
            DockArea::Bottom => &self.bottom,
        }
    }

    pub fn slot_mut(&mut self, area: DockArea) -> &mut DockSlot {
        match area {
            DockArea::Left => &mut self.left,
            DockArea::Center => &mut self.center,
            DockArea::Right => &mut self.right,
            DockArea::Bottom => &mut self.bottom,
        }
    }

    pub fn contains(&self, tab: DockTab) -> bool {
        DockArea::ALL
            .iter()
            .any(|area| self.slot(*area).tabs.contains(&tab))
    }

    pub fn area_for(&self, tab: DockTab) -> Option<DockArea> {
        DockArea::ALL
            .iter()
            .copied()
            .find(|area| self.slot(*area).tabs.contains(&tab))
    }

    pub fn set_active_tab(&mut self, area: DockArea, index: usize) {
        let slot = self.slot_mut(area);
        if !slot.tabs.is_empty() {
            slot.active = index.min(slot.tabs.len().saturating_sub(1));
        }
    }

    pub fn set_slot_size(&mut self, area: DockArea, size: f32) {
        if area != DockArea::Center {
            self.slot_mut(area).size = size.max(180.0);
        }
    }

    pub fn show_tab(&mut self, tab: DockTab) {
        if let Some(area) = self.area_for(tab) {
            if let Some(index) = self.slot(area).tabs.iter().position(|entry| *entry == tab) {
                self.set_active_tab(area, index);
            }
            return;
        }

        let area = tab.default_area();
        let slot = self.slot_mut(area);
        slot.tabs.push(tab);
        slot.active = slot.tabs.len().saturating_sub(1);
        slot.normalize();
    }

    pub fn hide_tab(&mut self, tab: DockTab) {
        for area in DockArea::ALL {
            let slot = self.slot_mut(area);
            if let Some(index) = slot.tabs.iter().position(|entry| *entry == tab) {
                slot.tabs.remove(index);
                if slot.active > index {
                    slot.active = slot.active.saturating_sub(1);
                }
                slot.normalize();
                break;
            }
        }
    }

    pub fn move_tab(&mut self, tab: DockTab, target: DockArea) {
        self.hide_tab(tab);
        let slot = self.slot_mut(target);
        slot.tabs.push(tab);
        slot.active = slot.tabs.len().saturating_sub(1);
        slot.normalize();
    }

    pub fn move_active_within_slot(&mut self, area: DockArea, direction: i32) {
        let slot = self.slot_mut(area);
        if slot.tabs.is_empty() {
            return;
        }

        let active = slot.active.min(slot.tabs.len().saturating_sub(1));
        let target = match direction {
            -1 if active > 0 => active - 1,
            1 if active + 1 < slot.tabs.len() => active + 1,
            _ => return,
        };
        slot.tabs.swap(active, target);
        slot.active = target;
    }

    pub fn normalize(&mut self) {
        self.left.normalize();
        self.center.normalize();
        self.right.normalize();
        self.bottom.normalize();

        let mut seen = BTreeSet::new();
        for area in DockArea::ALL {
            let slot = self.slot_mut(area);
            slot.tabs.retain(|tab| seen.insert(*tab));
            slot.normalize();
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedDockLayout {
    pub name: String,
    #[serde(default)]
    pub layout: DockLayout,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct EditorFavorites {
    #[serde(default)]
    pub scenes: Vec<String>,
    #[serde(default)]
    pub palettes: Vec<String>,
    #[serde(default)]
    pub tilesets: Vec<String>,
    #[serde(default)]
    pub metasprites: Vec<String>,
    #[serde(default)]
    pub animations: Vec<String>,
    #[serde(default)]
    pub dialogues: Vec<String>,
    #[serde(default)]
    pub sprite_sources: Vec<String>,
}

impl EditorFavorites {
    pub fn normalize(&mut self) {
        normalize_name_list(&mut self.scenes);
        normalize_name_list(&mut self.palettes);
        normalize_name_list(&mut self.tilesets);
        normalize_name_list(&mut self.metasprites);
        normalize_name_list(&mut self.animations);
        normalize_name_list(&mut self.dialogues);
        normalize_name_list(&mut self.sprite_sources);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedSceneSnippet {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub source_scene_id: Option<String>,
    #[serde(default)]
    pub scene_kind: SceneKind,
    #[serde(default)]
    pub size_tiles: GridSize,
    #[serde(default)]
    pub layers: Vec<TileLayer>,
    #[serde(default = "default_collision_layer")]
    pub collision: CollisionLayer,
    #[serde(default)]
    pub spawns: Vec<SpawnPoint>,
    #[serde(default)]
    pub checkpoints: Vec<Checkpoint>,
    #[serde(default)]
    pub entities: Vec<EntityPlacement>,
    #[serde(default)]
    pub triggers: Vec<TriggerVolume>,
}

impl SavedSceneSnippet {
    pub fn normalize(&mut self) {
        self.name = self.name.trim().to_string();
        self.source_scene_id = self
            .source_scene_id
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string());

        for layer in &mut self.layers {
            layer.id = layer.id.trim().to_string();
            layer.tileset_id = layer.tileset_id.trim().to_string();
        }

        for spawn in &mut self.spawns {
            spawn.id = spawn.id.trim().to_string();
        }
        for checkpoint in &mut self.checkpoints {
            checkpoint.id = checkpoint.id.trim().to_string();
        }
        for entity in &mut self.entities {
            entity.id = entity.id.trim().to_string();
            entity.archetype = entity.archetype.trim().to_string();
        }
        for trigger in &mut self.triggers {
            trigger.id = trigger.id.trim().to_string();
            trigger.script_id = trigger.script_id.trim().to_string();
        }

        let tile_count = self.size_tiles.tile_count();
        if tile_count > 0 {
            for layer in &mut self.layers {
                resize_or_pad(&mut layer.tiles, tile_count, 0);
            }
            resize_or_pad(&mut self.collision.solids, tile_count, false);
            resize_or_pad(&mut self.collision.ladders, tile_count, false);
            resize_or_pad(&mut self.collision.hazards, tile_count, false);
        }
    }
}

impl Default for SavedSceneSnippet {
    fn default() -> Self {
        Self {
            name: String::new(),
            source_scene_id: None,
            scene_kind: SceneKind::default(),
            size_tiles: GridSize::default(),
            layers: Vec::new(),
            collision: default_collision_layer(),
            spawns: Vec::new(),
            checkpoints: Vec::new(),
            entities: Vec::new(),
            triggers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SavedTileBrush {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub size_tiles: GridSize,
    #[serde(default)]
    pub tiles: Vec<u16>,
    #[serde(default)]
    pub solids: Vec<bool>,
    #[serde(default)]
    pub ladders: Vec<bool>,
    #[serde(default)]
    pub hazards: Vec<bool>,
}

impl SavedTileBrush {
    pub fn normalize(&mut self) {
        self.name = self.name.trim().to_string();
        let tile_count = self.size_tiles.tile_count();
        if tile_count > 0 {
            resize_or_pad(&mut self.tiles, tile_count, 0);
            resize_or_pad(&mut self.solids, tile_count, false);
            resize_or_pad(&mut self.ladders, tile_count, false);
            resize_or_pad(&mut self.hazards, tile_count, false);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SceneSnippetLibrary {
    #[serde(default)]
    pub snippets: Vec<SavedSceneSnippet>,
    #[serde(default)]
    pub brushes: Vec<SavedTileBrush>,
}

impl SceneSnippetLibrary {
    pub fn normalize(&mut self) {
        self.snippets
            .retain(|snippet| !snippet.name.trim().is_empty());
        for snippet in &mut self.snippets {
            snippet.name = snippet.name.trim().to_string();
        }
        self.snippets
            .sort_by(|left, right| left.name.cmp(&right.name));
        self.snippets
            .dedup_by(|left, right| left.name.eq_ignore_ascii_case(&right.name));
        for snippet in &mut self.snippets {
            snippet.normalize();
        }

        self.brushes.retain(|brush| !brush.name.trim().is_empty());
        for brush in &mut self.brushes {
            brush.name = brush.name.trim().to_string();
        }
        self.brushes
            .sort_by(|left, right| left.name.cmp(&right.name));
        self.brushes
            .dedup_by(|left, right| left.name.eq_ignore_ascii_case(&right.name));
        for brush in &mut self.brushes {
            brush.normalize();
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct WorkspaceFile {
    #[serde(default)]
    pub current_layout: DockLayout,
    #[serde(default)]
    pub saved_layouts: Vec<SavedDockLayout>,
    #[serde(default)]
    pub active_saved_layout: Option<String>,
}

impl WorkspaceFile {
    pub fn normalize(&mut self) {
        self.current_layout.normalize();
        self.saved_layouts
            .retain(|layout| !layout.name.trim().is_empty());
        for layout in &mut self.saved_layouts {
            layout.name = layout.name.trim().to_string();
        }
        self.saved_layouts
            .sort_by(|left, right| left.name.cmp(&right.name));
        self.saved_layouts
            .dedup_by(|left, right| left.name.eq_ignore_ascii_case(&right.name));
        for layout in &mut self.saved_layouts {
            layout.layout.normalize();
        }
        if let Some(active) = self.active_saved_layout.take() {
            let active = active.trim().to_string();
            if !active.is_empty()
                && self
                    .saved_layouts
                    .iter()
                    .any(|layout| layout.name.eq_ignore_ascii_case(&active))
            {
                self.active_saved_layout = Some(active);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct WorkspaceAddons {
    #[serde(default)]
    pub editor_favorites: EditorFavorites,
    #[serde(default)]
    pub scene_library: SceneSnippetLibrary,
}

impl WorkspaceAddons {
    pub fn normalize(&mut self) {
        self.editor_favorites.normalize();
        self.scene_library.normalize();
    }
}

pub fn workspace_state_path(project_root: &Utf8Path) -> Utf8PathBuf {
    project_root
        .join(WORKSPACE_DIR_NAME)
        .join(WORKSPACE_FILE_NAME)
}

pub fn workspace_addons_path(project_root: &Utf8Path) -> Utf8PathBuf {
    project_root
        .join(WORKSPACE_DIR_NAME)
        .join("editor-addons.ron")
}

pub fn load_workspace_file(project_root: &Utf8Path) -> Result<Option<WorkspaceFile>> {
    let path = workspace_state_path(project_root);
    if !path.exists() {
        return Ok(None);
    }

    let text = fs::read_to_string(&path).with_context(|| format!("failed to read {}", path))?;
    let mut file: WorkspaceFile =
        ron::from_str(&text).with_context(|| format!("failed to parse {}", path))?;
    file.normalize();
    Ok(Some(file))
}

pub fn load_workspace_addons(project_root: &Utf8Path) -> Result<Option<WorkspaceAddons>> {
    let path = workspace_addons_path(project_root);
    if !path.exists() {
        return Ok(None);
    }

    let text = fs::read_to_string(&path).with_context(|| format!("failed to read {}", path))?;
    let mut file: WorkspaceAddons =
        ron::from_str(&text).with_context(|| format!("failed to parse {}", path))?;
    file.normalize();
    Ok(Some(file))
}

pub fn save_workspace_file(project_root: &Utf8Path, workspace: &WorkspaceFile) -> Result<()> {
    let mut workspace = workspace.clone();
    workspace.normalize();

    let path = workspace_state_path(project_root);
    let parent = path
        .parent()
        .context("workspace file is missing a parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent))?;

    let text = ron::ser::to_string_pretty(&workspace, PrettyConfig::new())?;
    fs::write(&path, text).with_context(|| format!("failed to write {}", path))?;
    Ok(())
}

pub fn save_workspace_addons(project_root: &Utf8Path, addons: &WorkspaceAddons) -> Result<()> {
    let mut addons = addons.clone();
    addons.normalize();

    let path = workspace_addons_path(project_root);
    let parent = path
        .parent()
        .context("workspace addon path is missing a parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent))?;

    let text = ron::ser::to_string_pretty(&addons, PrettyConfig::new())?;
    fs::write(&path, text).with_context(|| format!("failed to write {}", path))?;
    Ok(())
}

pub fn copy_workspace_file(project_root: &Utf8Path, export_root: &Utf8Path) -> Result<()> {
    let source = workspace_state_path(project_root);
    if !source.exists() {
        return copy_workspace_addons(project_root, export_root);
    }

    let destination = workspace_state_path(export_root);
    let parent = destination
        .parent()
        .context("workspace export path is missing a parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent))?;
    fs::copy(&source, &destination)
        .with_context(|| format!("failed to copy {} to {}", source, destination))?;
    copy_workspace_addons(project_root, export_root)
}

pub fn copy_workspace_addons(project_root: &Utf8Path, export_root: &Utf8Path) -> Result<()> {
    let source = workspace_addons_path(project_root);
    if !source.exists() {
        return Ok(());
    }

    let destination = workspace_addons_path(export_root);
    let parent = destination
        .parent()
        .context("workspace export addon path is missing a parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent))?;
    fs::copy(&source, &destination)
        .with_context(|| format!("failed to copy {} to {}", source, destination))?;
    Ok(())
}

fn default_show_status_bar() -> bool {
    true
}

fn default_center_slot() -> DockSlot {
    DockSlot::new(0.0, vec![DockTab::Scene], 0)
}

fn default_collision_layer() -> CollisionLayer {
    CollisionLayer {
        solids: Vec::new(),
        ladders: Vec::new(),
        hazards: Vec::new(),
    }
}

fn normalize_name_list(values: &mut Vec<String>) {
    let mut normalized = values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    normalized.sort_by(|left, right| left.to_ascii_lowercase().cmp(&right.to_ascii_lowercase()));
    normalized.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    *values = normalized;
}

fn resize_or_pad<T: Clone>(values: &mut Vec<T>, len: usize, fill: T) {
    values.resize(len, fill);
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{DockArea, DockLayout, DockTab, WorkspaceFile};

    #[test]
    fn moving_tab_rehomes_it_once() {
        let mut layout = DockLayout::default();
        layout.show_tab(DockTab::Inspector);
        layout.move_tab(DockTab::Inspector, DockArea::Left);

        assert_eq!(layout.area_for(DockTab::Inspector), Some(DockArea::Left));
        assert_eq!(
            layout
                .left
                .tabs
                .iter()
                .filter(|tab| **tab == DockTab::Inspector)
                .count(),
            1
        );
    }

    #[test]
    fn workspace_file_normalizes_saved_layout_names() {
        let mut workspace = WorkspaceFile {
            saved_layouts: vec![
                super::SavedDockLayout {
                    name: "Animation".to_string(),
                    layout: DockLayout::default(),
                },
                super::SavedDockLayout {
                    name: "animation".to_string(),
                    layout: DockLayout::default(),
                },
            ],
            active_saved_layout: Some("animation".to_string()),
            ..WorkspaceFile::default()
        };

        workspace.normalize();

        assert_eq!(workspace.saved_layouts.len(), 1);
        assert_eq!(workspace.active_saved_layout.as_deref(), Some("animation"));
    }

    #[test]
    fn workspace_file_loads_older_shapes_with_default_persistence_fields() {
        let workspace: WorkspaceFile = ron::from_str(
            r#"
            (
                saved_layouts: [],
                active_saved_layout: None,
            )
            "#,
        )
        .expect("parse legacy workspace");

        assert!(workspace.saved_layouts.is_empty());
        assert!(workspace.active_saved_layout.is_none());
        assert!(
            super::load_workspace_addons(&camino::Utf8PathBuf::from("/tmp/does-not-exist"))
                .expect("load missing addons")
                .is_none()
        );
    }

    #[test]
    fn workspace_addons_normalize_favorites_and_libraries() {
        let mut addons = super::WorkspaceAddons {
            editor_favorites: super::EditorFavorites {
                scenes: vec![
                    " intro_stage ".to_string(),
                    "INTRO_STAGE".to_string(),
                    "boss_room".to_string(),
                ],
                tilesets: vec![" default_tiles ".to_string(), "DEFAULT_TILES".to_string()],
                ..Default::default()
            },
            scene_library: super::SceneSnippetLibrary {
                snippets: vec![
                    super::SavedSceneSnippet {
                        name: "  Shared Room  ".to_string(),
                        source_scene_id: Some(" intro_stage ".to_string()),
                        size_tiles: super::GridSize {
                            width: 2,
                            height: 1,
                        },
                        layers: vec![super::TileLayer {
                            id: " layer_a ".to_string(),
                            tileset_id: " default_tiles ".to_string(),
                            visible: true,
                            parallax_x: 1,
                            parallax_y: 1,
                            tiles: vec![7],
                        }],
                        collision: super::CollisionLayer {
                            solids: vec![true],
                            ladders: vec![],
                            hazards: vec![],
                        },
                        spawns: vec![],
                        checkpoints: vec![],
                        entities: vec![],
                        triggers: vec![],
                        ..Default::default()
                    },
                    super::SavedSceneSnippet {
                        name: "shared room".to_string(),
                        ..Default::default()
                    },
                ],
                brushes: vec![
                    super::SavedTileBrush {
                        name: "  Accent Brush ".to_string(),
                        size_tiles: super::GridSize {
                            width: 1,
                            height: 2,
                        },
                        tiles: vec![1],
                        solids: vec![true, false, true],
                        ladders: vec![false],
                        hazards: vec![],
                    },
                    super::SavedTileBrush {
                        name: "accent brush".to_string(),
                        ..Default::default()
                    },
                ],
            },
        };

        addons.normalize();

        assert_eq!(
            addons.editor_favorites.scenes,
            vec!["boss_room", "intro_stage"]
        );
        assert_eq!(addons.editor_favorites.tilesets, vec!["default_tiles"]);
        assert_eq!(addons.scene_library.snippets.len(), 1);
        assert_eq!(addons.scene_library.snippets[0].name, "Shared Room");
        assert_eq!(
            addons.scene_library.snippets[0].source_scene_id.as_deref(),
            Some("intro_stage")
        );
        assert_eq!(addons.scene_library.snippets[0].layers[0].id, "layer_a");
        assert_eq!(
            addons.scene_library.snippets[0].layers[0].tileset_id,
            "default_tiles"
        );
        assert_eq!(addons.scene_library.snippets[0].layers[0].tiles, vec![7, 0]);
        assert_eq!(
            addons.scene_library.snippets[0].collision.solids,
            vec![true, false]
        );
        assert_eq!(addons.scene_library.brushes.len(), 1);
        assert_eq!(addons.scene_library.brushes[0].name, "Accent Brush");
        assert_eq!(addons.scene_library.brushes[0].tiles, vec![1, 0]);
        assert_eq!(addons.scene_library.brushes[0].solids, vec![true, false]);
    }

    #[test]
    fn workspace_addons_round_trips_new_persistence_fields() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("monotonic clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("snes-maker-workspace-test-{}", unique));
        fs::create_dir_all(&root).expect("create temp root");
        let project_root = camino::Utf8PathBuf::from_path_buf(root.clone()).expect("utf8 root");

        let addons = super::WorkspaceAddons {
            editor_favorites: super::EditorFavorites {
                scenes: vec!["intro_stage".to_string()],
                palettes: vec!["default_palette".to_string()],
                ..Default::default()
            },
            scene_library: super::SceneSnippetLibrary {
                brushes: vec![super::SavedTileBrush {
                    name: "platform".to_string(),
                    size_tiles: super::GridSize {
                        width: 1,
                        height: 1,
                    },
                    tiles: vec![2],
                    solids: vec![true],
                    ladders: vec![false],
                    hazards: vec![false],
                }],
                ..Default::default()
            },
        };

        super::save_workspace_addons(&project_root, &addons).expect("save addons");
        let loaded = super::load_workspace_addons(&project_root)
            .expect("load workspace")
            .expect("workspace file present");

        assert_eq!(loaded.editor_favorites.scenes, vec!["intro_stage"]);
        assert_eq!(loaded.editor_favorites.palettes, vec!["default_palette"]);
        assert_eq!(loaded.scene_library.brushes.len(), 1);
        assert_eq!(loaded.scene_library.brushes[0].name, "platform");
    }
}
