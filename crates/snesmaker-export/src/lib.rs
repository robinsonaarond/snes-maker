use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use snesmaker_platformer::PlatformerGenreModule;
use snesmaker_project::{
    AnimationResource, CompiledScene, EntityAction, EntityKind, GenreModule, HealthHudStyle,
    MetaspriteResource, MovementPattern, PaletteResource, ProjectBundle, SceneResource, Tile8,
    TileLayer, TilesetResource,
};
use snesmaker_validator::{ValidationReport, validate_project};

const DISPLAY_MAP_WIDTH_TILES: usize = 64;
const DISPLAY_MAP_HEIGHT_TILES: usize = 32;
const VISIBLE_SCREEN_WIDTH_TILES: usize = 32;
const ENTITY_RUNTIME_BYTES: usize = 24;
const ACTION_NONE: u8 = 0;
const ACTION_HEAL_PLAYER: u8 = 1;
const ACTION_SET_ENTITY_ACTIVE: u8 = 2;
const MOVEMENT_NONE: u8 = 0;
const MOVEMENT_PATROL: u8 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildOutcome {
    pub project_root: Utf8PathBuf,
    pub build_dir: Utf8PathBuf,
    pub rom_path: Utf8PathBuf,
    pub stable_rom_path: Option<Utf8PathBuf>,
    pub report_path: Utf8PathBuf,
    pub rom_built: bool,
    pub validation: ValidationReport,
    pub assembler_status: AssemblerStatus,
    pub compiled_scenes: Vec<CompiledSceneSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AssemblerStatus {
    pub ca65_found: bool,
    pub ld65_found: bool,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompiledSceneSummary {
    pub scene_id: String,
    pub byte_len: usize,
    pub metadata: std::collections::BTreeMap<String, String>,
}

pub fn build_rom(
    project_root: &Utf8Path,
    output_override: Option<&Utf8Path>,
) -> Result<BuildOutcome> {
    let bundle = ProjectBundle::load(project_root)?;
    let build_dir = output_override
        .map(|path| path.to_owned())
        .unwrap_or_else(|| project_root.join(&bundle.manifest.build.output_dir));

    fs::create_dir_all(build_dir.join("generated"))?;

    let validation = validate_project(&bundle);
    let compiled_scenes = compile_scenes(&bundle)?;
    let stable_rom_path = build_dir.join(format!("{}.sfc", bundle.manifest.meta.slug));
    let linked_rom_path = build_dir.join(format!("{}.build.sfc", bundle.manifest.meta.slug));
    let report_path = build_dir.join("build-report.json");

    stage_runtime_files(&build_dir)?;
    write_generated_header(&build_dir, &bundle)?;
    write_generated_project_data(&build_dir, &bundle, &compiled_scenes)?;

    let assembler_status = maybe_assemble_rom(&bundle, &build_dir, &linked_rom_path)?;
    let rom_built =
        assembler_status.ca65_found && assembler_status.ld65_found && linked_rom_path.exists();
    let rom_path = if rom_built {
        finalize_rom_artifacts(
            &build_dir,
            &bundle.manifest.meta.slug,
            &linked_rom_path,
            &stable_rom_path,
        )?
    } else {
        stable_rom_path.clone()
    };
    let outcome = BuildOutcome {
        project_root: project_root.to_owned(),
        build_dir: build_dir.clone(),
        rom_path: rom_path.clone(),
        stable_rom_path: rom_built.then_some(stable_rom_path.clone()),
        report_path: report_path.clone(),
        rom_built,
        validation: validation.clone(),
        assembler_status,
        compiled_scenes: compiled_scenes
            .iter()
            .map(|scene| CompiledSceneSummary {
                scene_id: scene.scene_id.clone(),
                byte_len: scene.data_bytes.len(),
                metadata: scene.metadata.clone(),
            })
            .collect(),
    };

    fs::write(&report_path, serde_json::to_string_pretty(&outcome)?)?;

    if !validation.is_ok() {
        bail!(
            "validation failed with {} errors; see {}",
            validation.errors.len(),
            report_path
        );
    }

    Ok(outcome)
}

pub fn run_with_emulator(
    project_root: &Utf8Path,
    output_override: Option<&Utf8Path>,
    emulator_override: Option<&str>,
) -> Result<BuildOutcome> {
    let bundle = ProjectBundle::load(project_root)?;
    let outcome = build_rom(project_root, output_override)?;

    if !outcome.rom_built {
        bail!(
            "ROM was not built because ca65/ld65 are unavailable; generated report is at {}",
            outcome.report_path
        );
    }

    let emulator = emulator_override
        .map(str::to_owned)
        .or(bundle.manifest.editor.preferred_emulator.clone())
        .unwrap_or_else(|| "ares".to_string());

    let status = Command::new(&emulator)
        .arg(&outcome.rom_path)
        .status()
        .with_context(|| format!("failed to launch emulator '{}'", emulator))?;

    if !status.success() {
        bail!("emulator '{}' exited with status {}", emulator, status);
    }

    Ok(outcome)
}

fn compile_scenes(bundle: &ProjectBundle) -> Result<Vec<CompiledScene>> {
    let modules: Vec<Box<dyn GenreModule>> = vec![Box::new(PlatformerGenreModule::default())];
    let mut scenes = Vec::with_capacity(bundle.scenes.len());

    for scene in &bundle.scenes {
        let module = modules
            .iter()
            .find(|module| module.supports(scene))
            .ok_or_else(|| anyhow!("no genre module can compile scene '{}'", scene.id))?;
        scenes.push(module.scene_compiler().compile_scene(bundle, scene)?);
    }

    Ok(scenes)
}

fn stage_runtime_files(build_dir: &Utf8Path) -> Result<()> {
    let runtime_root = workspace_root().join("runtime/snes");
    fs::copy(runtime_root.join("main.s"), build_dir.join("main.s"))
        .context("failed to copy SNES runtime main.s")?;
    fs::copy(
        runtime_root.join("linker.cfg"),
        build_dir.join("linker.cfg"),
    )
    .context("failed to copy SNES runtime linker.cfg")?;
    Ok(())
}

fn write_generated_header(build_dir: &Utf8Path, bundle: &ProjectBundle) -> Result<()> {
    let mut title = bundle.manifest.meta.name.to_ascii_uppercase().into_bytes();
    title.truncate(21);
    while title.len() < 21 {
        title.push(b' ');
    }

    let rom_size_code = rom_size_code(bundle.manifest.build.rom_bank_count);
    let header_bytes = [
        title,
        vec![0x20],
        vec![0x00],
        vec![rom_size_code],
        vec![0x00],
        vec![0x01],
        vec![0x00],
        vec![0x00],
        vec![0x00, 0x00],
        vec![0x00, 0x00],
    ]
    .concat();

    fs::write(
        build_dir.join("generated/header.inc"),
        format_byte_directive("    .byte ", &header_bytes),
    )?;

    Ok(())
}

fn write_generated_project_data(
    build_dir: &Utf8Path,
    bundle: &ProjectBundle,
    compiled_scenes: &[CompiledScene],
) -> Result<()> {
    let bg_assets = build_bg_assets(bundle)?;
    let player_assets = build_player_assets(bundle)?;
    let mut text = String::new();
    text.push_str("PROJECT_TITLE_DATA:\n");
    text.push_str(&format_byte_directive(
        "    .byte ",
        bundle.manifest.meta.name.as_bytes(),
    ));
    text.push_str("    .byte $00\n");
    text.push_str(&format!(
        "PROJECT_SCENE_COUNT = {}\n",
        compiled_scenes.len()
    ));
    text.push_str(&format!(
        "; entry_scene = {}\n",
        bundle.manifest.gameplay.entry_scene
    ));
    text.push_str(&format!(
        "PROJECT_BG_PALETTE_BYTE_LEN = {}\n",
        bg_assets.palette_bytes.len()
    ));
    text.push_str("PROJECT_BG_PALETTE:\n");
    text.push_str(&format_byte_directive(
        "    .byte ",
        &bg_assets.palette_bytes,
    ));
    text.push_str(&format!(
        "PROJECT_BG_TILE_BYTE_LEN = {}\n",
        bg_assets.tile_bytes.len()
    ));
    text.push_str("PROJECT_BG_TILE_BYTES:\n");
    text.push_str(&format_byte_directive("    .byte ", &bg_assets.tile_bytes));
    text.push_str(&format!(
        "PROJECT_BG_MAP_WIDTH_TILES = {}\n",
        bg_assets.map_width_tiles
    ));
    text.push_str(&format!(
        "PROJECT_BG_MAP_HEIGHT_TILES = {}\n",
        bg_assets.map_height_tiles
    ));
    text.push_str(&format!(
        "PROJECT_MAX_SCROLL_X = {}\n",
        bg_assets.max_scroll_x_pixels
    ));
    text.push_str(&format!(
        "PROJECT_BG_MAP_BYTE_LEN = {}\n",
        bg_assets.tilemap_bytes.len()
    ));
    text.push_str("PROJECT_BG_MAP_BYTES:\n");
    text.push_str(&format_byte_directive(
        "    .byte ",
        &bg_assets.tilemap_bytes,
    ));
    text.push_str(&format!(
        "PROJECT_OBJ_PALETTE_BYTE_LEN = {}\n",
        player_assets.obj_palette_bytes.len()
    ));
    text.push_str("PROJECT_OBJ_PALETTE:\n");
    text.push_str(&format_byte_directive(
        "    .byte ",
        &player_assets.obj_palette_bytes,
    ));
    text.push_str(&format!(
        "PROJECT_OBJ_TILE_BYTE_LEN = {}\n",
        player_assets.obj_tile_bytes.len()
    ));
    text.push_str("PROJECT_OBJ_TILE_BYTES:\n");
    text.push_str(&format_byte_directive(
        "    .byte ",
        &player_assets.obj_tile_bytes,
    ));
    text.push_str(&format!(
        "PROJECT_VISUAL_HEADER_BYTE_LEN = {}\n",
        player_assets.visual_header_bytes.len()
    ));
    text.push_str("PROJECT_VISUAL_HEADERS:\n");
    text.push_str(&format_byte_directive(
        "    .byte ",
        &player_assets.visual_header_bytes,
    ));
    text.push_str(&format!(
        "PROJECT_VISUAL_PIECE_BYTE_LEN = {}\n",
        player_assets.visual_piece_bytes.len()
    ));
    text.push_str("PROJECT_VISUAL_PIECES:\n");
    text.push_str(&format_byte_directive(
        "    .byte ",
        &player_assets.visual_piece_bytes,
    ));
    text.push_str(&format!(
        "PROJECT_PLAYER_START_X = {}\n",
        player_assets.start_x
    ));
    text.push_str(&format!(
        "PROJECT_PLAYER_START_Y = {}\n",
        player_assets.start_y
    ));
    text.push_str(&format!(
        "PROJECT_PLAYER_GROUND_Y = {}\n",
        player_assets.ground_y
    ));
    text.push_str(&format!(
        "PROJECT_WORLD_WIDTH_PIXELS = {}\n",
        player_assets.world_width_pixels
    ));
    text.push_str(&format!(
        "PROJECT_PLAYER_MAX_X = {}\n",
        player_assets.player_max_x
    ));
    text.push_str(&format!(
        "PROJECT_PLAYER_VISUAL = ${:02X}\n",
        player_assets.player_visual
    ));
    text.push_str(&format!(
        "PROJECT_PLAYER_ALT_VISUAL = ${:02X}\n",
        player_assets.player_alt_visual
    ));
    text.push_str(&format!(
        "PROJECT_BULLET_VISUAL = ${:02X}\n",
        player_assets.bullet_visual
    ));
    text.push_str(&format!(
        "PROJECT_HUD_PIP_FULL_VISUAL = ${:02X}\n",
        player_assets.hud_pip_full_visual
    ));
    text.push_str(&format!(
        "PROJECT_HUD_PIP_EMPTY_VISUAL = ${:02X}\n",
        player_assets.hud_pip_empty_visual
    ));
    text.push_str(&format!(
        "PROJECT_HUD_HEART_FULL_VISUAL = ${:02X}\n",
        player_assets.hud_heart_full_visual
    ));
    text.push_str(&format!(
        "PROJECT_HUD_HEART_EMPTY_VISUAL = ${:02X}\n",
        player_assets.hud_heart_empty_visual
    ));
    text.push_str(&format!(
        "PROJECT_HUD_CELL_FULL_VISUAL = ${:02X}\n",
        player_assets.hud_cell_full_visual
    ));
    text.push_str(&format!(
        "PROJECT_HUD_CELL_EMPTY_VISUAL = ${:02X}\n",
        player_assets.hud_cell_empty_visual
    ));
    text.push_str(&format!(
        "PROJECT_PLAYER_MAX_HEALTH = {}\n",
        player_assets.player_max_health
    ));
    text.push_str(&format!(
        "PROJECT_PLAYER_STARTING_HEALTH = {}\n",
        player_assets.player_starting_health
    ));
    text.push_str(&format!(
        "PROJECT_HEALTH_HUD_STYLE = {}\n",
        player_assets.hud_style
    ));
    text.push_str(&format!(
        "PROJECT_ENTITY_COUNT = {}\n",
        player_assets.entity_count
    ));
    text.push_str(&format!(
        "PROJECT_ENTITY_BYTE_LEN = {}\n",
        player_assets.entity_bytes.len()
    ));
    text.push_str("PROJECT_ENTITY_BYTES:\n");
    text.push_str(&format_byte_directive(
        "    .byte ",
        &player_assets.entity_bytes,
    ));

    for (index, scene) in compiled_scenes.iter().enumerate() {
        text.push_str(&format!("SCENE_{}_DATA:\n", index));
        if scene.data_bytes.is_empty() {
            text.push_str("    .byte $00\n");
        } else {
            text.push_str(&format_byte_directive("    .byte ", &scene.data_bytes));
        }
        text.push_str(&format!(
            "SCENE_{}_BYTE_LEN = {}\n",
            index,
            scene.data_bytes.len()
        ));
    }

    fs::write(build_dir.join("generated/project_data.inc"), text)?;
    Ok(())
}

fn maybe_assemble_rom(
    bundle: &ProjectBundle,
    build_dir: &Utf8Path,
    rom_path: &Utf8Path,
) -> Result<AssemblerStatus> {
    let ca65_path = bundle.manifest.build.assembler.ca65_path.as_str();
    let ld65_path = bundle.manifest.build.assembler.ld65_path.as_str();
    let ca65_found = command_exists(ca65_path);
    let ld65_found = command_exists(ld65_path);
    let mut warnings = Vec::new();

    if !ca65_found {
        warnings.push(format!(
            "assembler '{}' was not found on PATH; skipping object build",
            ca65_path
        ));
    }

    if !ld65_found {
        warnings.push(format!(
            "linker '{}' was not found on PATH; skipping ROM link",
            ld65_path
        ));
    }

    if !(ca65_found && ld65_found) {
        return Ok(AssemblerStatus {
            ca65_found,
            ld65_found,
            warnings,
        });
    }

    let object_name = "main.o";
    let map_name = "main.map";
    let rom_name = rom_path
        .file_name()
        .ok_or_else(|| anyhow!("missing ROM file name for {}", rom_path))?;

    let ca65_status = Command::new(ca65_path)
        .current_dir(build_dir)
        .arg("main.s")
        .arg("-o")
        .arg(object_name)
        .status()
        .context("failed to invoke ca65")?;

    if !ca65_status.success() {
        bail!("ca65 exited with status {}", ca65_status);
    }

    let ld65_status = Command::new(ld65_path)
        .current_dir(build_dir)
        .arg("-C")
        .arg("linker.cfg")
        .arg(object_name)
        .arg("-o")
        .arg(rom_name)
        .arg("-m")
        .arg(map_name)
        .status()
        .context("failed to invoke ld65")?;

    if !ld65_status.success() {
        bail!("ld65 exited with status {}", ld65_status);
    }

    patch_lorom_checksum(rom_path)?;

    Ok(AssemblerStatus {
        ca65_found,
        ld65_found,
        warnings,
    })
}

fn workspace_root() -> Utf8PathBuf {
    Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn command_exists(command: &str) -> bool {
    if command.contains(std::path::MAIN_SEPARATOR) {
        return Utf8Path::new(command).exists();
    }

    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };

    std::env::split_paths(&path).any(|entry| entry.join(command).exists())
}

fn rom_size_code(bank_count: u16) -> u8 {
    let bytes = (bank_count.max(1) as usize) * 32 * 1024;
    let mut size_code = 0_u8;
    let mut size = 1024_usize;
    while size < bytes {
        size <<= 1;
        size_code += 1;
    }
    size_code
}

fn format_byte_directive(prefix: &str, bytes: &[u8]) -> String {
    let mut text = String::new();

    for chunk in bytes.chunks(16) {
        text.push_str(prefix);
        let row = chunk
            .iter()
            .map(|byte| format!("${byte:02X}"))
            .collect::<Vec<_>>()
            .join(", ");
        text.push_str(&row);
        text.push('\n');
    }

    text
}

fn patch_lorom_checksum(rom_path: &Utf8Path) -> Result<u16> {
    let mut rom = fs::read(rom_path).with_context(|| format!("failed to read ROM {}", rom_path))?;
    if rom.len() < 0x8000 {
        bail!("ROM {} is too small to contain a LoROM header", rom_path);
    }

    let checksum_offset = 0x7FDE;
    let complement_offset = 0x7FDC;
    rom[complement_offset] = 0;
    rom[complement_offset + 1] = 0;
    rom[checksum_offset] = 0;
    rom[checksum_offset + 1] = 0;

    let checksum = rom
        .iter()
        .fold(0_u32, |sum, byte| sum + (*byte as u32))
        .wrapping_rem(0x1_0000) as u16;
    let complement = checksum ^ 0xFFFF;

    rom[complement_offset..complement_offset + 2].copy_from_slice(&complement.to_le_bytes());
    rom[checksum_offset..checksum_offset + 2].copy_from_slice(&checksum.to_le_bytes());

    fs::write(rom_path, rom).with_context(|| format!("failed to write ROM {}", rom_path))?;
    Ok(checksum)
}

fn finalize_rom_artifacts(
    build_dir: &Utf8Path,
    slug: &str,
    linked_rom_path: &Utf8Path,
    stable_rom_path: &Utf8Path,
) -> Result<Utf8PathBuf> {
    let checksum = read_lorom_checksum(linked_rom_path)?;
    let build_stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is set before the Unix epoch")?
        .as_millis();
    let versioned_rom_path = build_dir.join(format!("{slug}-{build_stamp}-{checksum:04X}.sfc"));

    purge_old_versioned_roms(build_dir, slug, stable_rom_path, &versioned_rom_path)?;

    if versioned_rom_path.exists() {
        fs::remove_file(&versioned_rom_path).with_context(|| {
            format!(
                "failed to remove stale versioned ROM {}",
                versioned_rom_path
            )
        })?;
    }
    if stable_rom_path.exists() {
        fs::remove_file(stable_rom_path)
            .with_context(|| format!("failed to remove stale ROM {}", stable_rom_path))?;
    }

    fs::rename(linked_rom_path, &versioned_rom_path).with_context(|| {
        format!(
            "failed to move linked ROM {} to {}",
            linked_rom_path, versioned_rom_path
        )
    })?;
    fs::copy(&versioned_rom_path, stable_rom_path).with_context(|| {
        format!(
            "failed to copy versioned ROM {} to {}",
            versioned_rom_path, stable_rom_path
        )
    })?;

    Ok(versioned_rom_path)
}

fn read_lorom_checksum(rom_path: &Utf8Path) -> Result<u16> {
    let rom = fs::read(rom_path).with_context(|| format!("failed to read ROM {}", rom_path))?;
    if rom.len() < 0x7FE0 {
        bail!("ROM {} is too small to contain a LoROM checksum", rom_path);
    }

    Ok(u16::from_le_bytes([rom[0x7FDE], rom[0x7FDF]]))
}

fn purge_old_versioned_roms(
    build_dir: &Utf8Path,
    slug: &str,
    stable_rom_path: &Utf8Path,
    keep_path: &Utf8Path,
) -> Result<()> {
    for entry in fs::read_dir(build_dir)
        .with_context(|| format!("failed to read build directory {}", build_dir))?
    {
        let entry = entry?;
        let path = entry.path();
        let Ok(path) = Utf8PathBuf::from_path_buf(path) else {
            continue;
        };
        if path == stable_rom_path || path == keep_path {
            continue;
        }
        let Some(file_name) = path.file_name() else {
            continue;
        };
        if !file_name.starts_with(slug) || !file_name.ends_with(".sfc") || !file_name.contains('-')
        {
            continue;
        }
        fs::remove_file(&path)
            .with_context(|| format!("failed to remove stale versioned ROM {}", path))?;
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct BgAssets {
    palette_bytes: Vec<u8>,
    tile_bytes: Vec<u8>,
    tilemap_bytes: Vec<u8>,
    map_width_tiles: usize,
    map_height_tiles: usize,
    max_scroll_x_pixels: usize,
}

#[derive(Debug, Clone)]
struct PlayerAssets {
    obj_palette_bytes: Vec<u8>,
    obj_tile_bytes: Vec<u8>,
    visual_header_bytes: Vec<u8>,
    visual_piece_bytes: Vec<u8>,
    player_visual: u8,
    player_alt_visual: u8,
    bullet_visual: u8,
    hud_pip_full_visual: u8,
    hud_pip_empty_visual: u8,
    hud_heart_full_visual: u8,
    hud_heart_empty_visual: u8,
    hud_cell_full_visual: u8,
    hud_cell_empty_visual: u8,
    hud_style: u8,
    player_max_health: u8,
    player_starting_health: u8,
    entity_bytes: Vec<u8>,
    entity_count: u8,
    start_x: usize,
    start_y: usize,
    ground_y: usize,
    world_width_pixels: usize,
    player_max_x: usize,
}

#[derive(Debug, Clone)]
struct RuntimeVisualBuild {
    pieces: Vec<RuntimeVisualPieceBuild>,
    width_pixels: u8,
}

#[derive(Debug, Clone)]
struct RuntimeVisualPieceBuild {
    tile: Tile8,
    x: i8,
    y: i8,
    attr: u8,
}

fn build_bg_assets(bundle: &ProjectBundle) -> Result<BgAssets> {
    let scene = bundle
        .scene(&bundle.manifest.gameplay.entry_scene)
        .ok_or_else(|| {
            anyhow!(
                "entry scene '{}' is missing",
                bundle.manifest.gameplay.entry_scene
            )
        })?;
    let layer = scene
        .layers
        .first()
        .ok_or_else(|| anyhow!("scene '{}' has no visible layer to export", scene.id))?;
    let tileset = bundle.tileset(&layer.tileset_id).ok_or_else(|| {
        anyhow!(
            "scene '{}' references missing tileset '{}'",
            scene.id,
            layer.tileset_id
        )
    })?;
    let palette = bundle.palette(&tileset.palette_id).ok_or_else(|| {
        anyhow!(
            "tileset '{}' references missing palette '{}'",
            tileset.id,
            tileset.palette_id
        )
    })?;

    let palette_bytes = encode_palette_bytes(palette);
    let tile_bytes = encode_4bpp_tiles(tileset)?;
    let tilemap = build_display_tilemap(scene, layer, tileset)?;
    let tilemap_bytes = tilemap
        .iter()
        .flat_map(|entry| entry.to_le_bytes())
        .collect::<Vec<_>>();
    let max_scroll_x_pixels =
        (DISPLAY_MAP_WIDTH_TILES.saturating_sub(VISIBLE_SCREEN_WIDTH_TILES) * 8) as usize;

    Ok(BgAssets {
        palette_bytes,
        tile_bytes,
        tilemap_bytes,
        map_width_tiles: DISPLAY_MAP_WIDTH_TILES,
        map_height_tiles: DISPLAY_MAP_HEIGHT_TILES,
        max_scroll_x_pixels,
    })
}

fn build_player_assets(bundle: &ProjectBundle) -> Result<PlayerAssets> {
    let scene = bundle
        .scene(&bundle.manifest.gameplay.entry_scene)
        .ok_or_else(|| {
            anyhow!(
                "entry scene '{}' is missing",
                bundle.manifest.gameplay.entry_scene
            )
        })?;
    if scene.entities.len() > 16 {
        bail!(
            "scene '{}' has {} entities but the current runtime supports at most 16",
            scene.id,
            scene.entities.len()
        );
    }
    let spawn = scene
        .spawns
        .first()
        .ok_or_else(|| anyhow!("scene '{}' has no spawn point", scene.id))?;
    let player_animation = find_animation(bundle, "player_idle")?;
    let frame = player_animation
        .frames
        .first()
        .ok_or_else(|| anyhow!("animation '{}' has no frames", player_animation.id))?;
    let alt_frame = player_animation.frames.get(1).unwrap_or(frame);
    let player_sprite = find_metasprite(bundle, &frame.metasprite_id)?;
    let alt_player_sprite = find_metasprite(bundle, &alt_frame.metasprite_id)?;
    let shot_sprite = find_metasprite(bundle, "player_shot")?;
    let player_palette = bundle.palette(&player_sprite.palette_id).ok_or_else(|| {
        anyhow!(
            "metasprite '{}' references missing palette '{}'",
            player_sprite.id,
            player_sprite.palette_id
        )
    })?;
    let runtime_palette_id = player_palette.id.as_str();
    let mut runtime_tiles = vec![Tile8 {
        pixels: vec![0; 64],
    }];
    let mut visual_header_bytes = Vec::new();
    let mut visual_piece_bytes = Vec::new();
    let mut append_visual = |visual: RuntimeVisualBuild| -> Result<u8> {
        let visual_index = (visual_header_bytes.len() / 3) as u8;
        let piece_start = (visual_piece_bytes.len() / 4) as u8;
        visual_header_bytes.push(piece_start);
        visual_header_bytes.push(visual.pieces.len() as u8);
        visual_header_bytes.push(visual.width_pixels.max(8));
        for piece in visual.pieces {
            let tile_index = runtime_tiles.len();
            if tile_index > u8::MAX as usize {
                bail!("runtime OBJ tile count exceeded 255 tiles");
            }
            runtime_tiles.push(piece.tile);
            visual_piece_bytes.push(tile_index as u8);
            visual_piece_bytes.push(piece.x as u8);
            visual_piece_bytes.push(piece.y as u8);
            visual_piece_bytes.push(piece.attr);
        }
        Ok(visual_index)
    };

    let player_visual = append_visual(build_visual_from_metasprite(
        bundle,
        player_sprite,
        runtime_palette_id,
    )?)?;
    let player_alt_visual = append_visual(build_visual_from_metasprite(
        bundle,
        alt_player_sprite,
        runtime_palette_id,
    )?)?;
    let bullet_visual = append_visual(build_visual_from_metasprite(
        bundle,
        shot_sprite,
        runtime_palette_id,
    )?)?;

    let mut entity_visuals = std::collections::BTreeMap::new();
    for entity in &scene.entities {
        if entity_visuals.contains_key(entity.archetype.as_str()) {
            continue;
        }
        let metasprite = resolve_archetype_metasprite(bundle, &entity.archetype)?;
        let visual_index = append_visual(build_visual_from_metasprite(
            bundle,
            metasprite,
            runtime_palette_id,
        )?)?;
        entity_visuals.insert(entity.archetype.clone(), visual_index);
    }

    let hud_pip_full_visual = append_visual(build_single_tile_visual(hud_pip_tile(true)))?;
    let hud_pip_empty_visual = append_visual(build_single_tile_visual(hud_pip_tile(false)))?;
    let hud_heart_full_visual = append_visual(build_single_tile_visual(hud_heart_tile(true)))?;
    let hud_heart_empty_visual = append_visual(build_single_tile_visual(hud_heart_tile(false)))?;
    let hud_cell_full_visual = append_visual(build_single_tile_visual(hud_cell_tile(true)))?;
    let hud_cell_empty_visual = append_visual(build_single_tile_visual(hud_cell_tile(false)))?;

    let obj_tileset = TilesetResource {
        id: "generated_runtime_obj_tiles".to_string(),
        palette_id: player_palette.id.clone(),
        name: "Generated Runtime OBJ Tiles".to_string(),
        tiles: runtime_tiles,
    };
    let obj_tile_bytes = encode_4bpp_tiles(&obj_tileset)?;

    let world_width_pixels = DISPLAY_MAP_WIDTH_TILES * 8;
    let scaled_x = scale_scene_x(scene, spawn.position.x);
    let scaled_y = scale_scene_y(scene, spawn.position.y);
    let ground_y = scaled_y.saturating_sub(16);
    let start_y = ground_y;
    let entity_index_by_id = scene
        .entities
        .iter()
        .enumerate()
        .map(|(index, entity)| (entity.id.as_str(), index as u8))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut entity_bytes = Vec::with_capacity(scene.entities.len() * ENTITY_RUNTIME_BYTES);
    for entity in &scene.entities {
        let visual_index = *entity_visuals
            .get(entity.archetype.as_str())
            .ok_or_else(|| anyhow!("missing runtime visual for entity '{}'", entity.id))?;
        let action = encode_entity_action(entity, &entity_index_by_id)?;
        let (movement_kind, movement_speed, patrol_min_x, patrol_max_x) =
            encode_entity_movement(scene, entity);
        let scaled_hitbox_x = scale_scene_delta_x(scene, entity.hitbox.x) as i8;
        let scaled_hitbox_y = scale_scene_delta_y(scene, entity.hitbox.y) as i8;
        let scaled_hitbox_w = scale_scene_width(scene, entity.hitbox.width).max(1) as u8;
        let scaled_hitbox_h = scale_scene_height(scene, entity.hitbox.height).max(1) as u8;
        let scaled_entity_x = scale_scene_x(scene, entity.position.x) as u16;
        let scaled_entity_y = scale_scene_y(scene, entity.position.y) as u16;
        let flags = (u8::from(entity.active)) | (u8::from(entity.one_shot) << 1);

        entity_bytes.push(match entity.kind {
            EntityKind::Prop => 0,
            EntityKind::Pickup => 1,
            EntityKind::Enemy => 2,
            EntityKind::Switch => 3,
            EntityKind::Solid => 4,
        });
        entity_bytes.push(flags);
        entity_bytes.push(visual_index);
        entity_bytes.push(match entity.facing {
            snesmaker_project::Facing::Right => 0,
            snesmaker_project::Facing::Left => 1,
        });
        entity_bytes.push(scaled_hitbox_x as u8);
        entity_bytes.push(scaled_hitbox_y as u8);
        entity_bytes.push(scaled_hitbox_w);
        entity_bytes.push(scaled_hitbox_h);
        entity_bytes.push(entity.combat.contact_damage);
        entity_bytes.push(entity.combat.max_health);
        entity_bytes.push(action.0);
        entity_bytes.push(action.1);
        entity_bytes.push(action.2);
        entity_bytes.push(movement_kind);
        entity_bytes.push(movement_speed);
        entity_bytes.push(0);
        entity_bytes.extend(scaled_entity_x.to_le_bytes());
        entity_bytes.extend(scaled_entity_y.to_le_bytes());
        entity_bytes.extend(patrol_min_x.to_le_bytes());
        entity_bytes.extend(patrol_max_x.to_le_bytes());
    }
    let player_settings = &bundle.manifest.gameplay.player;

    Ok(PlayerAssets {
        obj_palette_bytes: encode_obj_palette_bytes(player_palette),
        obj_tile_bytes,
        visual_header_bytes,
        visual_piece_bytes,
        player_visual,
        player_alt_visual,
        bullet_visual,
        hud_pip_full_visual,
        hud_pip_empty_visual,
        hud_heart_full_visual,
        hud_heart_empty_visual,
        hud_cell_full_visual,
        hud_cell_empty_visual,
        hud_style: match player_settings.health_hud {
            HealthHudStyle::MegaPipsTopLeft => 0,
            HealthHudStyle::HeartsTopRight => 1,
            HealthHudStyle::CellsTopCenter => 2,
        },
        player_max_health: player_settings.max_health,
        player_starting_health: player_settings
            .starting_health
            .min(player_settings.max_health),
        entity_bytes,
        entity_count: scene.entities.len() as u8,
        start_x: scaled_x,
        start_y,
        ground_y,
        world_width_pixels,
        player_max_x: world_width_pixels.saturating_sub(16),
    })
}

fn encode_palette_bytes(palette: &PaletteResource) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(32);

    for index in 0..16 {
        let color = palette
            .colors
            .get(index)
            .copied()
            .unwrap_or(snesmaker_project::RgbaColor {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            });
        let value = snes_color(color.r, color.g, color.b);
        bytes.extend(value.to_le_bytes());
    }

    bytes
}

fn encode_obj_palette_bytes(palette: &PaletteResource) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(32);

    for index in 0..16 {
        let value = if index == 0 {
            0
        } else {
            let color =
                palette
                    .colors
                    .get(index)
                    .copied()
                    .unwrap_or(snesmaker_project::RgbaColor {
                        r: 0,
                        g: 0,
                        b: 0,
                        a: 255,
                    });
            snes_color(color.r, color.g, color.b)
        };
        bytes.extend(value.to_le_bytes());
    }

    bytes
}

fn encode_4bpp_tiles(tileset: &TilesetResource) -> Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(tileset.tiles.len() * 32);

    for (tile_index, tile) in tileset.tiles.iter().enumerate() {
        if tile.pixels.len() != 64 {
            bail!(
                "tileset '{}' tile {} must contain 64 pixels to export",
                tileset.id,
                tile_index
            );
        }

        let mut low_planes = [0_u8; 16];
        let mut high_planes = [0_u8; 16];

        for row in 0..8 {
            let mut plane0 = 0_u8;
            let mut plane1 = 0_u8;
            let mut plane2 = 0_u8;
            let mut plane3 = 0_u8;

            for col in 0..8 {
                let pixel = tile.pixels[row * 8 + col];
                if pixel > 0x0F {
                    bail!(
                        "tileset '{}' tile {} pixel {} exceeds 4bpp range",
                        tileset.id,
                        tile_index,
                        row * 8 + col
                    );
                }

                let mask = 1 << (7 - col);
                if pixel & 0x01 != 0 {
                    plane0 |= mask;
                }
                if pixel & 0x02 != 0 {
                    plane1 |= mask;
                }
                if pixel & 0x04 != 0 {
                    plane2 |= mask;
                }
                if pixel & 0x08 != 0 {
                    plane3 |= mask;
                }
            }

            low_planes[row * 2] = plane0;
            low_planes[row * 2 + 1] = plane1;
            high_planes[row * 2] = plane2;
            high_planes[row * 2 + 1] = plane3;
        }

        bytes.extend(low_planes);
        bytes.extend(high_planes);
    }

    Ok(bytes)
}

fn build_visual_from_metasprite(
    bundle: &ProjectBundle,
    metasprite: &MetaspriteResource,
    expected_palette_id: &str,
) -> Result<RuntimeVisualBuild> {
    if metasprite.palette_id != expected_palette_id {
        bail!(
            "metasprite '{}' uses palette '{}' but the runtime expects palette '{}'",
            metasprite.id,
            metasprite.palette_id,
            expected_palette_id
        );
    }

    let tileset = find_tileset_for_metasprite(bundle, metasprite)?;
    let width_pixels = metasprite
        .pieces
        .iter()
        .map(|piece| (piece.x + 8).max(8))
        .max()
        .unwrap_or(8)
        .clamp(8, 64) as u8;
    let mut pieces = Vec::with_capacity(metasprite.pieces.len());
    for piece in &metasprite.pieces {
        let tile = tileset
            .tiles
            .get(piece.tile_index as usize)
            .cloned()
            .ok_or_else(|| {
                anyhow!(
                    "tileset '{}' is missing tile {} for metasprite '{}'",
                    tileset.id,
                    piece.tile_index,
                    metasprite.id
                )
            })?;
        let mut attr = 0x30;
        if piece.h_flip {
            attr |= 0x40;
        }
        if piece.v_flip {
            attr |= 0x80;
        }
        pieces.push(RuntimeVisualPieceBuild {
            tile,
            x: piece.x as i8,
            y: piece.y as i8,
            attr,
        });
    }

    Ok(RuntimeVisualBuild {
        pieces,
        width_pixels,
    })
}

fn build_single_tile_visual(tile: Tile8) -> RuntimeVisualBuild {
    RuntimeVisualBuild {
        pieces: vec![RuntimeVisualPieceBuild {
            tile,
            x: 0,
            y: 0,
            attr: 0x30,
        }],
        width_pixels: 8,
    }
}

fn build_display_tilemap(
    scene: &SceneResource,
    layer: &TileLayer,
    tileset: &TilesetResource,
) -> Result<Vec<u16>> {
    let src_width = scene.size_tiles.width as usize;
    let src_height = scene.size_tiles.height as usize;
    if src_width == 0 || src_height == 0 {
        bail!("scene '{}' has an invalid zero-sized map", scene.id);
    }

    let mut entries = vec![0_u16; DISPLAY_MAP_WIDTH_TILES * DISPLAY_MAP_HEIGHT_TILES];
    for y in 0..DISPLAY_MAP_HEIGHT_TILES {
        let src_y = (y * src_height) / DISPLAY_MAP_HEIGHT_TILES;
        for x in 0..DISPLAY_MAP_WIDTH_TILES {
            let src_x = (x * src_width) / DISPLAY_MAP_WIDTH_TILES;
            let source_index = src_y * src_width + src_x;
            let tile_index = layer.tiles.get(source_index).copied().ok_or_else(|| {
                anyhow!(
                    "scene '{}' tilemap is missing index {}",
                    scene.id,
                    source_index
                )
            })?;
            if tile_index as usize >= tileset.tiles.len() {
                bail!(
                    "scene '{}' uses tile {} but tileset '{}' only has {} tiles",
                    scene.id,
                    tile_index,
                    tileset.id,
                    tileset.tiles.len()
                );
            }
            let screen_index = if x < 32 {
                y * 32 + x
            } else {
                32 * 32 + y * 32 + (x - 32)
            };
            entries[screen_index] = tile_index;
        }
    }

    Ok(entries)
}

fn scale_scene_x(scene: &SceneResource, x: i16) -> usize {
    let source_width_pixels = (scene.size_tiles.width as usize).saturating_mul(8).max(1);
    ((x.max(0) as usize) * DISPLAY_MAP_WIDTH_TILES * 8) / source_width_pixels
}

fn scale_scene_y(scene: &SceneResource, y: i16) -> usize {
    let source_height_pixels = (scene.size_tiles.height as usize).saturating_mul(8).max(1);
    ((y.max(0) as usize) * DISPLAY_MAP_HEIGHT_TILES * 8) / source_height_pixels
}

fn scale_scene_delta_x(scene: &SceneResource, x: i16) -> i16 {
    let source_width_pixels = (scene.size_tiles.width as i32).saturating_mul(8).max(1);
    (((x as i32) * (DISPLAY_MAP_WIDTH_TILES as i32) * 8) / source_width_pixels) as i16
}

fn scale_scene_delta_y(scene: &SceneResource, y: i16) -> i16 {
    let source_height_pixels = (scene.size_tiles.height as i32).saturating_mul(8).max(1);
    (((y as i32) * (DISPLAY_MAP_HEIGHT_TILES as i32) * 8) / source_height_pixels) as i16
}

fn scale_scene_width(scene: &SceneResource, width: u16) -> u16 {
    let source_width_pixels = (scene.size_tiles.width as usize).saturating_mul(8).max(1);
    (((width as usize) * DISPLAY_MAP_WIDTH_TILES * 8) / source_width_pixels).max(1) as u16
}

fn scale_scene_height(scene: &SceneResource, height: u16) -> u16 {
    let source_height_pixels = (scene.size_tiles.height as usize).saturating_mul(8).max(1);
    (((height as usize) * DISPLAY_MAP_HEIGHT_TILES * 8) / source_height_pixels).max(1) as u16
}

fn find_animation<'a>(bundle: &'a ProjectBundle, id: &str) -> Result<&'a AnimationResource> {
    bundle
        .animations
        .iter()
        .find(|animation| animation.id == id)
        .ok_or_else(|| anyhow!("animation '{}' is missing", id))
}

