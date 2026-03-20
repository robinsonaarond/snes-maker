use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use snesmaker_events::{EventCommand, reserved_unimplemented_commands};
use snesmaker_project::{EntityAction, EntityKind, GenreKind, ProjectBundle, SceneResource};

pub const MAX_COLORS_PER_PALETTE: usize = 16;
pub const MAX_PALETTES: usize = 8;
pub const MAX_TILESET_TILES: usize = 1024;
pub const MAX_TILE_PIXELS: usize = 64;
pub const MAX_METASPRITE_TILES_HARD: usize = 32;
pub const MAX_METASPRITE_TILES_WARN: usize = 24;
pub const MAX_SCENE_WIDTH_TILES: u16 = 256;
pub const MAX_SCENE_HEIGHT_TILES: u16 = 128;
pub const MAX_CHUNK_WIDTH_TILES: u16 = 32;
pub const MAX_CHUNK_HEIGHT_TILES: u16 = 32;
pub const ROM_BANK_SIZE: usize = 32 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: String,
    pub message: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct BuildBudgets {
    pub unique_tiles: usize,
    pub palette_colors: usize,
    pub scene_count: usize,
    pub metasprite_piece_peak: usize,
    pub estimated_rom_bytes: usize,
    pub estimated_rom_banks: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ValidationReport {
    pub errors: Vec<Diagnostic>,
    pub warnings: Vec<Diagnostic>,
    pub budgets: BuildBudgets,
}

impl ValidationReport {
    pub fn push(
        &mut self,
        severity: Severity,
        code: impl Into<String>,
        message: impl Into<String>,
        path: impl Into<Option<String>>,
    ) {
        let diagnostic = Diagnostic {
            severity,
            code: code.into(),
            message: message.into(),
            path: path.into(),
        };

        match severity {
            Severity::Error => self.errors.push(diagnostic),
            Severity::Warning => self.warnings.push(diagnostic),
        }
    }

    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

pub trait Validator {
    fn name(&self) -> &'static str;
    fn validate(&self, bundle: &ProjectBundle, report: &mut ValidationReport);
}

pub struct CompositeValidator {
    validators: Vec<Box<dyn Validator>>,
}

impl Default for CompositeValidator {
    fn default() -> Self {
        Self {
            validators: vec![
                Box::new(ManifestValidator),
                Box::new(SceneValidator),
                Box::new(AssetValidator),
                Box::new(DialogueValidator),
            ],
        }
    }
}

impl CompositeValidator {
    pub fn validate(&self, bundle: &ProjectBundle) -> ValidationReport {
        let mut report = ValidationReport::default();

        for validator in &self.validators {
            validator.validate(bundle, &mut report);
        }

        report.budgets = estimate_budgets(bundle);
        report
    }
}

pub fn validate_project(bundle: &ProjectBundle) -> ValidationReport {
    CompositeValidator::default().validate(bundle)
}

struct ManifestValidator;
struct SceneValidator;
struct AssetValidator;
struct DialogueValidator;

impl Validator for ManifestValidator {
    fn name(&self) -> &'static str {
        "manifest"
    }

    fn validate(&self, bundle: &ProjectBundle, report: &mut ValidationReport) {
        if bundle.manifest.build.rom_bank_count == 0 {
            report.push(
                Severity::Error,
                "manifest.zero_rom_banks",
                "build.rom_bank_count must be at least 1",
                Some("project.toml".to_string()),
            );
        }

        if bundle.manifest.meta.default_genre != GenreKind::SideScroller {
            report.push(
                Severity::Warning,
                "manifest.experimental_genre",
                "only the side-scroller runtime path is implemented in the initial milestone",
                Some("project.toml".to_string()),
            );
        }

        if bundle
            .scene(&bundle.manifest.gameplay.entry_scene)
            .is_none()
        {
            report.push(
                Severity::Error,
                "manifest.missing_entry_scene",
                format!(
                    "entry scene '{}' does not exist",
                    bundle.manifest.gameplay.entry_scene
                ),
                Some("project.toml".to_string()),
            );
        }

        let mut preset_ids = BTreeSet::new();
        for preset in &bundle.manifest.gameplay.physics_presets {
            if !preset_ids.insert(preset.id.as_str()) {
                report.push(
                    Severity::Error,
                    "manifest.duplicate_physics",
                    format!("physics preset '{}' is duplicated", preset.id),
                    Some("project.toml".to_string()),
                );
            }
        }

        if bundle
            .manifest
            .gameplay
            .physics_presets
            .iter()
            .all(|preset| preset.id != "megaman_like")
        {
            report.push(
                Severity::Warning,
                "manifest.megaman_preset_missing",
                "the default milestone expects a 'megaman_like' preset to exist",
                Some("project.toml".to_string()),
            );
        }

        let player = &bundle.manifest.gameplay.player;
        if player.max_health == 0 {
            report.push(
                Severity::Error,
                "manifest.player_health_zero",
                "gameplay.player.max_health must be at least 1",
                Some("project.toml".to_string()),
            );
        }
        if player.starting_health == 0 {
            report.push(
                Severity::Error,
                "manifest.player_starting_health_zero",
                "gameplay.player.starting_health must be at least 1",
                Some("project.toml".to_string()),
            );
        }
        if player.starting_health > player.max_health {
            report.push(
                Severity::Error,
                "manifest.player_starting_health_overflow",
                format!(
                    "gameplay.player.starting_health ({}) cannot exceed gameplay.player.max_health ({})",
                    player.starting_health, player.max_health
                ),
                Some("project.toml".to_string()),
            );
        }
    }
}

impl Validator for SceneValidator {
    fn name(&self) -> &'static str {
        "scene"
    }

    fn validate(&self, bundle: &ProjectBundle, report: &mut ValidationReport) {
        let mut scene_ids = BTreeSet::new();
        for scene in &bundle.scenes {
            if !scene_ids.insert(scene.id.as_str()) {
                report.push(
                    Severity::Error,
                    "scene.duplicate_id",
                    format!("scene '{}' is defined more than once", scene.id),
                    Some(format!("scene:{}", scene.id)),
                );
            }

            validate_scene_shape(scene, report);
            validate_scene_scripts(scene, bundle, report);
        }
    }
}

