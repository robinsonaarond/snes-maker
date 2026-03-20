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
    pub tiles: Vec<Tile8>,
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
    #[serde(default)]
    pub h_flip: bool,
    #[serde(default)]
    pub v_flip: bool,
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

        Ok(bundle)
    }

    pub fn write_template_project(project_root: impl AsRef<Utf8Path>, name: &str) -> Result<()> {
        let project_root = project_root.as_ref();
        fs::create_dir_all(project_root.join("content/scenes"))?;
        fs::create_dir_all(project_root.join("content/dialogues"))?;
        fs::create_dir_all(project_root.join("content/palettes"))?;
        fs::create_dir_all(project_root.join("content/tilesets"))?;
        fs::create_dir_all(project_root.join("content/metasprites"))?;
        fs::create_dir_all(project_root.join("content/animations"))?;
        fs::create_dir_all(project_root.join(PROJECT_SPRITE_SOURCE_DIR))?;

        let slug = slugify(name);
        let mut bundle = demo_bundle();
        bundle.manifest.meta.name = name.to_string();
        bundle.manifest.meta.slug = slug;

        fs::write(
            project_root.join("project.toml"),
            toml::to_string_pretty(&bundle.manifest)?,
        )?;

        write_ron(
            &project_root.join("content/scenes/intro_stage.scene.ron"),
            &bundle.scenes[0],
        )?;
        write_ron(
            &project_root.join("content/dialogues/intro.dialogue.ron"),
            &bundle.dialogues[0],
        )?;
        write_ron(
            &project_root.join("content/palettes/default.palette.ron"),
            &bundle.palettes[0],
        )?;
        write_ron(
            &project_root.join("content/tilesets/default.tileset.ron"),
            &bundle.tilesets[0],
        )?;
        write_ron(
            &project_root.join("content/metasprites/player.metasprite.ron"),
            &bundle.metasprites[0],
        )?;
        write_ron(
            &project_root.join("content/animations/player_idle.animation.ron"),
            &bundle.animations[0],
        )?;

        Ok(())
    }

    pub fn scene(&self, id: &str) -> Option<&SceneResource> {
        self.scenes.iter().find(|scene| scene.id == id)
    }

    pub fn dialogue(&self, id: &str) -> Option<&DialogueGraph> {
        self.dialogues.iter().find(|dialogue| dialogue.id == id)
    }

    pub fn palette(&self, id: &str) -> Option<&PaletteResource> {
        self.palettes.iter().find(|palette| palette.id == id)
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
                    h_flip: false,
                    v_flip: false,
                },
                SpriteTileRef {
                    tile_index: 2,
                    x: 8,
                    y: 0,
                    palette_slot: 0,
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
        assert_eq!(loaded.palettes, bundle.palettes);
        assert_eq!(loaded.tilesets, bundle.tilesets);
        assert_eq!(loaded.metasprites, bundle.metasprites);
        assert_eq!(loaded.animations, bundle.animations);
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