fn find_metasprite<'a>(bundle: &'a ProjectBundle, id: &str) -> Result<&'a MetaspriteResource> {
    bundle
        .metasprites
        .iter()
        .find(|metasprite| metasprite.id == id)
        .ok_or_else(|| anyhow!("metasprite '{}' is missing", id))
}

fn resolve_archetype_metasprite<'a>(
    bundle: &'a ProjectBundle,
    archetype: &str,
) -> Result<&'a MetaspriteResource> {
    if let Some(animation) = bundle.animation(archetype) {
        let frame = animation
            .frames
            .first()
            .ok_or_else(|| anyhow!("animation '{}' has no frames", animation.id))?;
        return find_metasprite(bundle, &frame.metasprite_id);
    }

    find_metasprite(bundle, archetype)
}

fn find_tileset_for_metasprite<'a>(
    bundle: &'a ProjectBundle,
    metasprite: &MetaspriteResource,
) -> Result<&'a TilesetResource> {
    let max_tile_index = metasprite
        .pieces
        .iter()
        .map(|piece| piece.tile_index)
        .max()
        .unwrap_or(0) as usize;

    bundle
        .tilesets
        .iter()
        .find(|tileset| {
            tileset.palette_id == metasprite.palette_id && tileset.tiles.len() > max_tile_index
        })
        .ok_or_else(|| {
            anyhow!(
                "no tileset with palette '{}' contains metasprite '{}' up to tile {}",
                metasprite.palette_id,
                metasprite.id,
                max_tile_index
            )
        })
}

