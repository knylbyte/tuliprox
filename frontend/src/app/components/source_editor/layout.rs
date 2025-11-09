use std::collections::{HashMap, HashSet};
use crate::app::components::{BlockId, BlockType, Block, Connection};

const BLOCK_SIZE: f32 = 50.0;
const GAP: f32 = 20.0;
const LAYER_DISTANCE: f32 = BLOCK_SIZE * 3.0;
const CANVAS_OFFSET: f32 = 10.0;
const ITERATIONS: usize = 5;

/// Cluster structure
struct Cluster {
    targets: Vec<BlockId>,
    inputs: Vec<BlockId>,
    outputs_per_target: HashMap<BlockId, Vec<BlockId>>,
}

/// Build clusters from connections
fn build_clusters(blocks: &[Block], connections: &[Connection]) -> Vec<Cluster> {
    let mut clusters = Vec::new();
    let mut visited_targets = HashSet::new();
    let block_map: HashMap<BlockId, &Block> = blocks.iter().map(|b| (b.id, b)).collect();

    for block in blocks {
        if block.block_type != BlockType::Target || visited_targets.contains(&block.id) {
            continue;
        }

        let mut cluster = Cluster {
            targets: vec![block.id],
            inputs: Vec::new(),
            outputs_per_target: HashMap::new(),
        };
        visited_targets.insert(block.id);

        // Collect inputs connected to this target
        for conn in connections.iter().filter(|c| c.to == block.id) {
            if let Some(input) = block_map.get(&conn.from) {
                if matches!(input.block_type, BlockType::InputXtream | BlockType::InputM3u) {
                    cluster.inputs.push(input.id);
                }
            }
        }

        // Collect outputs for this target
        let outputs: Vec<BlockId> = connections.iter()
            .filter(|c| c.from == block.id)
            .filter_map(|c| block_map.get(&c.to).filter(|b| matches!(b.block_type,
                BlockType::OutputXtream |
                BlockType::OutputM3u |
                BlockType::OutputHdHomeRun |
                BlockType::OutputStrm)).map(|b| b.id))
            .collect();

        cluster.outputs_per_target.insert(block.id, outputs);
        clusters.push(cluster);
    }

    clusters
}

