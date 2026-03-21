use std::collections::BTreeSet;
use std::fs;

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use ron::ser::PrettyConfig;
use serde::{Deserialize, Serialize};

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

    pub fn active_tab(&self, area: DockArea) -> Option<DockTab> {
        self.slot(area).active_tab()
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
        self.saved_layouts.retain(|layout| !layout.name.trim().is_empty());
        self.saved_layouts.sort_by(|left, right| left.name.cmp(&right.name));
        self.saved_layouts
            .dedup_by(|left, right| left.name.eq_ignore_ascii_case(&right.name));
        for layout in &mut self.saved_layouts {
            layout.layout.normalize();
        }
        if let Some(active) = &self.active_saved_layout {
            if self
                .saved_layouts
                .iter()
                .all(|layout| !layout.name.eq_ignore_ascii_case(active))
            {
                self.active_saved_layout = None;
            }
        }
    }
}

pub fn workspace_state_path(project_root: &Utf8Path) -> Utf8PathBuf {
    project_root.join(WORKSPACE_DIR_NAME).join(WORKSPACE_FILE_NAME)
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

pub fn copy_workspace_file(project_root: &Utf8Path, export_root: &Utf8Path) -> Result<()> {
    let source = workspace_state_path(project_root);
    if !source.exists() {
        return Ok(());
    }

    let destination = workspace_state_path(export_root);
    let parent = destination
        .parent()
        .context("workspace export path is missing a parent directory")?;
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

#[cfg(test)]
mod tests {
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
}