fn encode_entity_action(
    entity: &snesmaker_project::EntityPlacement,
    entity_index_by_id: &std::collections::BTreeMap<&str, u8>,
) -> Result<(u8, u8, u8)> {
    match &entity.action {
        EntityAction::None => Ok((ACTION_NONE, 0, u8::MAX)),
        EntityAction::HealPlayer { amount } => Ok((ACTION_HEAL_PLAYER, *amount, u8::MAX)),
        EntityAction::SetEntityActive {
            target_entity_id,
            active,
        } => Ok((
            ACTION_SET_ENTITY_ACTIVE,
            u8::from(*active),
            *entity_index_by_id
                .get(target_entity_id.as_str())
                .ok_or_else(|| {
                    anyhow!(
                        "entity '{}' references unknown target entity '{}'",
                        entity.id,
                        target_entity_id
                    )
                })?,
        )),
    }
}

fn encode_entity_movement(
    scene: &SceneResource,
    entity: &snesmaker_project::EntityPlacement,
) -> (u8, u8, u16, u16) {
    match entity.movement {
        MovementPattern::None => {
            let x = scale_scene_x(scene, entity.position.x) as u16;
            (MOVEMENT_NONE, 0, x, x)
        }
        MovementPattern::Patrol {
            left_offset,
            right_offset,
            speed,
        } => {
            let base_x = scale_scene_x(scene, entity.position.x) as i32;
            let left = base_x + scale_scene_delta_x(scene, left_offset) as i32;
            let right = base_x + scale_scene_delta_x(scene, right_offset) as i32;
            let min_x = left.min(right).max(0) as u16;
            let max_x = left.max(right).max(0) as u16;
            (MOVEMENT_PATROL, speed.max(1), min_x, max_x)
        }
    }
}