/// Main hierarchical cluster layout function
pub fn cluster_layout(blocks: &mut [Block], connections: &[Connection]) {
    let clusters = build_clusters(blocks, connections);
    let mut block_map: HashMap<BlockId, &mut Block> = blocks.iter_mut().map(|b| (b.id, b)).collect();
    let mut y_offset = CANVAS_OFFSET;
    let mut placed_inputs = HashSet::new();

    for cluster in clusters {
        // Compute cluster height based on max(column heights)
        let num_targets = cluster.targets.len();
        let num_inputs = cluster.inputs.len();
        let max_outputs = cluster.outputs_per_target.values().map(|v| v.len()).max().unwrap_or(0);
        let cluster_height = ((num_targets.max(num_inputs).max(max_outputs)) as f32 * BLOCK_SIZE) +
            (((num_targets.max(num_inputs).max(max_outputs)) -1) as f32 * GAP);

        // --- Step 1: Place Targets (raw Y, ignore outputs for now)
        let target_start_y = y_offset + (cluster_height - (num_targets as f32 * BLOCK_SIZE + (num_targets-1) as f32*GAP))/2.0;
        let mut target_y_map = HashMap::new();
        for (i, &target_id) in cluster.targets.iter().enumerate() {
            let y = target_start_y + i as f32 * (BLOCK_SIZE + GAP) + BLOCK_SIZE/2.0;
            if let Some(t) = block_map.get_mut(&target_id) {
                t.position = (LAYER_DISTANCE + CANVAS_OFFSET, y);
                target_y_map.insert(target_id, y);
            }
        }

        // --- Step 2: Place Inputs (centered on connected targets)
        let mut input_positions: HashMap<BlockId, f32> = HashMap::new();
        let mut input_to_targets: HashMap<BlockId, Vec<BlockId>> = HashMap::new();

        for &input_id in &cluster.inputs {
            let connected_targets: Vec<BlockId> = connections.iter()
                .filter(|c| c.to == input_id && cluster.targets.contains(&c.from))
                .map(|c| c.from)
                .collect();

            input_to_targets.insert(input_id, connected_targets.clone());

            // Average Y of connected targets
            let y = if !connected_targets.is_empty() {
                connected_targets.iter().map(|t| target_y_map[t]).sum::<f32>() / connected_targets.len() as f32
            } else {
                cluster_height / 2.0 + y_offset
            };
            input_positions.insert(input_id, y);
        }

        // --- Step 3: Spread inputs exclusive to a single target
        for &target_id in &cluster.targets {
            let exclusive_inputs: Vec<BlockId> = input_to_targets.iter()
                .filter(|(_, targets)| targets.len() == 1 && targets[0] == target_id)
                .map(|(&id, _)| id)
                .collect();

            let count = exclusive_inputs.len();
            if count > 1 {
                let target_y = target_y_map[&target_id];
                let total_height = count as f32 * BLOCK_SIZE + (count-1) as f32 * GAP;
                let start_y = target_y - total_height / 2.0 + BLOCK_SIZE / 2.0;

                for (i, input_id) in exclusive_inputs.iter().enumerate() {
                    input_positions.insert(*input_id, start_y + i as f32 * (BLOCK_SIZE + GAP));
                }
            }
        }

        // --- Step 4: Resolve overlaps iteratively
        for _ in 0..ITERATIONS {
            let mut sorted_inputs: Vec<(BlockId, f32)> = input_positions.iter().map(|(id, y)| (*id, *y)).collect();
            sorted_inputs.sort_by(|a,b| a.1.partial_cmp(&b.1).unwrap());

            for i in 1..sorted_inputs.len() {
                let prev_y = sorted_inputs[i-1].1;
                let curr_id = sorted_inputs[i].0;
                let curr_y = sorted_inputs[i].1;
                if curr_y - prev_y < BLOCK_SIZE + GAP {
                    let new_y = prev_y + BLOCK_SIZE + GAP;
                    if let Some(y_val) = input_positions.get_mut(&curr_id) {
                        *y_val = new_y;
                    }
                }
            }
        }

        // --- Step 5: Commit final input positions
        for (&input_id, &y) in input_positions.iter() {
            if let Some(b) = block_map.get_mut(&input_id) {
                b.position = (CANVAS_OFFSET, y);
                placed_inputs.insert(input_id);
            }
        }

        // --- Step 6: Place Outputs (centered under target)
        for (&target_id, outputs) in cluster.outputs_per_target.iter() {
            let target_y = target_y_map[&target_id];
            let output_count = outputs.len();
            if output_count == 0 { continue; }

            let outputs_height = output_count as f32 * BLOCK_SIZE + (output_count as f32 -1.0)*GAP;
            let output_start_y = target_y - outputs_height / 2.0;

            for (i, &out_id) in outputs.iter().enumerate() {
                if let Some(b) = block_map.get_mut(&out_id) {
                    let y = output_start_y + i as f32 * (BLOCK_SIZE + GAP) + BLOCK_SIZE/2.0;
                    b.position = (2.0*LAYER_DISTANCE + CANVAS_OFFSET, y);
                }
            }
        }

        // --- Step 7: Update y_offset for next cluster
        y_offset += cluster_height + GAP*2.0;
    }

    // --- Step 8: Place orphan inputs in a separate column
    let orphan_inputs: Vec<BlockId> = block_map.keys()
        .filter(|id| {
            let b = block_map.get(id).unwrap();
            matches!(b.block_type, BlockType::InputXtream | BlockType::InputM3u)
                && !placed_inputs.contains(id)
        })
        .cloned()
        .collect();

    for (i, input_id) in orphan_inputs.iter().enumerate() {
        if let Some(b) = block_map.get_mut(input_id) {
            let y = CANVAS_OFFSET + i as f32 * (BLOCK_SIZE + GAP) + BLOCK_SIZE / 2.0;
            b.position = (CANVAS_OFFSET, y);
        }
    }
}
