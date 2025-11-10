use crate::app::components::{Block, BlockId, Connection};
use std::collections::HashMap;

const CANVAS_OFFSET: f32 = 10.0;

const Y_GAP: f32 = 25.0;
const X_GAP: f32 = 50.0;

const BLOCK_HEIGHT: f32 = 50.0;
const BLOCK_WIDTH: f32 = 200.0;

struct TargetBlock {
    id: BlockId,
    outputs: Option<Vec<BlockId>>,
    height: f32,
}

impl TargetBlock {
    pub fn new(id: BlockId, outputs: Option<Vec<BlockId>>) -> Self {
        let height = Self::height(outputs.as_ref());
        TargetBlock { id, outputs, height }
    }

    fn height(outputs: Option<&Vec<BlockId>>) -> f32 {
        if let Some(outs) = outputs {
            let len = outs.len() as f32;
            ((len * BLOCK_HEIGHT) + ((len - 1.0) * Y_GAP)).max(BLOCK_HEIGHT)
        } else {
            BLOCK_HEIGHT
        }
    }

    pub fn set_position(&self, x: f32, y: f32, blocks: &mut [Block]) {
        let out_x = x + BLOCK_WIDTH + X_GAP;
        let mut out_y = y;
        if let Some(outputs) = self.outputs.as_ref() {
            for out in outputs {
                blocks[*out as usize -1].position = (out_x, out_y);
                out_y += BLOCK_HEIGHT + Y_GAP;
            }
        }
        blocks[self.id as usize - 1].position = (x, y + (self.height - BLOCK_HEIGHT) / 2.0);
    }
}

fn build_target_blocks(blocks: &mut [Block], connections: &[Connection]) -> Vec<TargetBlock> {
    let mut out_edges: HashMap<BlockId, Vec<BlockId>> = HashMap::new();
    let mut in_edges: HashMap<BlockId, Vec<BlockId>> = HashMap::new();

    for c in connections {
        out_edges.entry(c.from).or_default().push(c.to);
        in_edges.entry(c.to).or_default().push(c.from);
    }

    blocks
        .iter()
        .filter(|b| b.block_type.is_target())
        .map(|b| TargetBlock::new(b.id, out_edges.get(&b.id).cloned()))
        .collect()
}

pub fn layout(blocks: &mut [Block], connections: &[Connection]) {

    let target_blocks = build_target_blocks(blocks, connections);
    let mut start_y = CANVAS_OFFSET;
    let start_x = CANVAS_OFFSET + BLOCK_WIDTH  + X_GAP;
    for  target_block in &target_blocks {
        target_block.set_position(start_x, start_y, blocks);
        start_y += target_block.height + Y_GAP;
    }


    let target_map: HashMap<BlockId, &TargetBlock> = target_blocks
        .iter()
        .map(|t| (t.id, t))
        .collect();

    let target_positions: HashMap<BlockId, f32> = target_blocks
        .iter()
        .map(|t| (t.id, blocks[t.id as usize - 1].position.1))
        .collect();

    let mut last_input_y = CANVAS_OFFSET;

    for block in blocks.iter_mut().filter(|b| b.block_type.is_input()) {
        let block_id = block.id;
        let connected_targets: Vec<&TargetBlock> = connections
            .iter()
            .filter(|c| c.from == block_id)
            .filter_map(|c| target_map.get(&c.to))
            .copied()
            .collect();

        let y_pos = if connected_targets.is_empty() {
            let y = last_input_y;
            last_input_y += BLOCK_HEIGHT + Y_GAP;
            y
        } else if connected_targets.len() == 1 {
            target_positions[&connected_targets[0].id] // Position direkt aus HashMap
        } else {
            let min_y = connected_targets
                .iter()
                .map(|t| target_positions[&t.id])
                .fold(f32::INFINITY, |a, b| a.min(b));

            let max_y = connected_targets
                .iter()
                .map(|t| target_positions[&t.id])
                .fold(f32::NEG_INFINITY, |a, b| a.max(b));

            (min_y + max_y) / 2.0
        };

        block.position = (CANVAS_OFFSET, y_pos.max(last_input_y));
        last_input_y = block.position.1 + BLOCK_HEIGHT + Y_GAP;
    }
}