fn hud_pip_tile(filled: bool) -> Tile8 {
    tile_from_rows(if filled {
        &[
            [0, 0, 0, 0, 0, 0, 0, 0],
            [0, 6, 6, 6, 6, 6, 6, 0],
            [0, 6, 2, 2, 2, 2, 6, 0],
            [0, 6, 2, 6, 6, 2, 6, 0],
            [0, 6, 2, 6, 6, 2, 6, 0],
            [0, 6, 2, 2, 2, 2, 6, 0],
            [0, 6, 6, 6, 6, 6, 6, 0],
            [0, 0, 0, 0, 0, 0, 0, 0],
        ]
    } else {
        &[
            [0, 0, 0, 0, 0, 0, 0, 0],
            [0, 3, 3, 3, 3, 3, 3, 0],
            [0, 3, 0, 0, 0, 0, 3, 0],
            [0, 3, 0, 0, 0, 0, 3, 0],
            [0, 3, 0, 0, 0, 0, 3, 0],
            [0, 3, 0, 0, 0, 0, 3, 0],
            [0, 3, 3, 3, 3, 3, 3, 0],
            [0, 0, 0, 0, 0, 0, 0, 0],
        ]
    })
}

fn hud_heart_tile(filled: bool) -> Tile8 {
    tile_from_rows(if filled {
        &[
            [0, 7, 7, 0, 0, 7, 7, 0],
            [7, 7, 7, 7, 7, 7, 7, 7],
            [7, 2, 7, 7, 7, 7, 2, 7],
            [7, 7, 7, 7, 7, 7, 7, 7],
            [0, 7, 7, 7, 7, 7, 7, 0],
            [0, 0, 7, 7, 7, 7, 0, 0],
            [0, 0, 0, 7, 7, 0, 0, 0],
            [0, 0, 0, 0, 0, 0, 0, 0],
        ]
    } else {
        &[
            [0, 3, 3, 0, 0, 3, 3, 0],
            [3, 0, 0, 3, 3, 0, 0, 3],
            [3, 0, 0, 0, 0, 0, 0, 3],
            [0, 3, 0, 0, 0, 0, 3, 0],
            [0, 0, 3, 0, 0, 3, 0, 0],
            [0, 0, 0, 3, 3, 0, 0, 0],
            [0, 0, 0, 0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0, 0, 0, 0],
        ]
    })
}