impl Validator for AssetValidator {
    fn name(&self) -> &'static str {
        "asset"
    }

    fn validate(&self, bundle: &ProjectBundle, report: &mut ValidationReport) {
        if bundle.palettes.len() > MAX_PALETTES {
            report.push(
                Severity::Error,
                "asset.palette_bank_overflow",
                format!(
                    "project defines {} palettes but only {} are allowed in the initial strict profile",
                    bundle.palettes.len(),
                    MAX_PALETTES
                ),
                None::<String>,
            );
        }

        for palette in &bundle.palettes {
            if palette.colors.len() > MAX_COLORS_PER_PALETTE {
                report.push(
                    Severity::Error,
                    "asset.palette_too_large",
                    format!(
                        "palette '{}' uses {} colors but only {} are allowed",
                        palette.id,
                        palette.colors.len(),
                        MAX_COLORS_PER_PALETTE
                    ),
                    Some(format!("palette:{}", palette.id)),
                );
            }
        }

        for tileset in &bundle.tilesets {
            if bundle.palette(&tileset.palette_id).is_none() {
                report.push(
                    Severity::Error,
                    "asset.missing_palette",
                    format!(
                        "tileset '{}' references unknown palette '{}'",
                        tileset.id, tileset.palette_id
                    ),
                    Some(format!("tileset:{}", tileset.id)),
                );
            }

            if tileset.tiles.len() > MAX_TILESET_TILES {
                report.push(
                    Severity::Error,
                    "asset.tileset_overflow",
                    format!(
                        "tileset '{}' contains {} tiles but only {} are allowed",
                        tileset.id,
                        tileset.tiles.len(),
                        MAX_TILESET_TILES
                    ),
                    Some(format!("tileset:{}", tileset.id)),
                );
            }

            for (index, tile) in tileset.tiles.iter().enumerate() {
                if tile.pixels.len() != MAX_TILE_PIXELS {
                    report.push(
                        Severity::Error,
                        "asset.tile_shape_invalid",
                        format!(
                            "tileset '{}' tile {} has {} pixels; expected {}",
                            tileset.id,
                            index,
                            tile.pixels.len(),
                            MAX_TILE_PIXELS
                        ),
                        Some(format!("tileset:{}", tileset.id)),
                    );
                }
            }
        }

        for metasprite in &bundle.metasprites {
            let pieces = metasprite.pieces.len();
            if pieces > MAX_METASPRITE_TILES_HARD {
                report.push(
                    Severity::Error,
                    "asset.metasprite_hard_overflow",
                    format!(
                        "metasprite '{}' uses {} pieces; the hard limit is {}",
                        metasprite.id, pieces, MAX_METASPRITE_TILES_HARD
                    ),
                    Some(format!("metasprite:{}", metasprite.id)),
                );
            } else if pieces > MAX_METASPRITE_TILES_WARN {
                report.push(
                    Severity::Warning,
                    "asset.metasprite_oam_warning",
                    format!(
                        "metasprite '{}' uses {} pieces; this may exceed a safe OAM budget in motion-heavy scenes",
                        metasprite.id, pieces
                    ),
                    Some(format!("metasprite:{}", metasprite.id)),
                );
            }
        }
    }
}

