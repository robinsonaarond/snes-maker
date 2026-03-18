use std::collections::BTreeMap;

use anyhow::{Result, anyhow, bail};
use camino::Utf8Path;
use snesmaker_project::{PaletteResource, RgbaColor, Tile8, TilesetResource};

pub struct ImportedIndexedImage {
    pub width: u32,
    pub height: u32,
    pub palette: PaletteResource,
    pub tileset: TilesetResource,
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
            tiles,
        },
    })
}

fn color_key(color: &RgbaColor) -> u32 {
    ((color.r as u32) << 24) | ((color.g as u32) << 16) | ((color.b as u32) << 8) | color.a as u32
}
