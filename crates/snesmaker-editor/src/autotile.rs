use super::*;
use snesmaker_project::{AdjacencyRuleSet, AdjacencySource};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HoveredAdjacencyDebug {
    pub rule_name: String,
    pub source: AdjacencySource,
    pub mask: u8,
    pub tile_index: u16,
}

pub(super) fn rule_source_label(source: AdjacencySource) -> &'static str {
    match source {
        AdjacencySource::Terrain => "terrain",
        AdjacencySource::Ladder => "ladder",
        AdjacencySource::Hazard => "hazard",
    }
}

pub(super) fn rebuild_scene_adjacency(
    scene: &mut SceneResource,
    layer_index: usize,
    tileset: &TilesetResource,
    region: TileSelectionRect,
) -> usize {
    let Some(layer) = scene.layers.get(layer_index) else {
        return 0;
    };
    if layer.tiles.is_empty() || tileset.adjacency_rules.is_empty() {
        return 0;
    }

    let width = scene.size_tiles.width as usize;
    let height = scene.size_tiles.height as usize;
    if width == 0 || height == 0 {
        return 0;
    }

    let min_x = region.min_x.saturating_sub(1).min(width.saturating_sub(1));
    let min_y = region.min_y.saturating_sub(1).min(height.saturating_sub(1));
    let max_x = region.max_x.saturating_add(1).min(width.saturating_sub(1));
    let max_y = region.max_y.saturating_add(1).min(height.saturating_sub(1));

    let current_tiles = layer.tiles.clone();
    let mut next_tiles = current_tiles.clone();
    let prepared_rules = tileset
        .adjacency_rules
        .iter()
        .map(|rule| PreparedRule {
            rule,
            member_tiles: rule.mask_tiles.values().copied().collect(),
        })
        .collect::<Vec<_>>();
    let mut changed = 0;

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let index = y * width + x;
            let current_tile = current_tiles[index];
            for prepared in &prepared_rules {
                let participates = source_membership(
                    prepared.rule.source,
                    scene,
                    &current_tiles,
                    &prepared.member_tiles,
                    x,
                    y,
                );
                if participates {
                    let mask = neighbor_mask(scene, &current_tiles, prepared, x, y);
                    let chosen_tile = prepared
                        .rule
                        .mask_tiles
                        .get(&mask)
                        .copied()
                        .or_else(|| prepared.rule.mask_tiles.values().next().copied())
                        .unwrap_or(current_tile);
                    if next_tiles[index] != chosen_tile {
                        next_tiles[index] = chosen_tile;
                        changed += 1;
                    }
                } else if !matches!(prepared.rule.source, AdjacencySource::Terrain)
                    && prepared.member_tiles.contains(&current_tile)
                    && next_tiles[index] != 0
                {
                    next_tiles[index] = 0;
                    changed += 1;
                }
            }
        }
    }

    if changed > 0 {
        if let Some(layer) = scene.layers.get_mut(layer_index) {
            layer.tiles = next_tiles;
        }
    }

    changed
}

pub(super) fn hovered_adjacency_debug(
    scene: &SceneResource,
    layer_index: usize,
    tileset: &TilesetResource,
    hovered_tile: (usize, usize),
) -> Vec<HoveredAdjacencyDebug> {
    let Some(layer) = scene.layers.get(layer_index) else {
        return Vec::new();
    };
    if hovered_tile.0 >= scene.size_tiles.width as usize
        || hovered_tile.1 >= scene.size_tiles.height as usize
    {
        return Vec::new();
    }

    let tiles = &layer.tiles;
    let prepared_rules = tileset
        .adjacency_rules
        .iter()
        .map(|rule| PreparedRule {
            rule,
            member_tiles: rule.mask_tiles.values().copied().collect(),
        })
        .collect::<Vec<_>>();
    let mut debug = Vec::new();
    let index = hovered_tile.1 * scene.size_tiles.width as usize + hovered_tile.0;
    let current_tile = tiles.get(index).copied().unwrap_or_default();

    for prepared in &prepared_rules {
        let participates = source_membership(
            prepared.rule.source,
            scene,
            tiles,
            &prepared.member_tiles,
            hovered_tile.0,
            hovered_tile.1,
        );
        if !participates
            && !(matches!(prepared.rule.source, AdjacencySource::Terrain)
                && prepared.member_tiles.contains(&current_tile))
        {
            continue;
        }

        let mask = neighbor_mask(scene, tiles, prepared, hovered_tile.0, hovered_tile.1);
        let tile_index = prepared
            .rule
            .mask_tiles
            .get(&mask)
            .copied()
            .or_else(|| prepared.rule.mask_tiles.values().next().copied())
            .unwrap_or(current_tile);
        debug.push(HoveredAdjacencyDebug {
            rule_name: if prepared.rule.name.trim().is_empty() {
                prepared.rule.id.clone()
            } else {
                prepared.rule.name.clone()
            },
            source: prepared.rule.source,
            mask,
            tile_index,
        });
    }

    debug
}

struct PreparedRule<'a> {
    rule: &'a AdjacencyRuleSet,
    member_tiles: BTreeSet<u16>,
}