fn hud_cell_tile(filled: bool) -> Tile8 {
    tile_from_rows(if filled {
        &[
            [0, 0, 1, 1, 1, 1, 0, 0],
            [0, 1, 2, 2, 2, 2, 1, 0],
            [1, 2, 2, 2, 2, 2, 2, 1],
            [1, 2, 1, 1, 1, 1, 2, 1],
            [1, 2, 1, 1, 1, 1, 2, 1],
            [1, 2, 2, 2, 2, 2, 2, 1],
            [0, 1, 2, 2, 2, 2, 1, 0],
            [0, 0, 1, 1, 1, 1, 0, 0],
        ]
    } else {
        &[
            [0, 0, 3, 3, 3, 3, 0, 0],
            [0, 3, 0, 0, 0, 0, 3, 0],
            [3, 0, 0, 0, 0, 0, 0, 3],
            [3, 0, 0, 0, 0, 0, 0, 3],
            [3, 0, 0, 0, 0, 0, 0, 3],
            [3, 0, 0, 0, 0, 0, 0, 3],
            [0, 3, 0, 0, 0, 0, 3, 0],
            [0, 0, 3, 3, 3, 3, 0, 0],
        ]
    })
}

fn tile_from_rows(rows: &[[u8; 8]; 8]) -> Tile8 {
    Tile8 {
        pixels: rows.iter().flat_map(|row| row.iter().copied()).collect(),
    }
}

