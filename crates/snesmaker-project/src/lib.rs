use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};
use std::fs;

use anyhow::{Context, Result, anyhow};
use camino::{Utf8Path, Utf8PathBuf};
use ron::ser::PrettyConfig;
use serde::{Deserialize, Serialize};
use snesmaker_events::{DialogueGraph, EventScript, TriggerKind};
use walkdir::WalkDir;

pub const FIXED_POINT_SHIFT: i32 = 8;
pub const NTSC_FPS: u32 = 60;
pub const PROJECT_SPRITE_SOURCE_DIR: &str = "content/sprite_sources";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum GenreKind {
    #[default]
    SideScroller,
    TopDownRpg,
}

impl Display for GenreKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SideScroller => write!(f, "side_scroller"),
            Self::TopDownRpg => write!(f, "top_down_rpg"),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum MapperKind {
    #[default]
    LoRom,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum RegionKind {
    #[default]
    Ntsc,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectManifest {
    pub meta: ProjectMeta,
    pub build: BuildSettings,
    pub editor: EditorSettings,
    pub gameplay: GameplaySettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectMeta {
    pub name: String,
    pub slug: String,
    pub version: String,
    pub default_genre: GenreKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildSettings {
    pub mapper: MapperKind,
    pub region: RegionKind,
    pub rom_bank_count: u16,
    pub output_dir: String,
    pub assembler: AssemblerSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssemblerSettings {
    pub ca65_path: String,
    pub ld65_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EditorSettings {
    pub strict_mode: bool,
    pub show_budget_overlay: bool,
    pub preferred_emulator: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GameplaySettings {
    pub entry_scene: String,
    #[serde(default)]
    pub physics_presets: Vec<PhysicsProfile>,
    #[serde(default)]
    pub player: PlayerSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlayerSettings {
    pub max_health: u8,
    pub starting_health: u8,
    pub health_hud: HealthHudStyle,
}

impl Default for PlayerSettings {
    fn default() -> Self {
        Self {
            max_health: 6,
            starting_health: 6,
            health_hud: HealthHudStyle::MegaPipsTopLeft,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum HealthHudStyle {
    #[default]
    MegaPipsTopLeft,
    HeartsTopRight,
    CellsTopCenter,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PhysicsProfile {
    pub id: String,
    pub family: PhysicsFamily,
    pub gravity_fp: i32,
    pub max_fall_speed_fp: i32,
    pub ground_accel_fp: i32,
    pub air_accel_fp: i32,
    pub max_run_speed_fp: i32,
    pub jump_velocity_fp: i32,
    pub coyote_frames: u8,
    pub jump_buffer_frames: u8,
    pub ladder_speed_fp: i32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum PhysicsFamily {
    #[default]
    MegaManLike,
    MarioLike,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectBundle {
    pub manifest: ProjectManifest,
    pub scenes: Vec<SceneResource>,
    pub dialogues: Vec<DialogueGraph>,
    pub prefabs: Vec<PrefabResource>,
    pub palettes: Vec<PaletteResource>,
    pub tilesets: Vec<TilesetResource>,
    pub metasprites: Vec<MetaspriteResource>,
    pub animations: Vec<AnimationResource>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum SceneKind {
    #[default]
    SideScroller,
    TopDownRpg,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SceneResource {
    pub id: String,
    pub kind: SceneKind,
    pub size_tiles: GridSize,
    pub chunk_size_tiles: GridSize,
    pub background_color_index: u8,
    #[serde(default)]
    pub layers: Vec<TileLayer>,
    pub collision: CollisionLayer,
    #[serde(default)]
    pub spawns: Vec<SpawnPoint>,
    #[serde(default)]
    pub checkpoints: Vec<Checkpoint>,
    #[serde(default)]
    pub entities: Vec<EntityPlacement>,
    #[serde(default)]
    pub triggers: Vec<TriggerVolume>,
    #[serde(default)]
    pub scripts: Vec<EventScript>,
    #[serde(default)]
    pub prefab_instances: Vec<PrefabInstance>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GridSize {
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TileLayer {
    pub id: String,
    pub tileset_id: String,
    pub visible: bool,
    pub parallax_x: u8,
    pub parallax_y: u8,
    pub tiles: Vec<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollisionLayer {
    pub solids: Vec<bool>,
    pub ladders: Vec<bool>,
    pub hazards: Vec<bool>,
}

impl Default for CollisionLayer {
    fn default() -> Self {
        Self {
            solids: Vec::new(),
            ladders: Vec::new(),
            hazards: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpawnPoint {
    pub id: String,
    pub position: PointI16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Checkpoint {
    pub id: String,
    pub position: PointI16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntityPlacement {
    pub id: String,
    pub archetype: String,
    pub position: PointI16,
    pub facing: Facing,
    #[serde(default)]
    pub kind: EntityKind,
    #[serde(default = "default_entity_hitbox")]
    pub hitbox: RectI16,
    #[serde(default)]
    pub movement: MovementPattern,
    #[serde(default)]
    pub combat: CombatProfile,
    #[serde(default)]
    pub action: EntityAction,
    #[serde(default = "default_true")]
    pub active: bool,
    #[serde(default)]
    pub one_shot: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum EntityKind {
    #[default]
    Prop,
    Pickup,
    Enemy,
    Switch,
    Solid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum MovementPattern {
    #[default]
    None,
    Patrol {
        left_offset: i16,
        right_offset: i16,
        speed: u8,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CombatProfile {
    pub max_health: u8,
    pub contact_damage: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum EntityAction {
    #[default]
    None,
    HealPlayer {
        amount: u8,
    },
    SetEntityActive {
        target_entity_id: String,
        active: bool,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum Facing {
    Left,
    #[default]
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TriggerVolume {
    pub id: String,
    pub kind: TriggerKind,
    pub rect: RectI16,
    pub script_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PrefabInstance {
    #[serde(default)]
    pub id: String,
    pub prefab_id: String,
    #[serde(default)]
    pub position: PointI16,
    #[serde(default)]
    pub entity_overrides: Vec<PrefabEntityOverride>,
    #[serde(default)]
    pub trigger_overrides: Vec<PrefabTriggerOverride>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PrefabEntityOverride {
    #[serde(default)]
    pub entity_id: String,
    #[serde(default)]
    pub position: Option<PointI16>,
    #[serde(default)]
    pub facing: Option<Facing>,
    #[serde(default)]
    pub active: Option<bool>,
    #[serde(default)]
    pub one_shot: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PrefabTriggerOverride {
    #[serde(default)]
    pub trigger_id: String,
    #[serde(default)]
    pub position: Option<PointI16>,
    #[serde(default)]
    pub script_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrefabResource {
    pub id: String,
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
    #[serde(default)]
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

impl Default for PrefabResource {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            source_scene_id: None,
            scene_kind: SceneKind::default(),
            size_tiles: GridSize::default(),
            layers: Vec::new(),
            collision: CollisionLayer::default(),
            spawns: Vec::new(),
            checkpoints: Vec::new(),
            entities: Vec::new(),
            triggers: Vec::new(),
        }
    }
}

impl Default for PointI16 {
    fn default() -> Self {
        Self { x: 0, y: 0 }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct PointI16 {
    pub x: i16,
    pub y: i16,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct RectI16 {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

pub fn default_true() -> bool {
    true
}

pub fn default_entity_hitbox() -> RectI16 {
    RectI16 {
        x: 0,
        y: 0,
        width: 16,
        height: 16,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaletteResource {
    pub id: String,
    pub name: String,
    pub colors: Vec<RgbaColor>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct RgbaColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TilesetResource {
    pub id: String,
    pub palette_id: String,
    pub name: String,
    #[serde(default)]
    pub adjacency_rules: Vec<AdjacencyRuleSet>,
    pub tiles: Vec<Tile8>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum AdjacencySource {
    #[default]
    Terrain,
    Ladder,
    Hazard,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AdjacencyRuleSet {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub source: AdjacencySource,
    #[serde(default)]
    pub mask_tiles: BTreeMap<u8, u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Tile8 {
    pub pixels: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MetaspriteResource {
    pub id: String,
    pub palette_id: String,
    pub pieces: Vec<SpriteTileRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpriteTileRef {
    pub tile_index: u16,
    pub x: i16,
    pub y: i16,
    pub palette_slot: u8,
    #[serde(default = "default_sprite_priority")]
    pub priority: u8,
    #[serde(default)]
    pub h_flip: bool,
    #[serde(default)]
    pub v_flip: bool,
}

fn default_sprite_priority() -> u8 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnimationResource {
    pub id: String,
    pub frames: Vec<AnimationFrame>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnimationFrame {
    pub metasprite_id: String,
    pub duration_frames: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompiledScene {
    pub scene_id: String,
    pub genre: SceneKind,
    pub data_bytes: Vec<u8>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

pub trait SceneCompiler {
    fn genre(&self) -> SceneKind;
    fn compile_scene(&self, bundle: &ProjectBundle, scene: &SceneResource)
    -> Result<CompiledScene>;
}

pub trait GenreModule {
    fn id(&self) -> &'static str;
    fn supports(&self, scene: &SceneResource) -> bool;
    fn scene_compiler(&self) -> &dyn SceneCompiler;
}

impl Default for ProjectManifest {
    fn default() -> Self {
        Self {
            meta: ProjectMeta {
                name: "My SNES Game".to_string(),
                slug: "my_snes_game".to_string(),
                version: "0.1.0".to_string(),
                default_genre: GenreKind::SideScroller,
            },
            build: BuildSettings {
                mapper: MapperKind::LoRom,
                region: RegionKind::Ntsc,
                rom_bank_count: 8,
                output_dir: "build".to_string(),
                assembler: AssemblerSettings {
                    ca65_path: "ca65".to_string(),
                    ld65_path: "ld65".to_string(),
                },
            },
            editor: EditorSettings {
                strict_mode: true,
                show_budget_overlay: true,
                preferred_emulator: Some("ares".to_string()),
            },
            gameplay: GameplaySettings {
                entry_scene: "intro_stage".to_string(),
                physics_presets: vec![default_megaman_like_physics()],
                player: PlayerSettings::default(),
            },
        }
    }
}

impl Default for ProjectBundle {
    fn default() -> Self {
        demo_bundle()
    }
}

impl GridSize {
    pub fn tile_count(self) -> usize {
        self.width as usize * self.height as usize
    }
}

impl ProjectBundle {
    pub fn load(project_root: impl AsRef<Utf8Path>) -> Result<Self> {
        let project_root = project_root.as_ref();
        let manifest_path = project_root.join("project.toml");
        let manifest = toml::from_str::<ProjectManifest>(
            &fs::read_to_string(&manifest_path)
                .with_context(|| format!("failed to read {}", manifest_path))?,
        )
        .with_context(|| format!("failed to parse {}", manifest_path))?;

        let mut bundle = Self {
            manifest,
            scenes: Vec::new(),
            dialogues: Vec::new(),
            prefabs: Vec::new(),
            palettes: Vec::new(),
            tilesets: Vec::new(),
            metasprites: Vec::new(),
            animations: Vec::new(),
        };

        let content_root = project_root.join("content");
        if !content_root.exists() {
            return Ok(bundle);
        }

        for entry in WalkDir::new(&content_root)
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_file())
        {
            let path = Utf8PathBuf::from_path_buf(entry.into_path())
                .map_err(|_| anyhow!("non-utf8 content path"))?;

            if path.extension() != Some("ron") {
                continue;
            }

            let text = fs::read_to_string(&path)
                .with_context(|| format!("failed to read content resource {}", path))?;

            let file_name = path
                .file_name()
                .ok_or_else(|| anyhow!("missing file name for {}", path))?;

            if file_name.ends_with(".scene.ron") {
                bundle.scenes.push(
                    ron::from_str(&text).with_context(|| format!("failed to parse {}", path))?,
                );
            } else if file_name.ends_with(".dialogue.ron") {
                bundle.dialogues.push(
                    ron::from_str(&text).with_context(|| format!("failed to parse {}", path))?,
                );
            } else if file_name.ends_with(".prefab.ron") {
                bundle.prefabs.push(
                    ron::from_str(&text).with_context(|| format!("failed to parse {}", path))?,
                );
            } else if file_name.ends_with(".palette.ron") {
                bundle.palettes.push(
                    ron::from_str(&text).with_context(|| format!("failed to parse {}", path))?,
                );
            } else if file_name.ends_with(".tileset.ron") {
                bundle.tilesets.push(
                    ron::from_str(&text).with_context(|| format!("failed to parse {}", path))?,
                );
            } else if file_name.ends_with(".metasprite.ron") {
                bundle.metasprites.push(
                    ron::from_str(&text).with_context(|| format!("failed to parse {}", path))?,
                );
            } else if file_name.ends_with(".animation.ron") {
                bundle.animations.push(
                    ron::from_str(&text).with_context(|| format!("failed to parse {}", path))?,
                );
            }
        }

        bundle.scenes.sort_by(|left, right| left.id.cmp(&right.id));
        bundle
            .dialogues
            .sort_by(|left, right| left.id.cmp(&right.id));
        bundle.prefabs.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.id.cmp(&right.id))
        });
        bundle
            .palettes
            .sort_by(|left, right| left.id.cmp(&right.id));
        bundle
            .tilesets
            .sort_by(|left, right| left.id.cmp(&right.id));
        bundle
            .metasprites
            .sort_by(|left, right| left.id.cmp(&right.id));
        bundle
            .animations
            .sort_by(|left, right| left.id.cmp(&right.id));

        Ok(bundle)
    }

    pub fn write_template_project(project_root: impl AsRef<Utf8Path>, name: &str) -> Result<()> {
        let project_root = project_root.as_ref();
        let slug = slugify(name);
        let mut bundle = demo_bundle();
        bundle.manifest.meta.name = name.to_string();
        bundle.manifest.meta.slug = slug;
        bundle.save(project_root)
    }

    pub fn scene(&self, id: &str) -> Option<&SceneResource> {
        self.scenes.iter().find(|scene| scene.id == id)
    }

    pub fn resolved_scene(&self, id: &str) -> Option<SceneResource> {
        self.scene(id).map(|scene| self.resolve_scene(scene))
    }

    pub fn resolved_scene_by_index(&self, index: usize) -> Option<SceneResource> {
        self.scenes
            .get(index)
            .map(|scene| self.resolve_scene(scene))
    }

    pub fn dialogue(&self, id: &str) -> Option<&DialogueGraph> {
        self.dialogues.iter().find(|dialogue| dialogue.id == id)
    }

    pub fn palette(&self, id: &str) -> Option<&PaletteResource> {
        self.palettes.iter().find(|palette| palette.id == id)
    }

    pub fn prefab(&self, id: &str) -> Option<&PrefabResource> {
        self.prefabs.iter().find(|prefab| prefab.id == id)
    }

    pub fn resolve_scene(&self, scene: &SceneResource) -> SceneResource {
        let mut resolved = scene.clone();
        resolved.prefab_instances.clear();

        let mut spawn_ids = resolved
            .spawns
            .iter()
            .map(|spawn| spawn.id.clone())
            .collect::<BTreeSet<_>>();
        let mut checkpoint_ids = resolved
            .checkpoints
            .iter()
            .map(|checkpoint| checkpoint.id.clone())
            .collect::<BTreeSet<_>>();
        let mut entity_ids = resolved
            .entities
            .iter()
            .map(|entity| entity.id.clone())
            .collect::<BTreeSet<_>>();
        let mut trigger_ids = resolved
            .triggers
            .iter()
            .map(|trigger| trigger.id.clone())
            .collect::<BTreeSet<_>>();

        for instance in &scene.prefab_instances {
            let Some(prefab) = self.prefab(&instance.prefab_id) else {
                continue;
            };

            overlay_prefab_tiles(&mut resolved, prefab, instance.position);

            let mut resolved_entity_id_map = BTreeMap::<String, String>::new();
            for entity in &prefab.entities {
                let mut resolved_entity = entity.clone();
                resolved_entity.position.x = resolved_entity
                    .position
                    .x
                    .saturating_add(instance.position.x);
                resolved_entity.position.y = resolved_entity
                    .position
                    .y
                    .saturating_add(instance.position.y);

                if let Some(override_entry) = instance
                    .entity_overrides
                    .iter()
                    .find(|override_entry| override_entry.entity_id == entity.id)
                {
                    if let Some(position) = override_entry.position {
                        resolved_entity.position = PointI16 {
                            x: position.x.saturating_add(instance.position.x),
                            y: position.y.saturating_add(instance.position.y),
                        };
                    }
                    if let Some(facing) = override_entry.facing {
                        resolved_entity.facing = facing;
                    }
                    if let Some(active) = override_entry.active {
                        resolved_entity.active = active;
                    }
                    if let Some(one_shot) = override_entry.one_shot {
                        resolved_entity.one_shot = one_shot;
                    }
                }

                let desired_id = format!("{}__{}", instance.id, entity.id);
                resolved_entity.id = next_numbered_id(&entity_ids, &desired_id);
                resolved_entity_id_map.insert(entity.id.clone(), resolved_entity.id.clone());
                entity_ids.insert(resolved_entity.id.clone());
                resolved.entities.push(resolved_entity);
            }

            for entity in resolved
                .entities
                .iter_mut()
                .filter(|entity| resolved_entity_id_map.values().any(|id| id == &entity.id))
            {
                if let EntityAction::SetEntityActive {
                    target_entity_id, ..
                } = &mut entity.action
                {
                    if let Some(remapped) = resolved_entity_id_map.get(target_entity_id) {
                        *target_entity_id = remapped.clone();
                    }
                }
            }

            for spawn in &prefab.spawns {
                let mut resolved_spawn = spawn.clone();
                resolved_spawn.id =
                    next_numbered_id(&spawn_ids, &format!("{}__{}", instance.id, spawn.id));
                resolved_spawn.position.x = resolved_spawn
                    .position
                    .x
                    .saturating_add(instance.position.x);
                resolved_spawn.position.y = resolved_spawn
                    .position
                    .y
                    .saturating_add(instance.position.y);
                spawn_ids.insert(resolved_spawn.id.clone());
                resolved.spawns.push(resolved_spawn);
            }

            for checkpoint in &prefab.checkpoints {
                let mut resolved_checkpoint = checkpoint.clone();
                resolved_checkpoint.id = next_numbered_id(
                    &checkpoint_ids,
                    &format!("{}__{}", instance.id, checkpoint.id),
                );
                resolved_checkpoint.position.x = resolved_checkpoint
                    .position
                    .x
                    .saturating_add(instance.position.x);
                resolved_checkpoint.position.y = resolved_checkpoint
                    .position
                    .y
                    .saturating_add(instance.position.y);
                checkpoint_ids.insert(resolved_checkpoint.id.clone());
                resolved.checkpoints.push(resolved_checkpoint);
            }

            for trigger in &prefab.triggers {
                let mut resolved_trigger = trigger.clone();
                resolved_trigger.id =
                    next_numbered_id(&trigger_ids, &format!("{}__{}", instance.id, trigger.id));
                resolved_trigger.rect.x =
                    resolved_trigger.rect.x.saturating_add(instance.position.x);
                resolved_trigger.rect.y =
                    resolved_trigger.rect.y.saturating_add(instance.position.y);

                if let Some(override_entry) = instance
                    .trigger_overrides
                    .iter()
                    .find(|override_entry| override_entry.trigger_id == trigger.id)
                {
                    if let Some(position) = override_entry.position {
                        resolved_trigger.rect.x = position.x.saturating_add(instance.position.x);
                        resolved_trigger.rect.y = position.y.saturating_add(instance.position.y);
                    }
                    if let Some(script_id) = &override_entry.script_id {
                        resolved_trigger.script_id = script_id.clone();
                    }
                }

                trigger_ids.insert(resolved_trigger.id.clone());
                resolved.triggers.push(resolved_trigger);
            }
        }

        resolved
    }

    pub fn tileset(&self, id: &str) -> Option<&TilesetResource> {
        self.tilesets.iter().find(|tileset| tileset.id == id)
    }

    pub fn metasprite(&self, id: &str) -> Option<&MetaspriteResource> {
        self.metasprites
            .iter()
            .find(|metasprite| metasprite.id == id)
    }

    pub fn animation(&self, id: &str) -> Option<&AnimationResource> {
        self.animations.iter().find(|animation| animation.id == id)
    }

    pub fn save(&self, project_root: impl AsRef<Utf8Path>) -> Result<()> {
        let project_root = project_root.as_ref();
        fs::create_dir_all(project_root.join("content/scenes"))?;
        fs::create_dir_all(project_root.join("content/dialogues"))?;
        fs::create_dir_all(project_root.join("content/prefabs"))?;
        fs::create_dir_all(project_root.join("content/palettes"))?;
        fs::create_dir_all(project_root.join("content/tilesets"))?;
        fs::create_dir_all(project_root.join("content/metasprites"))?;
        fs::create_dir_all(project_root.join("content/animations"))?;
        fs::create_dir_all(project_root.join(PROJECT_SPRITE_SOURCE_DIR))?;

        fs::write(
            project_root.join("project.toml"),
            toml::to_string_pretty(&self.manifest)?,
        )?;

        rewrite_ron_group(
            &project_root.join("content/scenes"),
            ".scene.ron",
            &self.scenes,
            |scene| &scene.id,
        )?;
        rewrite_ron_group(
            &project_root.join("content/dialogues"),
            ".dialogue.ron",
            &self.dialogues,
            |dialogue| &dialogue.id,
        )?;
        rewrite_ron_group(
            &project_root.join("content/prefabs"),
            ".prefab.ron",
            &self.prefabs,
            |prefab| &prefab.id,
        )?;
        rewrite_ron_group(
            &project_root.join("content/palettes"),
            ".palette.ron",
            &self.palettes,
            |palette| &palette.id,
        )?;
        rewrite_ron_group(
            &project_root.join("content/tilesets"),
            ".tileset.ron",
            &self.tilesets,
            |tileset| &tileset.id,
        )?;
        rewrite_ron_group(
            &project_root.join("content/metasprites"),
            ".metasprite.ron",
            &self.metasprites,
            |metasprite| &metasprite.id,
        )?;
        rewrite_ron_group(
            &project_root.join("content/animations"),
            ".animation.ron",
            &self.animations,
            |animation| &animation.id,
        )?;

        Ok(())
    }

    pub fn unique_ids(&self) -> BTreeSet<&str> {
        let mut ids = BTreeSet::new();

        ids.extend(self.scenes.iter().map(|scene| scene.id.as_str()));
        ids.extend(self.dialogues.iter().map(|dialogue| dialogue.id.as_str()));
        ids.extend(self.prefabs.iter().map(|prefab| prefab.id.as_str()));
        ids.extend(self.palettes.iter().map(|palette| palette.id.as_str()));
        ids.extend(self.tilesets.iter().map(|tileset| tileset.id.as_str()));
        ids.extend(
            self.metasprites
                .iter()
                .map(|metasprite| metasprite.id.as_str()),
        );
        ids.extend(
            self.animations
                .iter()
                .map(|animation| animation.id.as_str()),
        );

        ids
    }
}

fn overlay_prefab_tiles(scene: &mut SceneResource, prefab: &PrefabResource, origin: PointI16) {
    let tile_offset_x = (origin.x.max(0) as usize) / 8;
    let tile_offset_y = (origin.y.max(0) as usize) / 8;
    let scene_width = scene.size_tiles.width as usize;
    let scene_height = scene.size_tiles.height as usize;

    for prefab_layer in &prefab.layers {
        let Some(scene_layer) = scene
            .layers
            .iter_mut()
            .find(|layer| layer.id == prefab_layer.id)
        else {
            continue;
        };
        for local_y in 0..prefab.size_tiles.height as usize {
            let world_y = tile_offset_y + local_y;
            if world_y >= scene_height {
                continue;
            }
            for local_x in 0..prefab.size_tiles.width as usize {
                let world_x = tile_offset_x + local_x;
                if world_x >= scene_width {
                    continue;
                }
                let prefab_index = local_y * prefab.size_tiles.width as usize + local_x;
                let scene_index = world_y * scene_width + world_x;
                if let Some(tile) = prefab_layer.tiles.get(prefab_index) {
                    scene_layer.tiles[scene_index] = *tile;
                }
                if let Some(value) = prefab.collision.solids.get(prefab_index) {
                    scene.collision.solids[scene_index] = *value;
                }
                if let Some(value) = prefab.collision.ladders.get(prefab_index) {
                    scene.collision.ladders[scene_index] = *value;
                }
                if let Some(value) = prefab.collision.hazards.get(prefab_index) {
                    scene.collision.hazards[scene_index] = *value;
                }
            }
        }
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

fn rewrite_ron_group<T: Serialize>(
    directory: &Utf8Path,
    suffix: &str,
    items: &[T],
    id_for: impl Fn(&T) -> &str,
) -> Result<()> {
    if directory.exists() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = Utf8PathBuf::from_path_buf(entry.path())
                .map_err(|_| anyhow!("non-utf8 resource path in {}", directory))?;
            let Some(file_name) = path.file_name() else {
                continue;
            };
            if file_name.ends_with(suffix) {
                fs::remove_file(&path)
                    .with_context(|| format!("failed to remove stale resource {}", path))?;
            }
        }
    }

    for item in items {
        let file_name = format!("{}{}", slugify(id_for(item)), suffix);
        write_ron(&directory.join(file_name), item)?;
    }

    Ok(())
}

pub fn default_megaman_like_physics() -> PhysicsProfile {
    PhysicsProfile {
        id: "megaman_like".to_string(),
        family: PhysicsFamily::MegaManLike,
        gravity_fp: fp(0.28),
        max_fall_speed_fp: fp(4.0),
        ground_accel_fp: fp(0.35),
        air_accel_fp: fp(0.22),
        max_run_speed_fp: fp(1.75),
        jump_velocity_fp: fp(-4.1),
        coyote_frames: 4,
        jump_buffer_frames: 4,
        ladder_speed_fp: fp(1.0),
    }
}

pub fn fp(value: f32) -> i32 {
    (value * ((1 << FIXED_POINT_SHIFT) as f32)).round() as i32
}

pub fn write_ron<T: Serialize>(path: &Utf8Path, value: &T) -> Result<()> {
    let text = ron::ser::to_string_pretty(value, PrettyConfig::new())?;
    fs::write(path, text)?;
    Ok(())
}

pub fn slugify(name: &str) -> String {
    name.to_ascii_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

fn vertical_edge_mask_tiles(top_tile: u16, body_tile: u16) -> BTreeMap<u8, u16> {
    (0_u8..16)
        .map(|mask| {
            (
                mask,
                if mask & 0b0100 == 0 {
                    top_tile
                } else {
                    body_tile
                },
            )
        })
        .collect()
}

fn default_tileset_adjacency_rules() -> Vec<AdjacencyRuleSet> {
    vec![
        AdjacencyRuleSet {
            id: "terrain_ground".to_string(),
            name: "Terrain Ground".to_string(),
            source: AdjacencySource::Terrain,
            mask_tiles: vertical_edge_mask_tiles(1, 2),
        },
        AdjacencyRuleSet {
            id: "ladder_column".to_string(),
            name: "Ladder Column".to_string(),
            source: AdjacencySource::Ladder,
            mask_tiles: vertical_edge_mask_tiles(3, 4),
        },
        AdjacencyRuleSet {
            id: "hazard_strip".to_string(),
            name: "Hazard Strip".to_string(),
            source: AdjacencySource::Hazard,
            mask_tiles: vertical_edge_mask_tiles(5, 6),
        },
    ]
}

fn demo_ladder_tile(top: bool) -> Tile8 {
    let mut pixels = vec![0; 64];
    for y in 0..8 {
        pixels[y * 8 + 2] = 3;
        pixels[y * 8 + 5] = 3;
        if top && y == 0 {
            for x in 1..7 {
                pixels[x] = 1;
            }
        } else if y == 2 || y == 5 {
            for x in 2..6 {
                pixels[y * 8 + x] = 2;
            }
        }
    }
    Tile8 { pixels }
}

fn demo_hazard_tile(top: bool) -> Tile8 {
    let mut pixels = vec![0; 64];
    for y in 0..8 {
        for x in 0..8 {
            if top && y < 2 {
                pixels[y * 8 + x] = if (x + y) % 2 == 0 { 3 } else { 1 };
            } else if y >= 2 {
                pixels[y * 8 + x] = if (x + y) % 2 == 0 { 1 } else { 3 };
            }
        }
    }
    Tile8 { pixels }
}

fn starter_prefabs() -> Vec<PrefabResource> {
    let mut prefabs = vec![
        PrefabResource {
            id: "floor_chunk_wide".to_string(),
            name: "Floor Chunk Wide".to_string(),
            source_scene_id: Some("intro_stage".to_string()),
            scene_kind: SceneKind::SideScroller,
            size_tiles: GridSize {
                width: 4,
                height: 2,
            },
            layers: vec![TileLayer {
                id: "bg".to_string(),
                tileset_id: "default_tiles".to_string(),
                visible: true,
                parallax_x: 1,
                parallax_y: 1,
                tiles: vec![1, 1, 1, 1, 2, 2, 2, 2],
            }],
            collision: CollisionLayer {
                solids: vec![true; 8],
                ladders: vec![false; 8],
                hazards: vec![false; 8],
            },
            ..PrefabResource::default()
        },
        PrefabResource {
            id: "ladder_column".to_string(),
            name: "Ladder Column".to_string(),
            source_scene_id: Some("intro_stage".to_string()),
            scene_kind: SceneKind::SideScroller,
            size_tiles: GridSize {
                width: 2,
                height: 4,
            },
            layers: vec![TileLayer {
                id: "bg".to_string(),
                tileset_id: "default_tiles".to_string(),
                visible: true,
                parallax_x: 1,
                parallax_y: 1,
                tiles: vec![0, 0, 0, 0, 1, 0, 2, 0],
            }],
            collision: CollisionLayer {
                solids: vec![false; 8],
                ladders: vec![false, true, false, true, false, true, false, true],
                hazards: vec![false; 8],
            },
            ..PrefabResource::default()
        },
        PrefabResource {
            id: "checkpoint_stop".to_string(),
            name: "Checkpoint Stop".to_string(),
            source_scene_id: Some("intro_stage".to_string()),
            scene_kind: SceneKind::SideScroller,
            size_tiles: GridSize {
                width: 3,
                height: 2,
            },
            layers: vec![TileLayer {
                id: "bg".to_string(),
                tileset_id: "default_tiles".to_string(),
                visible: true,
                parallax_x: 1,
                parallax_y: 1,
                tiles: vec![1, 1, 1, 2, 2, 2],
            }],
            collision: CollisionLayer {
                solids: vec![true; 6],
                ladders: vec![false; 6],
                hazards: vec![false; 6],
            },
            spawns: vec![SpawnPoint {
                id: "prefab_spawn".to_string(),
                position: PointI16 { x: 8, y: 0 },
            }],
            checkpoints: vec![Checkpoint {
                id: "prefab_checkpoint".to_string(),
                position: PointI16 { x: 16, y: 0 },
            }],
            ..PrefabResource::default()
        },
        PrefabResource {
            id: "npc_hint_spot".to_string(),
            name: "NPC Hint Spot".to_string(),
            source_scene_id: Some("intro_stage".to_string()),
            scene_kind: SceneKind::SideScroller,
            size_tiles: GridSize {
                width: 3,
                height: 2,
            },
            layers: vec![TileLayer {
                id: "bg".to_string(),
                tileset_id: "default_tiles".to_string(),
                visible: true,
                parallax_x: 1,
                parallax_y: 1,
                tiles: vec![1, 1, 1, 2, 2, 2],
            }],
            collision: CollisionLayer {
                solids: vec![true; 6],
                ladders: vec![false; 6],
                hazards: vec![false; 6],
            },
            entities: vec![EntityPlacement {
                id: "guide".to_string(),
                archetype: "guide_bot".to_string(),
                position: PointI16 { x: 8, y: 0 },
                facing: Facing::Right,
                kind: EntityKind::Prop,
                hitbox: default_entity_hitbox(),
                movement: MovementPattern::None,
                combat: CombatProfile::default(),
                action: EntityAction::None,
                active: true,
                one_shot: false,
            }],
            triggers: vec![TriggerVolume {
                id: "hint_trigger".to_string(),
                kind: TriggerKind::Touch,
                rect: RectI16 {
                    x: 0,
                    y: 0,
                    width: 24,
                    height: 16,
                },
                script_id: "start_dialogue".to_string(),
            }],
            ..PrefabResource::default()
        },
        PrefabResource {
            id: "alarm_switch_pack".to_string(),
            name: "Alarm Switch Pack".to_string(),
            source_scene_id: Some("intro_stage".to_string()),
            scene_kind: SceneKind::SideScroller,
            size_tiles: GridSize {
                width: 4,
                height: 2,
            },
            layers: vec![TileLayer {
                id: "bg".to_string(),
                tileset_id: "default_tiles".to_string(),
                visible: true,
                parallax_x: 1,
                parallax_y: 1,
                tiles: vec![1, 1, 1, 1, 2, 2, 2, 2],
            }],
            collision: CollisionLayer {
                solids: vec![true; 8],
                ladders: vec![false; 8],
                hazards: vec![false; 8],
            },
            entities: vec![
                EntityPlacement {
                    id: "guide_switch".to_string(),
                    archetype: "guide_bot".to_string(),
                    position: PointI16 { x: 8, y: 0 },
                    facing: Facing::Right,
                    kind: EntityKind::Switch,
                    hitbox: default_entity_hitbox(),
                    movement: MovementPattern::None,
                    combat: CombatProfile::default(),
                    action: EntityAction::SetEntityActive {
                        target_entity_id: "guard_met".to_string(),
                        active: true,
                    },
                    active: true,
                    one_shot: true,
                },
                EntityPlacement {
                    id: "guard_met".to_string(),
                    archetype: "met_enemy".to_string(),
                    position: PointI16 { x: 16, y: 0 },
                    facing: Facing::Left,
                    kind: EntityKind::Enemy,
                    hitbox: default_entity_hitbox(),
                    movement: MovementPattern::Patrol {
                        left_offset: -8,
                        right_offset: 8,
                        speed: 1,
                    },
                    combat: CombatProfile {
                        max_health: 2,
                        contact_damage: 1,
                    },
                    action: EntityAction::None,
                    active: false,
                    one_shot: false,
                },
            ],
            triggers: vec![TriggerVolume {
                id: "alarm_prompt".to_string(),
                kind: TriggerKind::Touch,
                rect: RectI16 {
                    x: 0,
                    y: 0,
                    width: 32,
                    height: 16,
                },
                script_id: "start_dialogue".to_string(),
            }],
            ..PrefabResource::default()
        },
    ];
    prefabs.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.id.cmp(&right.id))
    });
    prefabs
}

pub fn demo_bundle() -> ProjectBundle {
    let scene_size = GridSize {
        width: 16,
        height: 12,
    };

    let mut ground_tiles = vec![0_u16; scene_size.tile_count()];
    let mut solids = vec![false; scene_size.tile_count()];
    let mut ladders = vec![false; scene_size.tile_count()];
    let mut hazards = vec![false; scene_size.tile_count()];

    for x in 0..scene_size.width as usize {
        ground_tiles[(scene_size.height as usize - 2) * scene_size.width as usize + x] = 1;
        ground_tiles[(scene_size.height as usize - 1) * scene_size.width as usize + x] = 2;
        solids[(scene_size.height as usize - 2) * scene_size.width as usize + x] = true;
        solids[(scene_size.height as usize - 1) * scene_size.width as usize + x] = true;
    }

    for y in 4..10 {
        ladders[y * scene_size.width as usize + 9] = true;
    }

    hazards[(scene_size.height as usize - 1) * scene_size.width as usize + 14] = true;

    ProjectBundle {
        manifest: ProjectManifest::default(),
        scenes: vec![SceneResource {
            id: "intro_stage".to_string(),
            kind: SceneKind::SideScroller,
            size_tiles: scene_size,
            chunk_size_tiles: GridSize {
                width: 16,
                height: 12,
            },
            background_color_index: 0,
            layers: vec![TileLayer {
                id: "bg".to_string(),
                tileset_id: "default_tiles".to_string(),
                visible: true,
                parallax_x: 1,
                parallax_y: 1,
                tiles: ground_tiles,
            }],
            collision: CollisionLayer {
                solids,
                ladders,
                hazards,
            },
            spawns: vec![SpawnPoint {
                id: "start".to_string(),
                position: PointI16 { x: 24, y: 96 },
            }],
            checkpoints: vec![Checkpoint {
                id: "midpoint".to_string(),
                position: PointI16 { x: 88, y: 96 },
            }],
            entities: vec![
                EntityPlacement {
                    id: "met".to_string(),
                    archetype: "met_enemy".to_string(),
                    position: PointI16 { x: 120, y: 96 },
                    facing: Facing::Left,
                    kind: EntityKind::Enemy,
                    hitbox: default_entity_hitbox(),
                    movement: MovementPattern::Patrol {
                        left_offset: -24,
                        right_offset: 24,
                        speed: 1,
                    },
                    combat: CombatProfile {
                        max_health: 3,
                        contact_damage: 1,
                    },
                    action: EntityAction::None,
                    active: true,
                    one_shot: false,
                },
                EntityPlacement {
                    id: "npc_guide".to_string(),
                    archetype: "guide_bot".to_string(),
                    position: PointI16 { x: 48, y: 96 },
                    facing: Facing::Right,
                    kind: EntityKind::Prop,
                    hitbox: default_entity_hitbox(),
                    movement: MovementPattern::None,
                    combat: CombatProfile::default(),
                    action: EntityAction::None,
                    active: true,
                    one_shot: false,
                },
            ],
            triggers: vec![
                TriggerVolume {
                    id: "intro_dialogue".to_string(),
                    kind: TriggerKind::Touch,
                    rect: RectI16 {
                        x: 40,
                        y: 80,
                        width: 24,
                        height: 24,
                    },
                    script_id: "start_dialogue".to_string(),
                },
                TriggerVolume {
                    id: "goal".to_string(),
                    kind: TriggerKind::Touch,
                    rect: RectI16 {
                        x: 224,
                        y: 64,
                        width: 16,
                        height: 48,
                    },
                    script_id: "stage_clear".to_string(),
                },
            ],
            scripts: vec![
                EventScript {
                    id: "start_dialogue".to_string(),
                    commands: vec![
                        snesmaker_events::EventCommand::FreezePlayer { frozen: true },
                        snesmaker_events::EventCommand::ShowDialogue {
                            dialogue_id: "intro".to_string(),
                            node_id: None,
                        },
                        snesmaker_events::EventCommand::FreezePlayer { frozen: false },
                    ],
                },
                EventScript {
                    id: "stage_clear".to_string(),
                    commands: vec![
                        snesmaker_events::EventCommand::EmitCheckpoint {
                            checkpoint_id: "midpoint".to_string(),
                        },
                        snesmaker_events::EventCommand::SetFlag {
                            flag: "intro_stage_clear".to_string(),
                            value: true,
                        },
                    ],
                },
            ],
            prefab_instances: Vec::new(),
        }],
        dialogues: vec![DialogueGraph {
            id: "intro".to_string(),
            opening_node: "greeting".to_string(),
            nodes: vec![
                snesmaker_events::DialogueNode {
                    id: "greeting".to_string(),
                    speaker: "Guide".to_string(),
                    text: "Welcome to SNES Maker. Reach the gate to finish the stage.".to_string(),
                    commands: vec![],
                    choices: vec![],
                    next: Some("hint".to_string()),
                },
                snesmaker_events::DialogueNode {
                    id: "hint".to_string(),
                    speaker: "Guide".to_string(),
                    text: "Climb the ladder, avoid the hazard, and use checkpoints.".to_string(),
                    commands: vec![],
                    choices: vec![],
                    next: None,
                },
            ],
        }],
        prefabs: starter_prefabs(),
        palettes: vec![PaletteResource {
            id: "default_palette".to_string(),
            name: "Default".to_string(),
            colors: vec![
                RgbaColor {
                    r: 12,
                    g: 18,
                    b: 32,
                    a: 255,
                },
                RgbaColor {
                    r: 128,
                    g: 216,
                    b: 255,
                    a: 255,
                },
                RgbaColor {
                    r: 239,
                    g: 243,
                    b: 248,
                    a: 255,
                },
                RgbaColor {
                    r: 82,
                    g: 198,
                    b: 108,
                    a: 255,
                },
            ],
        }],
        tilesets: vec![TilesetResource {
            id: "default_tiles".to_string(),
            palette_id: "default_palette".to_string(),
            name: "Default Tiles".to_string(),
            adjacency_rules: default_tileset_adjacency_rules(),
            tiles: vec![
                Tile8 {
                    pixels: vec![0; 64],
                },
                Tile8 {
                    pixels: vec![
                        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                        0, 0, 3, 3, 0, 0, 0, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 2, 2, 2, 2, 3, 3, 2, 2,
                        2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
                    ],
                },
                Tile8 {
                    pixels: vec![
                        2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3, 3, 3, 3, 2, 2, 2, 3, 3, 2, 2, 3, 3, 2, 2,
                        3, 2, 2, 2, 2, 3, 2, 2, 3, 2, 2, 2, 2, 3, 2, 2, 3, 3, 2, 2, 3, 3, 2, 2, 2,
                        3, 3, 3, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
                    ],
                },
                demo_ladder_tile(true),
                demo_ladder_tile(false),
                demo_hazard_tile(true),
                demo_hazard_tile(false),
            ],
        }],
        metasprites: vec![MetaspriteResource {
            id: "player_idle".to_string(),
            palette_id: "default_palette".to_string(),
            pieces: vec![
                SpriteTileRef {
                    tile_index: 1,
                    x: 0,
                    y: 0,
                    palette_slot: 0,
                    priority: 3,
                    h_flip: false,
                    v_flip: false,
                },
                SpriteTileRef {
                    tile_index: 2,
                    x: 8,
                    y: 0,
                    palette_slot: 0,
                    priority: 3,
                    h_flip: false,
                    v_flip: false,
                },
            ],
        }],
        animations: vec![AnimationResource {
            id: "player_idle".to_string(),
            frames: vec![AnimationFrame {
                metasprite_id: "player_idle".to_string(),
                duration_frames: 12,
            }],
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use snesmaker_events::{DialogueChoice, EventCommand};

    #[test]
    fn round_trips_manifest_toml() {
        let manifest = ProjectManifest::default();
        let text = toml::to_string_pretty(&manifest).expect("serialize manifest");
        let parsed: ProjectManifest = toml::from_str(&text).expect("parse manifest");
        assert_eq!(manifest, parsed);
    }

    #[test]
    fn writes_and_loads_template_project() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 tempdir");
        ProjectBundle::write_template_project(&root, "Test Project").expect("write project");
        let loaded = ProjectBundle::load(&root).expect("load project");
        assert_eq!(loaded.manifest.meta.name, "Test Project");
        assert!(loaded.scene("intro_stage").is_some());
        assert!(loaded.dialogue("intro").is_some());
        assert!(loaded.prefab("alarm_switch_pack").is_some());
    }

    #[test]
    fn saves_and_loads_bundle_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 tempdir");
        let bundle = demo_bundle();

        bundle.save(&root).expect("save bundle");
        let loaded = ProjectBundle::load(&root).expect("load saved bundle");

        assert_eq!(loaded.manifest, bundle.manifest);
        assert_eq!(loaded.scenes, bundle.scenes);
        assert_eq!(loaded.dialogues, bundle.dialogues);
        assert_eq!(loaded.prefabs, bundle.prefabs);
        assert_eq!(loaded.palettes, bundle.palettes);
        assert_eq!(loaded.tilesets, bundle.tilesets);
        assert_eq!(loaded.metasprites, bundle.metasprites);
        assert_eq!(loaded.animations, bundle.animations);
    }

    #[test]
    fn saves_and_loads_prefabs_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 tempdir");
        let mut bundle = demo_bundle();
        bundle.prefabs.push(PrefabResource {
            id: "room_chunk".to_string(),
            name: "Room Chunk".to_string(),
            source_scene_id: Some("intro_stage".to_string()),
            scene_kind: SceneKind::SideScroller,
            size_tiles: GridSize {
                width: 2,
                height: 2,
            },
            layers: vec![TileLayer {
                id: "bg".to_string(),
                tileset_id: "default_tiles".to_string(),
                visible: true,
                parallax_x: 1,
                parallax_y: 1,
                tiles: vec![1, 2, 0, 1],
            }],
            collision: CollisionLayer {
                solids: vec![true, true, false, false],
                ladders: vec![false, false, false, true],
                hazards: vec![false, false, false, false],
            },
            spawns: vec![SpawnPoint {
                id: "prefab_spawn".to_string(),
                position: PointI16 { x: 8, y: 8 },
            }],
            checkpoints: Vec::new(),
            entities: Vec::new(),
            triggers: vec![TriggerVolume {
                id: "prefab_trigger".to_string(),
                kind: TriggerKind::Touch,
                rect: RectI16 {
                    x: 0,
                    y: 0,
                    width: 16,
                    height: 16,
                },
                script_id: "start_dialogue".to_string(),
            }],
        });

        bundle.save(&root).expect("save bundle");
        let loaded = ProjectBundle::load(&root).expect("load saved bundle");

        assert_eq!(loaded.prefabs.len(), bundle.prefabs.len());
        assert_eq!(
            loaded.prefab("room_chunk"),
            bundle
                .prefabs
                .iter()
                .find(|prefab| prefab.id == "room_chunk")
        );
    }

    #[test]
    fn preserves_event_links_through_save_and_load() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 tempdir");
        let mut bundle = demo_bundle();
        bundle.dialogues[0].nodes[0].choices.push(DialogueChoice {
            text: "Need a hint".to_string(),
            next: "hint".to_string(),
            condition_flag: Some("met_guide".to_string()),
        });
        if let EventCommand::ShowDialogue { node_id, .. } =
            &mut bundle.scenes[0].scripts[0].commands[1]
        {
            *node_id = Some("hint".to_string());
        }

        bundle.save(&root).expect("save bundle");
        let loaded = ProjectBundle::load(&root).expect("load saved bundle");

        assert_eq!(loaded.scenes[0].triggers[0].script_id, "start_dialogue");
        assert_eq!(loaded.scenes[0].scripts[0].id, "start_dialogue");
        assert_eq!(
            loaded.scenes[0].scripts[0].commands[1],
            EventCommand::ShowDialogue {
                dialogue_id: "intro".to_string(),
                node_id: Some("hint".to_string()),
            }
        );
        assert_eq!(loaded.dialogues[0].opening_node, "greeting");
        assert_eq!(loaded.dialogues[0].nodes[0].choices.len(), 1);
        assert_eq!(loaded.dialogues[0].nodes[0].choices[0].next, "hint");
    }

    #[test]
    fn resolves_prefab_instances_with_overrides() {
        let mut bundle = demo_bundle();
        bundle.scenes[0].prefab_instances.push(PrefabInstance {
            id: "alarm_pack_a".to_string(),
            prefab_id: "alarm_switch_pack".to_string(),
            position: PointI16 { x: 32, y: 64 },
            entity_overrides: vec![
                PrefabEntityOverride {
                    entity_id: "guard_met".to_string(),
                    position: Some(PointI16 { x: 24, y: 0 }),
                    facing: Some(Facing::Right),
                    active: Some(true),
                    one_shot: None,
                },
                PrefabEntityOverride {
                    entity_id: "guide_switch".to_string(),
                    position: None,
                    facing: None,
                    active: None,
                    one_shot: Some(false),
                },
            ],
            trigger_overrides: vec![PrefabTriggerOverride {
                trigger_id: "alarm_prompt".to_string(),
                position: Some(PointI16 { x: 4, y: -8 }),
                script_id: Some("stage_clear".to_string()),
            }],
        });

        let resolved = bundle
            .resolved_scene("intro_stage")
            .expect("resolved intro scene");

        assert!(resolved.prefab_instances.is_empty());

        let switch = resolved
            .entities
            .iter()
            .find(|entity| entity.id == "alarm_pack_a_guide_switch")
            .expect("resolved switch entity");
        let guard = resolved
            .entities
            .iter()
            .find(|entity| entity.id == "alarm_pack_a_guard_met")
            .expect("resolved guard entity");
        let prompt = resolved
            .triggers
            .iter()
            .find(|trigger| trigger.id == "alarm_pack_a_alarm_prompt")
            .expect("resolved alarm trigger");

        assert_eq!(guard.position, PointI16 { x: 56, y: 64 });
        assert_eq!(guard.facing, Facing::Right);
        assert!(guard.active);
        assert!(!switch.one_shot);
        assert_eq!(prompt.rect.x, 36);
        assert_eq!(prompt.rect.y, 56);
        assert_eq!(prompt.script_id, "stage_clear");
        assert!(matches!(
            &switch.action,
            EntityAction::SetEntityActive {
                target_entity_id,
                active: true,
            } if target_entity_id == "alarm_pack_a_guard_met"
        ));

        let tile_index = 8 * bundle.scenes[0].size_tiles.width as usize + 4;
        assert_eq!(resolved.layers[0].tiles[tile_index], 1);
        assert!(resolved.collision.solids[tile_index]);
    }

    #[test]
    fn loads_legacy_metasprite_without_priority() {
        let metasprite: MetaspriteResource = ron::from_str(
            r#"
            (
                id: "legacy",
                palette_id: "default_palette",
                pieces: [
                    (
                        tile_index: 4,
                        x: -8,
                        y: 8,
                        palette_slot: 2,
                        h_flip: true,
                        v_flip: false,
                    ),
                ],
            )
            "#,
        )
        .expect("deserialize legacy metasprite");

        assert_eq!(metasprite.pieces.len(), 1);
        assert_eq!(metasprite.pieces[0].priority, 3);
    }

    #[test]
    fn ignores_non_ron_content_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 tempdir");
        ProjectBundle::write_template_project(&root, "Test Project").expect("write project");
        let sprite_dir = root.join(PROJECT_SPRITE_SOURCE_DIR);
        std::fs::create_dir_all(&sprite_dir).expect("create sprite dir");
        std::fs::write(
            sprite_dir.join("player_idle_sheet.png"),
            [0_u8, 159, 146, 150],
        )
        .expect("write sprite sheet");

        let loaded = ProjectBundle::load(&root).expect("load project with binary asset");
        assert!(loaded.animation("player_idle").is_some());
    }

    #[test]
    fn slugify_collapses_separators() {
        assert_eq!(slugify("Hello, SNES World!"), "hello_snes_world");
    }
}