fn neighbor_mask(
    scene: &SceneResource,
    tiles: &[u16],
    prepared: &PreparedRule<'_>,
    x: usize,
    y: usize,
) -> u8 {
    let mut mask = 0_u8;
    if y > 0
        && source_membership(
            prepared.rule.source,
            scene,
            tiles,
            &prepared.member_tiles,
            x,
            y - 1,
        )
    {
        mask |= 0b0001;
    }
    if x + 1 < scene.size_tiles.width as usize
        && source_membership(
            prepared.rule.source,
            scene,
            tiles,
            &prepared.member_tiles,
            x + 1,
            y,
        )
    {
        mask |= 0b0010;
    }
    if y + 1 < scene.size_tiles.height as usize
        && source_membership(
            prepared.rule.source,
            scene,
            tiles,
            &prepared.member_tiles,
            x,
            y + 1,
        )
    {
        mask |= 0b0100;
    }
    if x > 0
        && source_membership(
            prepared.rule.source,
            scene,
            tiles,
            &prepared.member_tiles,
            x - 1,
            y,
        )
    {
        mask |= 0b1000;
    }
    mask
}

fn source_membership(
    source: AdjacencySource,
    scene: &SceneResource,
    tiles: &[u16],
    member_tiles: &BTreeSet<u16>,
    x: usize,
    y: usize,
) -> bool {
    let index = y * scene.size_tiles.width as usize + x;
    match source {
        AdjacencySource::Terrain => member_tiles.contains(&tiles.get(index).copied().unwrap_or(0)),
        AdjacencySource::Ladder => scene.collision.ladders.get(index).copied().unwrap_or(false),
        AdjacencySource::Hazard => scene.collision.hazards.get(index).copied().unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use snesmaker_project::{
        AdjacencyRuleSet, AdjacencySource, CollisionLayer, GridSize, TileLayer,
    };

    #[test]
    fn rebuilds_terrain_neighbors_from_mask_rules() {
        let tileset = TilesetResource {
            id: "tiles".to_string(),
            palette_id: "palette".to_string(),
            name: "Tiles".to_string(),
            adjacency_rules: vec![AdjacencyRuleSet {
                id: "terrain".to_string(),
                name: "Terrain".to_string(),
                source: AdjacencySource::Terrain,
                mask_tiles: [(0, 10_u16), (2, 11_u16), (15, 12_u16)]
                    .into_iter()
                    .collect(),
            }],
            tiles: vec![
                Tile8 {
                    pixels: vec![0; 64]
                };
                16
            ],
        };
        let mut scene = SceneResource {
            id: "scene".to_string(),
            kind: snesmaker_project::SceneKind::SideScroller,
            size_tiles: GridSize {
                width: 3,
                height: 3,
            },
            chunk_size_tiles: GridSize {
                width: 3,
                height: 3,
            },
            background_color_index: 0,
            layers: vec![TileLayer {
                id: "bg".to_string(),
                tileset_id: "tiles".to_string(),
                visible: true,
                parallax_x: 1,
                parallax_y: 1,
                tiles: vec![0, 10, 0, 10, 10, 10, 0, 10, 0],
            }],
            collision: CollisionLayer::default(),
            spawns: Vec::new(),
            checkpoints: Vec::new(),
            entities: Vec::new(),
            triggers: Vec::new(),
            scripts: Vec::new(),
            prefab_instances: Vec::new(),
        };

        let changed = rebuild_scene_adjacency(
            &mut scene,
            0,
            &tileset,
            TileSelectionRect::from_points((0, 0), (2, 2)),
        );

        assert_eq!(changed, 2);
        assert_eq!(scene.layers[0].tiles[3], 11);
        assert_eq!(scene.layers[0].tiles[4], 12);
    }

    #[test]
    fn clears_ladder_tiles_when_collision_is_removed() {
        let tileset = TilesetResource {
            id: "tiles".to_string(),
            palette_id: "palette".to_string(),
            name: "Tiles".to_string(),
            adjacency_rules: vec![AdjacencyRuleSet {
                id: "ladder".to_string(),
                name: "Ladder".to_string(),
                source: AdjacencySource::Ladder,
                mask_tiles: [(0, 3_u16), (4, 4_u16)].into_iter().collect(),
            }],
            tiles: vec![
                Tile8 {
                    pixels: vec![0; 64]
                };
                8
            ],
        };
        let mut scene = SceneResource {
            id: "scene".to_string(),
            kind: snesmaker_project::SceneKind::SideScroller,
            size_tiles: GridSize {
                width: 1,
                height: 2,
            },
            chunk_size_tiles: GridSize {
                width: 1,
                height: 2,
            },
            background_color_index: 0,
            layers: vec![TileLayer {
                id: "bg".to_string(),
                tileset_id: "tiles".to_string(),
                visible: true,
                parallax_x: 1,
                parallax_y: 1,
                tiles: vec![3, 4],
            }],
            collision: CollisionLayer {
                solids: vec![false; 2],
                ladders: vec![true, false],
                hazards: vec![false; 2],
            },
            spawns: Vec::new(),
            checkpoints: Vec::new(),
            entities: Vec::new(),
            triggers: Vec::new(),
            scripts: Vec::new(),
            prefab_instances: Vec::new(),
        };

        let changed = rebuild_scene_adjacency(
            &mut scene,
            0,
            &tileset,
            TileSelectionRect::from_points((0, 0), (0, 1)),
        );

        assert_eq!(changed, 1);
        assert_eq!(scene.layers[0].tiles, vec![3, 0]);
    }
}
