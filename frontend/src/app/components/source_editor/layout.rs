use std::collections::{HashMap};
use crate::app::components::{Block, BlockId, Connection};

const LAYER_DISTANCE: f32 = 200.0; // X Distance
const NODE_DISTANCE: f32 = 100.0; // Y distance
const CANVAS_OFFSET: f32 = 10.0;

/// Computes layers so that `from` nodes are always left of `to` nodes
fn compute_layers_from_to(blocks: &[Block], connections: &[Connection]) -> HashMap<BlockId, usize> {
    let mut layers: HashMap<BlockId, usize> = HashMap::new();

    // Start with all blocks in layer 0
    for block in blocks {
        layers.insert(block.id, 0);
    }

    // Propagate layer constraints: to >= from + 1
    let mut changed = true;
    while changed {
        changed = false;
        for conn in connections {
            let from_layer = *layers.get(&conn.from).unwrap();
            let to_layer = *layers.get(&conn.to).unwrap();
            if to_layer <= from_layer {
                layers.insert(conn.to, from_layer + 1);
                changed = true;
            }
        }
    }

    layers
}

pub fn hierarchical_layout(blocks: &mut [Block], connections: &[Connection]) {
    let block_layers = compute_layers_from_to(blocks, connections);

    // Build layers -> block IDs map
    let mut layers_map: HashMap<usize, Vec<BlockId>> = HashMap::new();
    for (&block_id, &layer_index) in block_layers.iter() {
        layers_map.entry(layer_index).or_default().push(block_id);
    }

    // Temporary map to store Y positions
    let mut block_y: HashMap<BlockId, f32> = HashMap::new();

    // Sort layers
    let mut sorted_layers: Vec<_> = layers_map.keys().cloned().collect();
    sorted_layers.sort_unstable();

    // Top-down pass: initial Y based on parent average
    for &layer_index in &sorted_layers {
        let block_ids = &layers_map[&layer_index];
        for &block_id in block_ids {
            let from_blocks: Vec<&Block> = connections
                .iter()
                .filter(|c| c.to == block_id)
                .filter_map(|c| blocks.iter().find(|b| b.id == c.from))
                .collect();

            let y = if !from_blocks.is_empty() {
                let sum: f32 = from_blocks.iter()
                    .map(|b| block_y.get(&b.id).copied().unwrap_or(0.0))
                    .sum();
                sum / from_blocks.len() as f32
            } else {
                block_ids.iter().position(|&id| id == block_id).unwrap() as f32 * NODE_DISTANCE
            };

            block_y.insert(block_id, y);
        }
    }

    // Resolve overlaps within each layer & propagate adjustments to parents
    for &layer_index in &sorted_layers {
        let block_ids = &layers_map[&layer_index];
        let mut sorted_blocks: Vec<_> = block_ids.to_vec();
        sorted_blocks.sort_by(|a, b| block_y[a].partial_cmp(&block_y[b]).unwrap());

        // Determine min/max Y of this layer based on parent positions
        let min_y = sorted_blocks.iter().map(|id| block_y[id]).fold(f32::INFINITY, f32::min);
        let max_y = sorted_blocks.iter().map(|id| block_y[id]).fold(f32::NEG_INFINITY, f32::max);

        let available_height = max_y - min_y;
        let required_height = (sorted_blocks.len() - 1) as f32 * NODE_DISTANCE;

        let offset = if available_height > required_height {
            // Center nodes in the available span
            (available_height - required_height) / 2.0
        } else {
            0.0
        };

        let mut current_y = min_y + offset;
        for &block_id in &sorted_blocks {
            block_y.insert(block_id, current_y);
            current_y += NODE_DISTANCE;
        }
    }

    // Assign final positions
    for block in blocks.iter_mut() {
        let x = block_layers[&block.id] as f32 * LAYER_DISTANCE + CANVAS_OFFSET;
        let y = block_y[&block.id] + CANVAS_OFFSET;
        block.position = (x, y);
    }
}