fn snes_color(r: u8, g: u8, b: u8) -> u16 {
    let r = ((r as u16) * 31 + 127) / 255;
    let g = ((g as u16) * 31 + 127) / 255;
    let b = ((b as u16) * 31 + 127) / 255;
    r | (g << 5) | (b << 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_snes_palette_entries() {
        let color = snes_color(255, 0, 0);
        assert_eq!(color, 0x001F);
    }

    #[test]
    fn encodes_4bpp_tiles() {
        let tileset = TilesetResource {
            id: "test".to_string(),
            palette_id: "p".to_string(),
            name: "Test".to_string(),
            tiles: vec![snesmaker_project::Tile8 {
                pixels: vec![
                    1, 1, 1, 1, 0, 0, 0, 0, 2, 2, 2, 2, 0, 0, 0, 0, 4, 4, 4, 4, 0, 0, 0, 0, 8, 8,
                    8, 8, 0, 0, 0, 0, 1, 2, 4, 8, 0, 0, 0, 0, 8, 4, 2, 1, 0, 0, 0, 0, 15, 15, 15,
                    15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                ],
            }],
        };

        let bytes = encode_4bpp_tiles(&tileset).expect("tile encoding");
        assert_eq!(bytes.len(), 32);
        assert_eq!(bytes[0], 0xF0);
        assert_eq!(bytes[1], 0x00);
        assert_eq!(bytes[2], 0x00);
        assert_eq!(bytes[3], 0xF0);
    }

    #[test]
    fn patches_lorom_checksum_fields() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rom_path =
            camino::Utf8PathBuf::from_path_buf(dir.path().join("test.sfc")).expect("utf8");
        let mut rom = vec![0_u8; 0x8000];
        rom[0] = 1;
        rom[1] = 2;
        std::fs::write(&rom_path, rom).expect("write rom");

        let checksum = patch_lorom_checksum(&rom_path).expect("patch checksum");
        let patched = std::fs::read(&rom_path).expect("read rom");
        let complement = u16::from_le_bytes([patched[0x7FDC], patched[0x7FDD]]);
        let stored_checksum = u16::from_le_bytes([patched[0x7FDE], patched[0x7FDF]]);
        assert_eq!(checksum, stored_checksum);
        assert_eq!(stored_checksum ^ complement, 0xFFFF);
    }

    #[test]
    fn finalizes_rom_into_versioned_and_stable_artifacts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let build_dir =
            camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 build dir");
        let linked_rom_path = build_dir.join("demo.build.sfc");
        let stable_rom_path = build_dir.join("demo.sfc");
        let mut rom = vec![0_u8; 0x8000];
        rom[0] = 7;
        rom[1] = 9;
        std::fs::write(&linked_rom_path, rom).expect("write linked rom");
        patch_lorom_checksum(&linked_rom_path).expect("patch checksum");

        let versioned_rom_path =
            finalize_rom_artifacts(&build_dir, "demo", &linked_rom_path, &stable_rom_path)
                .expect("finalize rom");

        assert!(versioned_rom_path.exists());
        assert!(stable_rom_path.exists());
        assert_ne!(versioned_rom_path, stable_rom_path);
        assert!(
            versioned_rom_path
                .file_name()
                .expect("versioned file name")
                .starts_with("demo-")
        );
        assert!(!linked_rom_path.exists());
        assert_eq!(
            std::fs::read(&versioned_rom_path).expect("read versioned rom"),
            std::fs::read(&stable_rom_path).expect("read stable rom")
        );
    }
}