impl Validator for DialogueValidator {
    fn name(&self) -> &'static str {
        "dialogue"
    }

    fn validate(&self, bundle: &ProjectBundle, report: &mut ValidationReport) {
        let mut ids = BTreeSet::new();
        for dialogue in &bundle.dialogues {
            if !ids.insert(dialogue.id.as_str()) {
                report.push(
                    Severity::Error,
                    "dialogue.duplicate_id",
                    format!("dialogue '{}' is defined more than once", dialogue.id),
                    Some(format!("dialogue:{}", dialogue.id)),
                );
            }

            if dialogue
                .nodes
                .iter()
                .all(|node| node.id != dialogue.opening_node)
            {
                report.push(
                    Severity::Error,
                    "dialogue.opening_node_missing",
                    format!(
                        "dialogue '{}' opening node '{}' does not exist",
                        dialogue.id, dialogue.opening_node
                    ),
                    Some(format!("dialogue:{}", dialogue.id)),
                );
            }
        }
    }
}

fn validate_scene_shape(scene: &SceneResource, report: &mut ValidationReport) {
    if scene.size_tiles.width > MAX_SCENE_WIDTH_TILES
        || scene.size_tiles.height > MAX_SCENE_HEIGHT_TILES
    {
        report.push(
            Severity::Error,
            "scene.bounds_exceeded",
            format!(
                "scene '{}' is {}x{} tiles; the current strict limit is {}x{}",
                scene.id,
                scene.size_tiles.width,
                scene.size_tiles.height,
                MAX_SCENE_WIDTH_TILES,
                MAX_SCENE_HEIGHT_TILES
            ),
            Some(format!("scene:{}", scene.id)),
        );
    }

    if scene.chunk_size_tiles.width > MAX_CHUNK_WIDTH_TILES
        || scene.chunk_size_tiles.height > MAX_CHUNK_HEIGHT_TILES
    {
        report.push(
            Severity::Error,
            "scene.chunk_exceeded",
            format!(
                "scene '{}' chunk size is {}x{} but the maximum is {}x{}",
                scene.id,
                scene.chunk_size_tiles.width,
                scene.chunk_size_tiles.height,
                MAX_CHUNK_WIDTH_TILES,
                MAX_CHUNK_HEIGHT_TILES
            ),
            Some(format!("scene:{}", scene.id)),
        );
    }

    let tile_count = scene.size_tiles.tile_count();
    for layer in &scene.layers {
        if layer.tiles.len() != tile_count {
            report.push(
                Severity::Error,
                "scene.layer_shape_invalid",
                format!(
                    "scene '{}' layer '{}' has {} tiles but expected {}",
                    scene.id,
                    layer.id,
                    layer.tiles.len(),
                    tile_count
                ),
                Some(format!("scene:{}", scene.id)),
            );
        }
    }

    if scene.collision.solids.len() != tile_count
        || scene.collision.ladders.len() != tile_count
        || scene.collision.hazards.len() != tile_count
    {
        report.push(
            Severity::Error,
            "scene.collision_shape_invalid",
            format!(
                "scene '{}' collision layers must all contain exactly {} entries",
                scene.id, tile_count
            ),
            Some(format!("scene:{}", scene.id)),
        );
    }

    let entity_ids: BTreeSet<&str> = scene
        .entities
        .iter()
        .map(|entity| entity.id.as_str())
        .collect();
    for entity in &scene.entities {
        if entity.hitbox.width == 0 || entity.hitbox.height == 0 {
            report.push(
                Severity::Error,
                "scene.entity_hitbox_invalid",
                format!(
                    "scene '{}' entity '{}' must have a non-zero hitbox",
                    scene.id, entity.id
                ),
                Some(format!("scene:{}", scene.id)),
            );
        }

        match entity.kind {
            EntityKind::Enemy => {
                if entity.combat.max_health == 0 {
                    report.push(
                        Severity::Error,
                        "scene.enemy_health_invalid",
                        format!(
                            "scene '{}' enemy '{}' must have combat.max_health >= 1",
                            scene.id, entity.id
                        ),
                        Some(format!("scene:{}", scene.id)),
                    );
                }
                if entity.combat.contact_damage == 0 {
                    report.push(
                        Severity::Warning,
                        "scene.enemy_damage_zero",
                        format!(
                            "scene '{}' enemy '{}' has zero contact damage",
                            scene.id, entity.id
                        ),
                        Some(format!("scene:{}", scene.id)),
                    );
                }
            }
            EntityKind::Pickup | EntityKind::Switch => {
                if matches!(entity.action, EntityAction::None) {
                    report.push(
                        Severity::Warning,
                        "scene.entity_action_missing",
                        format!(
                            "scene '{}' entity '{}' is {:?} but has no action configured",
                            scene.id, entity.id, entity.kind
                        ),
                        Some(format!("scene:{}", scene.id)),
                    );
                }
            }
            EntityKind::Prop | EntityKind::Solid => {}
        }

        if let EntityAction::SetEntityActive {
            target_entity_id, ..
        } = &entity.action
        {
            if !entity_ids.contains(target_entity_id.as_str()) {
                report.push(
                    Severity::Error,
                    "scene.entity_target_missing",
                    format!(
                        "scene '{}' entity '{}' targets missing entity '{}'",
                        scene.id, entity.id, target_entity_id
                    ),
                    Some(format!("scene:{}", scene.id)),
                );
            }
        }
    }
}

