use std::collections::BTreeMap;

use anyhow::{Result, anyhow, bail};
use camino::Utf8Path;
use image::RgbaImage;
use snesmaker_project::{
    AnimationFrame, AnimationResource, MetaspriteResource, PaletteResource, ProjectBundle,
    RgbaColor, SpriteTileRef, Tile8, TilesetResource, slugify,
};

pub struct ImportedIndexedImage {
    pub width: u32,
    pub height: u32,
    pub palette: PaletteResource,
    pub tileset: TilesetResource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuggestedImportIds {
    pub base_id: String,
    pub animation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteSheetImportRequest {
    pub base_id: String,
    pub animation_id: String,
    pub frame_width_px: u32,
    pub frame_height_px: u32,
    pub frame_count: usize,
    pub columns: usize,
    pub frame_duration: u8,
    pub target_tileset_id: String,
    pub target_palette_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteSheetImportResult {
    pub tileset_id: String,
    pub animation_id: String,
    pub imported_tile_count: usize,
    pub metasprite_ids: Vec<String>,
}

pub fn import_png_as_tiles(
    path: &Utf8Path,
    palette_id: &str,
    tileset_id: &str,
    name: &str,
) -> Result<ImportedIndexedImage> {
    let image = image::open(path)?;
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();

    if width % 8 != 0 || height % 8 != 0 {
        bail!("PNG dimensions must be multiples of 8 pixels");
    }

    let mut palette_lookup = BTreeMap::new();
    let mut palette_colors = Vec::new();
    let mut indexed_pixels = Vec::with_capacity((width * height) as usize);

    for pixel in rgba.pixels() {
        let color = RgbaColor {
            r: pixel[0],
            g: pixel[1],
            b: pixel[2],
            a: pixel[3],
        };

        let palette_index = if let Some(index) = palette_lookup.get(&color_key(&color)) {
            *index
        } else {
            if palette_colors.len() >= 16 {
                return Err(anyhow!(
                    "PNG '{}' uses more than 16 colors; reduce it before import",
                    path
                ));
            }

            let index = palette_colors.len() as u8;
            palette_lookup.insert(color_key(&color), index);
            palette_colors.push(color);
            index
        };

        indexed_pixels.push(palette_index);
    }

    let mut tiles = Vec::new();
    let tiles_wide = width / 8;
    let tiles_high = height / 8;

    for tile_y in 0..tiles_high {
        for tile_x in 0..tiles_wide {
            let mut pixels = Vec::with_capacity(64);
            for y in 0..8 {
                for x in 0..8 {
                    let pixel_x = tile_x * 8 + x;
                    let pixel_y = tile_y * 8 + y;
                    let index = (pixel_y * width + pixel_x) as usize;
                    pixels.push(indexed_pixels[index]);
                }
            }
            tiles.push(Tile8 { pixels });
        }
    }

    Ok(ImportedIndexedImage {
        width,
        height,
        palette: PaletteResource {
            id: palette_id.to_string(),
            name: name.to_string(),
            colors: palette_colors,
        },
        tileset: TilesetResource {
            id: tileset_id.to_string(),
            palette_id: palette_id.to_string(),
            name: name.to_string(),
            adjacency_rules: Vec::new(),
            tiles,
        },
    })
}

pub fn suggest_sprite_sheet_ids(bundle: &ProjectBundle, source_label: &str) -> SuggestedImportIds {
    let stem = slugify(source_label);
    let stem = if stem.is_empty() {
        "imported_sprite".to_string()
    } else {
        stem
    };
    let mut reserved_ids = bundle
        .unique_ids()
        .into_iter()
        .map(str::to_string)
        .collect::<std::collections::BTreeSet<_>>();
    let base_id = next_unique_import_id(&reserved_ids, &stem);
    reserved_ids.insert(base_id.clone());
    let animation_id = next_unique_import_id(&reserved_ids, &format!("{stem}_anim"));

    SuggestedImportIds {
        base_id,
        animation_id,
    }
}

pub fn import_sprite_sheet_into_bundle(
    bundle: &mut ProjectBundle,
    request: &SpriteSheetImportRequest,
    rgba: &[u8],
    size: [usize; 2],
) -> Result<SpriteSheetImportResult> {
    if request.base_id.trim().is_empty() || request.animation_id.trim().is_empty() {
        bail!("Base id and animation id are required.");
    }
    if request.frame_width_px == 0
        || request.frame_height_px == 0
        || request.frame_width_px % 8 != 0
        || request.frame_height_px % 8 != 0
    {
        bail!("Frame width and height must be non-zero multiples of 8 pixels.");
    }
    if request.frame_count == 0 || request.columns == 0 {
        bail!("Frame count and columns must be greater than zero.");
    }

    let sheet = RgbaImage::from_raw(size[0] as u32, size[1] as u32, rgba.to_vec())
        .ok_or_else(|| anyhow!("failed to decode sprite sheet pixels"))?;

    let mut reserved_ids = bundle
        .unique_ids()
        .into_iter()
        .map(str::to_string)
        .collect::<std::collections::BTreeSet<_>>();
    if reserved_ids.contains(request.animation_id.as_str()) {
        bail!(
            "Animation id '{}' already exists. Choose a unique id before importing.",
            request.animation_id
        );
    }

    let tileset_index = bundle
        .tilesets
        .iter()
        .position(|tileset| tileset.id == request.target_tileset_id)
        .ok_or_else(|| anyhow!("target tileset '{}' is missing", request.target_tileset_id))?;
    let palette_index = bundle
        .palettes
        .iter()
        .position(|palette| palette.id == request.target_palette_id)
        .ok_or_else(|| anyhow!("target palette '{}' is missing", request.target_palette_id))?;

    reserved_ids.insert(request.animation_id.clone());
    let palette_id = bundle
        .palettes
        .get(palette_index)
        .ok_or_else(|| anyhow!("palette index out of range"))?
        .id
        .clone();

    let (new_metasprites, animation_frames, imported_tile_count) = {
        let palette = bundle
            .palettes
            .get_mut(palette_index)
            .ok_or_else(|| anyhow!("palette index out of range"))?;
        let tileset = bundle
            .tilesets
            .get_mut(tileset_index)
            .ok_or_else(|| anyhow!("tileset index out of range"))?;
        let starting_tile_count = tileset.tiles.len();

        let mut metasprites = Vec::with_capacity(request.frame_count);
        let mut frames = Vec::with_capacity(request.frame_count);
        for frame_index in 0..request.frame_count {
            let metasprite_id = if request.frame_count == 1 {
                request.base_id.clone()
            } else {
                format!("{}_{:02}", request.base_id, frame_index + 1)
            };
            if reserved_ids.contains(metasprite_id.as_str()) {
                bail!(
                    "Metasprite id '{}' already exists. Choose a different base id.",
                    metasprite_id
                );
            }
            reserved_ids.insert(metasprite_id.clone());

            let frame_x = (frame_index % request.columns) as u32 * request.frame_width_px;
            let frame_y = (frame_index / request.columns) as u32 * request.frame_height_px;
            if frame_x + request.frame_width_px > sheet.width()
                || frame_y + request.frame_height_px > sheet.height()
            {
                bail!(
                    "Frame {} exceeds the sprite sheet bounds. Check the frame size, count, or column count.",
                    frame_index + 1
                );
            }

            let frame_tiles_x = request.frame_width_px / 8;
            let frame_tiles_y = request.frame_height_px / 8;
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
                        priority: 3,
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
                duration_frames: request.frame_duration.max(1),
            });
        }

        (
            metasprites,
            frames,
            bundle.tilesets[tileset_index]
                .tiles
                .len()
                .saturating_sub(starting_tile_count),
        )
    };

    let metasprite_ids = new_metasprites
        .iter()
        .map(|metasprite| metasprite.id.clone())
        .collect::<Vec<_>>();
    bundle.metasprites.extend(new_metasprites);
    bundle.animations.push(AnimationResource {
        id: request.animation_id.clone(),
        frames: animation_frames,
    });

    Ok(SpriteSheetImportResult {
        tileset_id: request.target_tileset_id.clone(),
        animation_id: request.animation_id.clone(),
        imported_tile_count,
        metasprite_ids,
    })
}

fn next_unique_import_id(existing: &std::collections::BTreeSet<String>, base: &str) -> String {
    let stem = slugify(base);
    let stem = if stem.is_empty() {
        "imported_sprite".to_string()
    } else {
        stem
    };
    if !existing.contains(&stem) {
        return stem;
    }

    let mut suffix = 2;
    loop {
        let candidate = format!("{stem}_{suffix:02}");
        if !existing.contains(&candidate) {
            return candidate;
        }
        suffix += 1;
    }
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

fn color_key(color: &RgbaColor) -> u32 {
    ((color.r as u32) << 24) | ((color.g as u32) << 16) | ((color.b as u32) << 8) | color.a as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use snesmaker_project::demo_bundle;

    #[test]
    fn suggests_unique_import_ids_from_source_name() {
        let mut bundle = demo_bundle();
        bundle.animations.push(AnimationResource {
            id: "player_idle_anim".to_string(),
            frames: Vec::new(),
        });

        let ids = suggest_sprite_sheet_ids(&bundle, "Player Idle");
        assert_eq!(ids.base_id, "player_idle_02");
        assert_eq!(ids.animation_id, "player_idle_anim_02");
    }

    #[test]
    fn imports_sprite_sheet_and_rejects_duplicate_ids() {
        let mut bundle = demo_bundle();
        let request = SpriteSheetImportRequest {
            base_id: "imported_sprite".to_string(),
            animation_id: "imported_anim".to_string(),
            frame_width_px: 8,
            frame_height_px: 8,
            frame_count: 1,
            columns: 1,
            frame_duration: 6,
            target_tileset_id: bundle.tilesets[0].id.clone(),
            target_palette_id: bundle.palettes[0].id.clone(),
        };
        let mut rgba = vec![0_u8; 8 * 8 * 4];
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[255, 0, 0, 255]);
        }

        let result =
            import_sprite_sheet_into_bundle(&mut bundle, &request, &rgba, [8, 8]).expect("import");
        assert_eq!(result.animation_id, "imported_anim");
        assert_eq!(result.imported_tile_count, 1);
        assert_eq!(result.metasprite_ids, vec!["imported_sprite".to_string()]);

        let duplicate_error =
            import_sprite_sheet_into_bundle(&mut bundle, &request, &rgba, [8, 8]).unwrap_err();
        assert!(duplicate_error.to_string().contains("already exists"));
    }
}
