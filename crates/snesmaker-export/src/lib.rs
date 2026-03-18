use std::fs;
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use snesmaker_platformer::PlatformerGenreModule;
use snesmaker_project::{
    AnimationResource, CompiledScene, GenreModule, MetaspriteResource, PaletteResource,
    ProjectBundle, SceneResource, Tile8, TileLayer, TilesetResource,
};
use snesmaker_validator::{ValidationReport, validate_project};

const DISPLAY_MAP_WIDTH_TILES: usize = 64;
const DISPLAY_MAP_HEIGHT_TILES: usize = 32;
const VISIBLE_SCREEN_WIDTH_TILES: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildOutcome {
    pub project_root: Utf8PathBuf,
    pub build_dir: Utf8PathBuf,
    pub rom_path: Utf8PathBuf,
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
    let rom_path = build_dir.join(format!("{}.sfc", bundle.manifest.meta.slug));
    let report_path = build_dir.join("build-report.json");

    stage_runtime_files(&build_dir)?;
    write_generated_header(&build_dir, &bundle)?;
    write_generated_project_data(&build_dir, &bundle, &compiled_scenes)?;

    let assembler_status = maybe_assemble_rom(&bundle, &build_dir, &rom_path)?;
    let outcome = BuildOutcome {
        project_root: project_root.to_owned(),
        build_dir: build_dir.clone(),
        rom_path: rom_path.clone(),
        report_path: report_path.clone(),
        rom_built: assembler_status.ca65_found && assembler_status.ld65_found && rom_path.exists(),
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
        "PROJECT_PLAYER_BASE_TILE = ${:02X}\n",
        player_assets.player_base_tile
    ));
    text.push_str(&format!(
        "PROJECT_BULLET_TILE = ${:02X}\n",
        player_assets.bullet_tile
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

fn patch_lorom_checksum(rom_path: &Utf8Path) -> Result<()> {
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
    player_base_tile: u8,
    bullet_tile: u8,
    start_x: usize,
    start_y: usize,
    ground_y: usize,
    world_width_pixels: usize,
    player_max_x: usize,
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
    let spawn = scene
        .spawns
        .first()
        .ok_or_else(|| anyhow!("scene '{}' has no spawn point", scene.id))?;
    let player_animation = find_animation(bundle, "player_idle")?;
    let frame = player_animation
        .frames
        .first()
        .ok_or_else(|| anyhow!("animation '{}' has no frames", player_animation.id))?;
    let player_sprite = find_metasprite(bundle, &frame.metasprite_id)?;
    let shot_sprite = find_metasprite(bundle, "player_shot")?;
    let player_palette = bundle.palette(&player_sprite.palette_id).ok_or_else(|| {
        anyhow!(
            "metasprite '{}' references missing palette '{}'",
            player_sprite.id,
            player_sprite.palette_id
        )
    })?;
    let source_tileset = find_tileset_for_sprites(bundle, player_sprite, shot_sprite)?;

    let mut player_pieces = player_sprite.pieces.clone();
    player_pieces.sort_by_key(|piece| (piece.y, piece.x));
    if player_pieces.len() != 4 {
        bail!(
            "player metasprite '{}' must contain exactly 4 pieces for the demo runtime",
            player_sprite.id
        );
    }
    let expected_positions = [(0_i16, 0_i16), (8, 0), (0, 8), (8, 8)];
    for (piece, (expected_x, expected_y)) in player_pieces.iter().zip(expected_positions) {
        if piece.x != expected_x || piece.y != expected_y {
            bail!(
                "player metasprite '{}' must use a 2x2 8x8 layout at (0,0), (8,0), (0,8), (8,8)",
                player_sprite.id
            );
        }
        if piece.h_flip || piece.v_flip {
            bail!(
                "player metasprite '{}' pieces cannot use tile flips in the demo OBJ exporter",
                player_sprite.id
            );
        }
    }

    let bullet_piece = shot_sprite
        .pieces
        .first()
        .ok_or_else(|| anyhow!("metasprite '{}' has no pieces", shot_sprite.id))?;
    if bullet_piece.h_flip || bullet_piece.v_flip {
        bail!(
            "metasprite '{}' cannot use tile flips in the demo OBJ exporter",
            shot_sprite.id
        );
    }
    let obj_tile_bytes = encode_obj_tiles(source_tileset, &player_pieces, bullet_piece.tile_index)?;

    let world_width_pixels = DISPLAY_MAP_WIDTH_TILES * 8;
    let scaled_x = scale_scene_x(scene, spawn.position.x);
    let scaled_y = scale_scene_y(scene, spawn.position.y);
    let ground_y = scaled_y.saturating_sub(16);
    let start_y = ground_y;

    Ok(PlayerAssets {
        obj_palette_bytes: encode_obj_palette_bytes(player_palette),
        obj_tile_bytes,
        player_base_tile: 0,
        bullet_tile: 2,
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

fn encode_obj_tiles(
    source_tileset: &TilesetResource,
    player_pieces: &[snesmaker_project::SpriteTileRef],
    bullet_tile_index: u16,
) -> Result<Vec<u8>> {
    let blank_tile = Tile8 {
        pixels: vec![0; 64],
    };
    let mut obj_tiles = vec![blank_tile.clone(); 18];

    let tile_at = |index: u16| -> Result<Tile8> {
        source_tileset
            .tiles
            .get(index as usize)
            .cloned()
            .ok_or_else(|| {
                anyhow!(
                    "tileset '{}' is missing tile {} for OBJ export",
                    source_tileset.id,
                    index
                )
            })
    };

    obj_tiles[0] = tile_at(player_pieces[0].tile_index)?;
    obj_tiles[1] = tile_at(player_pieces[1].tile_index)?;
    obj_tiles[2] = tile_at(bullet_tile_index)?;
    obj_tiles[16] = tile_at(player_pieces[2].tile_index)?;
    obj_tiles[17] = tile_at(player_pieces[3].tile_index)?;

    let obj_tileset = TilesetResource {
        id: "generated_obj_tiles".to_string(),
        palette_id: source_tileset.palette_id.clone(),
        name: "Generated OBJ Tiles".to_string(),
        tiles: obj_tiles,
    };

    encode_4bpp_tiles(&obj_tileset)
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

fn find_tileset_for_sprites<'a>(
    bundle: &'a ProjectBundle,
    player_sprite: &MetaspriteResource,
    shot_sprite: &MetaspriteResource,
) -> Result<&'a TilesetResource> {
    if player_sprite.palette_id != shot_sprite.palette_id {
        bail!(
            "player metasprite '{}' and shot metasprite '{}' must share one palette for the demo runtime",
            player_sprite.id,
            shot_sprite.id
        );
    }

    let max_tile_index = player_sprite
        .pieces
        .iter()
        .chain(shot_sprite.pieces.iter())
        .map(|piece| piece.tile_index)
        .max()
        .unwrap_or(0) as usize;

    bundle
        .tilesets
        .iter()
        .find(|tileset| {
            tileset.palette_id == player_sprite.palette_id && tileset.tiles.len() > max_tile_index
        })
        .ok_or_else(|| {
            anyhow!(
                "no tileset with palette '{}' contains the player/shot tiles up to index {}",
                player_sprite.palette_id,
                max_tile_index
            )
        })
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

        patch_lorom_checksum(&rom_path).expect("patch checksum");
        let patched = std::fs::read(&rom_path).expect("read rom");
        let complement = u16::from_le_bytes([patched[0x7FDC], patched[0x7FDD]]);
        let checksum = u16::from_le_bytes([patched[0x7FDE], patched[0x7FDF]]);
        assert_eq!(checksum ^ complement, 0xFFFF);
    }
}