fn validate_scene_scripts(
    scene: &SceneResource,
    bundle: &ProjectBundle,
    report: &mut ValidationReport,
) {
    let script_ids: BTreeSet<&str> = scene
        .scripts
        .iter()
        .map(|script| script.id.as_str())
        .collect();

    for trigger in &scene.triggers {
        if !script_ids.contains(trigger.script_id.as_str()) {
            report.push(
                Severity::Error,
                "scene.trigger_missing_script",
                format!(
                    "scene '{}' trigger '{}' references missing script '{}'",
                    scene.id, trigger.id, trigger.script_id
                ),
                Some(format!("scene:{}", scene.id)),
            );
        }
    }

    for script in &scene.scripts {
        for reserved in reserved_unimplemented_commands(script) {
            report.push(
                Severity::Warning,
                "script.reserved_command",
                format!(
                    "scene '{}' script '{}' uses reserved command '{}'; the export pipeline will keep this as a placeholder",
                    scene.id, script.id, reserved
                ),
                Some(format!("scene:{}", scene.id)),
            );
        }

        for command in &script.commands {
            match command {
                EventCommand::ShowDialogue { dialogue_id, .. } => {
                    if bundle.dialogue(dialogue_id).is_none() {
                        report.push(
                            Severity::Error,
                            "script.dialogue_missing",
                            format!(
                                "scene '{}' script '{}' references missing dialogue '{}'",
                                scene.id, script.id, dialogue_id
                            ),
                            Some(format!("scene:{}", scene.id)),
                        );
                    }
                }
                EventCommand::LoadScene { scene_id, .. } => {
                    if bundle.scene(scene_id).is_none() {
                        report.push(
                            Severity::Error,
                            "script.target_scene_missing",
                            format!(
                                "scene '{}' script '{}' references missing scene '{}'",
                                scene.id, script.id, scene_id
                            ),
                            Some(format!("scene:{}", scene.id)),
                        );
                    }
                }
                _ => {}
            }
        }
    }
}

fn estimate_budgets(bundle: &ProjectBundle) -> BuildBudgets {
    let unique_tiles = bundle
        .tilesets
        .iter()
        .map(|tileset| tileset.tiles.len())
        .sum();
    let palette_colors = bundle
        .palettes
        .iter()
        .map(|palette| palette.colors.len())
        .sum();
    let metasprite_piece_peak = bundle
        .metasprites
        .iter()
        .map(|metasprite| metasprite.pieces.len())
        .max()
        .unwrap_or_default();

    let manifest_bytes = toml::to_string(&bundle.manifest)
        .map(|text| text.len())
        .unwrap_or_default();
    let asset_bytes = bundle
        .tilesets
        .iter()
        .map(|tileset| tileset.tiles.len() * 64)
        .sum::<usize>()
        + bundle
            .palettes
            .iter()
            .map(|palette| palette.colors.len() * 4)
            .sum::<usize>()
        + bundle
            .scenes
            .iter()
            .map(|scene| scene.size_tiles.tile_count() * 2)
            .sum::<usize>();
    let estimated_rom_bytes = manifest_bytes + asset_bytes + bundle.scenes.len() * 1024 + 16 * 1024;
    let estimated_rom_banks = estimated_rom_bytes.div_ceil(ROM_BANK_SIZE);

    BuildBudgets {
        unique_tiles,
        palette_colors,
        scene_count: bundle.scenes.len(),
        metasprite_piece_peak,
        estimated_rom_bytes,
        estimated_rom_banks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use snesmaker_project::{PaletteResource, ProjectBundle, RgbaColor};

    #[test]
    fn catches_palette_overflow() {
        let mut bundle = ProjectBundle::default();
        bundle.palettes = vec![PaletteResource {
            id: "overflow".to_string(),
            name: "Overflow".to_string(),
            colors: (0..17)
                .map(|index| RgbaColor {
                    r: index,
                    g: index,
                    b: index,
                    a: 255,
                })
                .collect(),
        }];

        let report = validate_project(&bundle);
        assert!(
            report
                .errors
                .iter()
                .any(|diagnostic| diagnostic.code == "asset.palette_too_large")
        );
    }
}
