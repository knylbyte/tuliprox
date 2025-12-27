use crate::utils;
use indexmap::IndexMap;
use log::error;
use serde::{Deserialize, Serialize};
use shared::error::{str_to_io_error, to_io_error};
use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::marker::PhantomData;
use std::mem::size_of;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;
use crate::utils::{binary_deserialize, binary_serialize};

const BLOCK_SIZE: usize = 4096;
const LEN_SIZE: usize = 4;
const FLAG_SIZE: usize = 1;
const MAGIC: &[u8; 4] = b"BTRE";
const STORAGE_VERSION: u32 = 3;
const HEADER_SIZE: u64 = BLOCK_SIZE as u64;
const ROOT_OFFSET_POS: u64 = 8;

fn is_multiple_of_block_size(file: &File) -> io::Result<bool> {
    let file_size = file.metadata()?.len(); // Get the file size in bytes
    Ok(file_size.is_multiple_of(BLOCK_SIZE as u64)) // Check if file size is a multiple of BLOCK_SIZE
}

fn is_file_valid(file: File) -> io::Result<File> {
    match is_multiple_of_block_size(&file) {
        Ok(valid) => {
            if !valid {
                return Err(io::Error::new(io::ErrorKind::InvalidData, format!("Tree file has to be multiple of block size {BLOCK_SIZE}")));
            }
        }
        Err(err) => return Err(err)
    }
    Ok(file)
}

#[inline]
fn u32_from_bytes(bytes: &[u8]) -> io::Result<u32> {
    Ok(u32::from_le_bytes(bytes.try_into().map_err(to_io_error)?))
}

#[inline]
fn get_entry_index_upper_bound<K>(keys: &[K], key: &K) -> usize
where
    K: Ord + Serialize + for<'de> Deserialize<'de> + Clone,
{
    let mut left = 0;
    let mut right = keys.len();
    while left < right {
        let mid = left + ((right - left) >> 1);
        if &keys[mid] <= key {
            left = mid + 1;
        } else {
            right = mid;
        }
    }
    left
}



#[derive(Debug, Clone)]
struct BPlusTreeNode<K, V> {
    keys: Vec<K>,
    children: Vec<BPlusTreeNode<K, V>>,
    is_leaf: bool,
    value_info: Vec<(u64, u32)>, // Store (offset, length) for values in leaf nodes
    values: Vec<V>,              // only used in leaf nodes
}

impl<K, V> BPlusTreeNode<K, V>
where
    K: Ord + Serialize + for<'de> Deserialize<'de> + Clone,
    V: Serialize + for<'de> Deserialize<'de> + Clone,
{
    #[inline]
    const fn new(is_leaf: bool) -> Self {
        Self {
            is_leaf,
            keys: vec![],
            children: vec![],
            value_info: vec![],
            values: vec![],
        }
    }

    // pub fn count(&self) -> usize {
    //     if self.is_leaf {
    //         self.values.len()
    //     } else {
    //         self.children.iter().map(|child| child.count()).sum()
    //     }
    // }

    #[inline]
    fn is_overflow(&self, order: usize) -> bool {
        self.keys.len() > order
    }

    #[inline]
    const fn get_median_index(order: usize) -> usize {
        order >> 1
    }

    fn find_leaf_entry(node: &Self) -> Option<&K> {
        if node.is_leaf {
            node.keys.first()
        } else if let Some(child) = node.children.first() {
            Self::find_leaf_entry(child)
        } else {
            None
        }
    }

    fn query(&self, key: &K) -> Option<&V> {
        if self.is_leaf {
            return self.keys.binary_search(key).map_or(None, |idx| self.values.get(idx));
        }
        self.children.get(self.get_entry_index_upper_bound(key))?.query(key)
    }

    fn get_entry_index_upper_bound(&self, key: &K) -> usize {
        get_entry_index_upper_bound::<K>(&self.keys, key)
    }

    fn insert(&mut self, key: K, v: V, inner_order: usize, leaf_order: usize) -> Option<Self> {
        if self.is_leaf {
            // Use single binary search instead of redundant searches
            match self.keys.binary_search(&key) {
                Ok(pos) => {
                    // Key exists, update value
                    self.values[pos] = v;
                    return None;
                }
                Err(pos) => {
                    // Key doesn't exist, insert at correct position
                    self.keys.insert(pos, key);
                    self.values.insert(pos, v);
                    if self.is_overflow(leaf_order) {
                        return Some(self.split(leaf_order));
                    }
                }
            }
        } else {
            let pos = self.get_entry_index_upper_bound(&key);
            let child = self.children.get_mut(pos)?;
            let node = child.insert(key.clone(), v, inner_order, leaf_order);
            if let Some(tree_node) = node {
                if let Some(leaf_key) = Self::find_leaf_entry(&tree_node) {
                    let idx = self.get_entry_index_upper_bound(leaf_key);
                    if self.keys.binary_search(leaf_key).is_err() {
                        self.keys.insert(idx, leaf_key.clone());
                        self.children.insert(idx + 1, tree_node);
                        if self.is_overflow(inner_order) {
                            return Some(self.split(inner_order));
                        }
                    }
                }
            }
        }
        None
    }

    fn split(&mut self, order: usize) -> Self {
        let median = Self::get_median_index(order);
        if self.is_leaf {
            let mut node = Self::new(true);
            node.keys = self.keys.split_off(median);
            node.values = self.values.split_off(median);
            node
        } else {
            let mut node = Self::new(false);
            node.keys = self.keys.split_off(median + 1);
            node.children = self.children.split_off(median + 1);
            // No need to clone and push - split_off already handles the split correctly
            node
        }
    }

    /// Find the largest key <= `key` in this subtree.
    /// Returns a reference to (key, value) if found (only valid for leaf entries).
    fn find_le(&self, key: &K) -> Option<(&K, &V)> {
        if self.is_leaf {
            // find index of first key > key, then step one back
            let idx = self.get_entry_index_upper_bound(key);
            if idx == 0 {
                None
            } else {
                let i = idx - 1;
                // safe: leaf guarantees values.len() == keys.len()
                Some((&self.keys[i], &self.values[i]))
            }
        } else {
            // descend into the appropriate child (child index = upper_bound)
            let child_idx = self.get_entry_index_upper_bound(key);
            // child_idx can be equal to children.len() if key > all keys; children.get handles that
            if let Some(child) = self.children.get(child_idx) {
                child.find_le(key)
            } else {
                // fallback: if child_idx is out of bounds, try last child (defensive)
                self.children.last().and_then(|c| c.find_le(key))
            }
        }
    }

    pub fn traverse<F>(&self, visit: &mut F)
    where
        F: FnMut(&Vec<K>, &Vec<V>),
    {
        if self.is_leaf {
            visit(&self.keys, &self.values);
        }
        self.children.iter().for_each(|child| child.traverse(visit));
    }

    /// Calculate the serialized size of this node in bytes (rounded up to block size)
    fn calculate_serialized_size(&self) -> io::Result<u64> {
        // Header: is_leaf flag
        let mut size = FLAG_SIZE;
        
        // Keys: length + serialized data
        let keys_encoded = binary_serialize(&self.keys)?;
        size += LEN_SIZE + keys_encoded.len();
        
        if self.is_leaf {
            // Leaf nodes now store value_info instead of values
            // value_info: length + Vec<(u64, u32)>
            let info_encoded = binary_serialize(&self.value_info)?;
            size += LEN_SIZE + info_encoded.len();
        } else {
            // Internal node: pointer length + pointers
            // We use current children count for estimate
            let mut pointer_vec: Vec<u64> = vec![0; self.children.len()];
            pointer_vec.resize(self.children.len(), 0);
            let pointer_encoded = binary_serialize(&pointer_vec)?;
            size += LEN_SIZE + pointer_encoded.len();
        }
        
        // Round up to block size
        let blocks = size.div_ceil(BLOCK_SIZE);
        Ok((blocks * BLOCK_SIZE) as u64)
    }

    fn serialize_to_block<W: Write + Seek>(
        &self,
        file: &mut W,
        buffer: &mut Vec<u8>, // must be length BLOCK_SIZE
        offset: u64,
    ) -> io::Result<u64> {
        // Keep backward-compatible on-disk layout with minimal allocations.
        let mut current_offset = offset;

        // Set node type (no need to zero entire buffer upfront)
        let buffer_slice = &mut buffer[..];
        buffer_slice[0] = u8::from(self.is_leaf);
        let mut write_pos = FLAG_SIZE;

        // ---- Write keys (length + bytes) into the first block ----
        let keys_encoded = binary_serialize(&self.keys)?;
        let keys_len = keys_encoded.len();
        buffer_slice[write_pos..write_pos + LEN_SIZE]
            .copy_from_slice(&u32::try_from(keys_len).map_err(to_io_error)?.to_le_bytes());
        write_pos += LEN_SIZE;
        // NOTE: By design of the legacy layout, keys are expected to fit into the first block.
        buffer_slice[write_pos..write_pos + keys_len].copy_from_slice(&keys_encoded);
        write_pos += keys_len;
        drop(keys_encoded);

        if self.is_leaf {
            // ---- Leaf nodes: write value info (offset + length) ----
            let info_encoded = binary_serialize(&self.value_info)?;
            let info_len = info_encoded.len();
            
            // CRITICAL CHECK: Ensure metadata fits in block
            if write_pos + LEN_SIZE + info_len > BLOCK_SIZE {
                return Err(io::Error::other(format!("Leaf node overflow: keys ({keys_len}) + value_info ({info_len}) exceeds block size")));
            }

            buffer_slice[write_pos..write_pos + LEN_SIZE]
                .copy_from_slice(&u32::try_from(info_len).map_err(to_io_error)?.to_le_bytes());
            write_pos += LEN_SIZE;
            buffer_slice[write_pos..write_pos + info_len].copy_from_slice(&info_encoded);
            write_pos += info_len;

            // Zero unused portion and write first block
            if write_pos < BLOCK_SIZE {
                buffer_slice[write_pos..BLOCK_SIZE].fill(0u8);
            }
            file.seek(SeekFrom::Start(offset))?;
            file.write_all(&buffer_slice[..BLOCK_SIZE])?;
            current_offset += BLOCK_SIZE as u64;
        } else {
            // Internal node: write breadth-first style pointers
            let pointer_offset_within_first_block = offset + write_pos as u64;

            // Zero unused portion and write first block
            if write_pos < BLOCK_SIZE {
                buffer_slice[write_pos..BLOCK_SIZE].fill(0u8);
            }
            file.seek(SeekFrom::Start(offset))?;
            file.write_all(&buffer_slice[..BLOCK_SIZE])?;
            current_offset += BLOCK_SIZE as u64;

            let mut pointer_vec: Vec<u64> = Vec::with_capacity(self.children.len());
            for child in &self.children {
                pointer_vec.push(current_offset);
                current_offset = child.serialize_to_block(file, buffer, current_offset)?;
            }

            let pointer_encoded = binary_serialize(&pointer_vec)?;
            let pointer_len = u32::try_from(pointer_encoded.len()).map_err(to_io_error)?;

            // CRITICAL CHECK: Ensure pointers fit in block
            if write_pos + LEN_SIZE + pointer_encoded.len() > BLOCK_SIZE {
                 return Err(io::Error::other(format!("Internal node overflow during recursion: keys ({}) + pointers ({}) exceeds block size", keys_len, pointer_encoded.len())));
            }

            file.seek(SeekFrom::Start(pointer_offset_within_first_block))?;
            file.write_all(&pointer_len.to_le_bytes())?;
            file.write_all(&pointer_encoded)?;
        }

        Ok(current_offset)
    }

    /// Serialize the tree in breadth-first order for better disk locality
    /// This improves query performance by keeping nodes at the same level contiguous
    fn serialize_breadth_first<W: Write + Seek>(
        &mut self,
        file: &mut W,
        buffer: &mut Vec<u8>,
        start_offset: u64,
    ) -> io::Result<u64> {
        use std::collections::HashMap;
        
        // Pass 1: Calculate offsets for all nodes in breadth-first order (Immutable)
        let mut node_offset_map: HashMap<*const BPlusTreeNode<K, V>, u64> = HashMap::new();
        let mut current_offset = start_offset;
        
        {
            let mut current_level = vec![&*self];
            node_offset_map.insert(std::ptr::from_ref(self), current_offset);
            current_offset += self.calculate_serialized_size()?;

            while !current_level.is_empty() {
                let mut next_level = Vec::new();
                for node in current_level {
                    if !node.is_leaf {
                        for child in &node.children {
                            let child_ptr = std::ptr::from_ref(child);
                            node_offset_map.insert(child_ptr, current_offset);
                            current_offset += child.calculate_serialized_size()?;
                            next_level.push(child);
                        }
                    }
                }
                current_level = next_level;
            }
        }
        
        // Pass 2: Calculate offsets for value blocks and update value_info (Mutable)
        {
            let mut current_level_mut = vec![&mut *self];
            while !current_level_mut.is_empty() {
                let mut next_level_mut = Vec::new();
                for node in current_level_mut {
                    if node.is_leaf {
                        node.value_info.clear();
                        for value in &node.values {
                            let value_bytes = binary_serialize(value)?;
                            let value_len = u32::try_from(value_bytes.len()).unwrap_or(0);
                            node.value_info.push((current_offset, value_len));
                            
                            let blocks_needed = (value_len as usize).div_ceil(BLOCK_SIZE);
                            current_offset += (blocks_needed * BLOCK_SIZE) as u64;
                        }
                    } else {
                        for child in &mut node.children {
                            next_level_mut.push(child);
                        }
                    }
                }
                current_level_mut = next_level_mut;
            }
        }
        
        // Pass 3: Write nodes with their keys and value pointers (Immutable)
        {
            let mut current_level_indices = vec![&*self];
            while !current_level_indices.is_empty() {
                let mut next_level = Vec::new();
                for node in current_level_indices {
                    let node_ptr = std::ptr::from_ref(node);
                    let node_offset = node_offset_map[&node_ptr];
                    
                    if node.is_leaf {
                        node.serialize_to_block(file, buffer, node_offset)?;
                    } else {
                        let child_offsets: Vec<u64> = node.children.iter()
                            .map(|c| node_offset_map[&std::ptr::from_ref(c)])
                            .collect();
                        
                        node.serialize_internal_with_offsets(file, buffer, node_offset, &child_offsets)?;
                        for child in &node.children {
                            next_level.push(child);
                        }
                    }
                }
                current_level_indices = next_level;
            }
        }
        
        // Pass 4: Write all value blocks in a contiguous data pool (Immutable)
        {
            let mut current_level_values = vec![&*self];
            while !current_level_values.is_empty() {
                let mut next_level = Vec::new();
                for node in current_level_values {
                    if node.is_leaf {
                        for (value, &(val_offset, _val_len)) in node.values.iter().zip(node.value_info.iter()) {
                            let value_bytes = binary_serialize(value)?;
                            file.seek(SeekFrom::Start(val_offset))?;
                            
                            let mut pos = 0;
                            while pos < value_bytes.len() {
                                let chunk = std::cmp::min(BLOCK_SIZE, value_bytes.len() - pos);
                                buffer[..chunk].copy_from_slice(&value_bytes[pos..pos + chunk]);
                                if chunk < BLOCK_SIZE {
                                    buffer[chunk..BLOCK_SIZE].fill(0u8);
                                }
                                file.write_all(buffer)?;
                                pos += chunk;
                            }
                        }
                    } else {
                        for child in &node.children {
                            next_level.push(child);
                        }
                    }
                }
                current_level_values = next_level;
            }
        }
        
        Ok(current_offset)
    }

    /// Serialize an internal node with pre-calculated child offsets
    fn serialize_internal_with_offsets<W: Write + Seek>(
        &self,
        file: &mut W,
        buffer: &mut [u8],
        offset: u64,
        child_offsets: &[u64],
    ) -> io::Result<u64> {
        // Similar to serialize_to_block but for internal nodes with known child offsets
        let buffer_slice = &mut buffer[..];
        buffer_slice[0] = u8::from(self.is_leaf);
        let mut write_pos = FLAG_SIZE;

        // Write keys
        let keys_encoded = binary_serialize(&self.keys)?;
        let keys_len = keys_encoded.len();
        buffer_slice[write_pos..write_pos + LEN_SIZE]
            .copy_from_slice(&u32::try_from(keys_len).map_err(to_io_error)?.to_le_bytes());
        write_pos += LEN_SIZE;
        buffer_slice[write_pos..write_pos + keys_len].copy_from_slice(&keys_encoded);
        write_pos += keys_len;
        drop(keys_encoded);

        let pointer_offset_within_first_block = offset + write_pos as u64;

        // Zero unused portion and write first block
        if write_pos < BLOCK_SIZE {
            buffer_slice[write_pos..BLOCK_SIZE].fill(0u8);
        }
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(&buffer_slice[..BLOCK_SIZE])?;

        // Write child pointers
        let pointer_encoded = binary_serialize(child_offsets)?;
        let pointer_len = u32::try_from(pointer_encoded.len()).map_err(to_io_error)?;
        
        // CRITICAL CHECK: Ensure pointers fit in the remaining space of the first block or we've allocated enough
        if write_pos + LEN_SIZE + pointer_encoded.len() > BLOCK_SIZE {
             return Err(io::Error::other(format!("Internal node overflow: keys ({}) + pointers ({}) exceeds block size. Consider reducing ORDER.", keys_len, pointer_encoded.len())));
        }

        file.seek(SeekFrom::Start(pointer_offset_within_first_block))?;
        file.write_all(&pointer_len.to_le_bytes())?;
        file.write_all(&pointer_encoded)?;

        Ok(offset + BLOCK_SIZE as u64)
    }

    fn deserialize_from_block<R: Read + Seek>(
        file: &mut R,
        buffer: &mut [u8],
        offset: u64,
        nested: bool,
    ) -> io::Result<(Self, Option<Vec<u64>>)> {
        // Read the full first block into buffer (always aligned on-disk)
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(buffer)?;

        let slice = &buffer[..];
        
        // Node type
        let is_leaf = slice[0] == 1u8;
        let mut read_pos = FLAG_SIZE;

        // ---- Keys ----
        let keys_length = u32_from_bytes(&slice[read_pos..read_pos + LEN_SIZE])? as usize;
        read_pos += LEN_SIZE;
        let keys: Vec<K> = binary_deserialize(&slice[read_pos..read_pos + keys_length])?;
        read_pos += keys_length;

        // ---- Value info (offset, length) for leaf nodes ----
        let (value_info, values): (Vec<(u64, u32)>, Vec<V>) = if is_leaf {
            // Read value_info
            let info_length = u32_from_bytes(&slice[read_pos..read_pos + LEN_SIZE])? as usize;
            read_pos += LEN_SIZE;
            let info: Vec<(u64, u32)> = binary_deserialize(&slice[read_pos..read_pos + info_length])?;
            
            // Values are loaded on-demand when nested=true
            if nested {
                let mut vals = Vec::with_capacity(info.len());
                for &(value_offset, value_len) in &info {
                    let value = Self::load_value_with_len(file, value_offset, value_len)?;
                    vals.push(value);
                }
                (info, vals)
            } else {
                (info, Vec::new())
            }
        } else {
            (Vec::new(), Vec::new())
        };

        // ---- Pointers for internal nodes ----
        let (children, children_pointer): (Vec<Self>, Option<Vec<u64>>) = if is_leaf {
            (Vec::new(), None)
        } else {
            let pointers_length = u32_from_bytes(&slice[read_pos..read_pos + LEN_SIZE])? as usize;
            read_pos += LEN_SIZE;
            let pointers: Vec<u64> = binary_deserialize(&slice[read_pos..read_pos + pointers_length])?;
            if nested {
                let mut nodes = Vec::with_capacity(pointers.len());
                let mut child_buffer = vec![0u8; BLOCK_SIZE];
                for &ptr in &pointers {
                    let (child, _) = Self::deserialize_from_block(file, &mut child_buffer, ptr, nested)?;
                    nodes.push(child);
                }
                (nodes, None)
            } else {
                (Vec::new(), Some(pointers))
            }
        };

        Ok((Self { keys, children, is_leaf, value_info, values }, children_pointer))
    }

    fn deserialize_from_block_slice<R: Read + Seek>(
        slice: &[u8],
        file: &mut R,
        nested: bool,
    ) -> io::Result<(Self, Option<Vec<u64>>)> {
        // Node type
        let is_leaf = slice[0] == 1u8;
        let mut read_pos = FLAG_SIZE;

        // ---- Keys ----
        let keys_length = u32_from_bytes(&slice[read_pos..read_pos + LEN_SIZE])? as usize;
        read_pos += LEN_SIZE;
        let keys: Vec<K> = binary_deserialize(&slice[read_pos..read_pos + keys_length])?;
        read_pos += keys_length;

        // ---- Value info (offset, length) for leaf nodes ----
        let (value_info, values): (Vec<(u64, u32)>, Vec<V>) = if is_leaf {
            // Read value_info
            let info_length = u32_from_bytes(&slice[read_pos..read_pos + LEN_SIZE])? as usize;
            read_pos += LEN_SIZE;
            let info: Vec<(u64, u32)> = binary_deserialize(&slice[read_pos..read_pos + info_length])?;
            
            // Values are loaded on-demand when nested=true
            if nested {
                let mut vals = Vec::with_capacity(info.len());
                for &(value_offset, value_len) in &info {
                    let value = Self::load_value_with_len(file, value_offset, value_len)?;
                    vals.push(value);
                }
                (info, vals)
            } else {
                (info, Vec::new())
            }
        } else {
            (Vec::new(), Vec::new())
        };

        // ---- Pointers for internal nodes ----
        let (children, children_pointer): (Vec<Self>, Option<Vec<u64>>) = if is_leaf {
            (Vec::new(), None)
        } else {
            let pointers_length = u32_from_bytes(&slice[read_pos..read_pos + LEN_SIZE])? as usize;
            read_pos += LEN_SIZE;
            let pointers: Vec<u64> = binary_deserialize(&slice[read_pos..read_pos + pointers_length])?;
            if nested {
                let mut nodes = Vec::with_capacity(pointers.len());
                let mut child_buffer = vec![0u8; BLOCK_SIZE];
                for &ptr in &pointers {
                    let (child, _) = Self::deserialize_from_block(file, &mut child_buffer, ptr, nested)?;
                    nodes.push(child);
                }
                (nodes, None)
            } else {
                (Vec::new(), Some(pointers))
            }
        };

        Ok((Self { keys, children, is_leaf, value_info, values }, children_pointer))
    }

    fn load_value_with_len<R: Read + Seek>(file: &mut R, offset: u64, length: u32) -> io::Result<V> {
        file.seek(SeekFrom::Start(offset))?;
        
        let mut value_data = vec![0u8; length as usize];
        file.read_exact(&mut value_data)?;
        
        binary_deserialize(&value_data)
    }
}

#[derive(Debug, Clone)]
pub struct BPlusTree<K, V> {
    root: BPlusTreeNode<K, V>,
    inner_order: usize,
    leaf_order: usize,
    dirty: bool,
}

const fn calc_order<K>() -> (usize, usize) {
    // Phase 2 Layout:
    // Internal: FLAG (1) + LEN_K (4) + KEYS + LEN_P (4) + POINTERS (8 each)
    // Leaf:    FLAG (1) + LEN_K (4) + KEYS + LEN_INFO (4) + VALUE_INFO (12 each)
    
    let base_overhead = FLAG_SIZE + LEN_SIZE + LEN_SIZE + 64; // flag + keys_len + info_len + safety buffer
    let key_size = size_of::<K>();
    let pointer_size = 8;
    let info_size = 12; // (u64, u32)
    
    // MessagePack overhead can be more than 2 bytes for larger types
    let msgpack_overhead_per_entry = 4; 

    let inner_order = (BLOCK_SIZE - base_overhead) / (key_size + pointer_size + msgpack_overhead_per_entry);
    let leaf_order = (BLOCK_SIZE - base_overhead) / (key_size + info_size + msgpack_overhead_per_entry);
    
    // Ensure we have at least a minimal order (manual max for const fn)
    let final_inner = if inner_order < 2 { 2 } else { inner_order };
    let final_leaf = if leaf_order < 2 { 2 } else { leaf_order };
    (final_inner, final_leaf)
}

impl<K, V> Default for BPlusTree<K, V>
where
    K: Ord + Serialize + for<'de> Deserialize<'de> + Clone,
    V: Serialize + for<'de> Deserialize<'de> + Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> BPlusTree<K, V>
where
    K: Ord + Serialize + for<'de> Deserialize<'de> + Clone,
    V: Serialize + for<'de> Deserialize<'de> + Clone,
{
    pub const fn new() -> Self {
        let (inner_order, leaf_order) = calc_order::<K>();
        Self {
            root: BPlusTreeNode::<K, V>::new(true),
            inner_order,
            leaf_order,
            dirty: false,
        }
    }

    pub fn insert(&mut self, key: K, value: V) {
        self.dirty = true;
        if self.root.keys.is_empty() {
            self.root.keys.push(key);
            self.root.values.push(value);
            return;
        }

        if let Some(node) = self.root.insert(key, value, self.inner_order, self.leaf_order) {
            let child_key_opt = if node.is_leaf {
                node.keys.first()
            } else {
                BPlusTreeNode::<K, V>::find_leaf_entry(&node)
            };

            if let Some(child_key) = child_key_opt {
                let mut new_root = BPlusTreeNode::<K, V>::new(false);
                new_root.keys.push(child_key.clone());
                new_root.children.push(std::mem::replace(&mut self.root, BPlusTreeNode::new(true)));
                new_root.children.push(node);

                self.root = new_root;
            } else {
                error!("Failed to insert child key");
            }
        }
    }

    // pub fn count(&self) -> usize {
    //     self.root.count()
    // }

    pub fn query(&self, key: &K) -> Option<&V> {
        self.root.query(key)
    }

    pub fn store(&mut self, filepath: &Path) -> io::Result<u64> {
        if self.dirty {
            // Advisory lock to prevent concurrent COW updates
            let _lock = FileLock::try_lock(filepath)?;
            self.store_internal(filepath)
        } else {
            Ok(0)
        }
    }

    /// Internal store without locking, used for compaction or initial save.
    fn store_internal(&mut self, filepath: &Path) -> io::Result<u64> {
        let tempfile = NamedTempFile::new()?;
        let mut file = utils::file_writer(&tempfile);
        let mut buffer = vec![0u8; BLOCK_SIZE];

        // Write header block 0
        let mut header = [0u8; BLOCK_SIZE];
        header[0..4].copy_from_slice(MAGIC);
        header[4..8].copy_from_slice(&STORAGE_VERSION.to_le_bytes());
        // Placeholder for root offset, will be updated after serialization
        header[8..16].copy_from_slice(&HEADER_SIZE.to_le_bytes()); 
        file.write_all(&header)?;

        // Use breadth-first serialization for better disk locality
        match self.root.serialize_breadth_first(&mut file, &mut buffer, HEADER_SIZE) {
            Ok(result) => {
                file.flush()?;
                drop(file);
                if let Err(err) = utils::rename_or_copy(tempfile.path(), filepath, false) {
                    return Err(str_to_io_error(&format!("Temp file rename/copy did not work {} {err}", tempfile.path().to_string_lossy())));
                }
                self.dirty = false;
                Ok(result)
            }
            Err(err) => Err(err),
        }
    }

    pub fn load(filepath: &Path) -> io::Result<Self> {
        let mut file = is_file_valid(File::open(filepath)?)?;

        // Verify Header
        let mut header = [0u8; 16];
        file.read_exact(&mut header)?;
        if &header[0..4] != MAGIC {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid magic number"));
        }
        let version = u32::from_le_bytes(header[4..8].try_into().map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid version slice"))?);
        if version != STORAGE_VERSION {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("Unsupported storage version: {version}")));
        }
        let root_offset = u64::from_le_bytes(header[8..16].try_into().map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid root offset slice"))?);

        let mut reader = utils::file_reader(file);
        let mut buffer = vec![0u8; BLOCK_SIZE];
        // Start after header block
        let (root, _) = BPlusTreeNode::<K, V>::deserialize_from_block(&mut reader, &mut buffer, root_offset, true)?;

        let (inner_order, leaf_order) = calc_order::<K>();
        Ok(Self {
            root,
            inner_order,
            leaf_order,
            dirty: false,
        })
    }

    /// Find the largest key <= `key` in the in-memory tree and return references to (key, value).
    pub fn find_le(&self, key: &K) -> Option<(&K, &V)> {
        // empty tree
        if self.root.keys.is_empty() && self.root.is_leaf && self.root.values.is_empty() {
            return None;
        }
        self.root.find_le(key)
    }

    pub fn traverse<F>(&self, mut visit: F)
    where
        F: FnMut(&Vec<K>, &Vec<V>),
    {
        self.root.traverse(&mut visit);
    }
}

fn query_tree<K, V, R: Read + Seek>(
    file: &mut R, 
    buffer: &mut [u8],
    cache: &mut IndexMap<u64, Vec<u8>>,
    key: &K, 
    start_offset: u64
) -> Option<V>
where
    K: Ord + Serialize + for<'de> Deserialize<'de> + Clone,
    V: Serialize + for<'de> Deserialize<'de> + Clone,
{
    let mut offset = start_offset;
    loop {
        // Try Cache First
        let (node, pointers) = if let Some(cached_block) = cache.get_mut(&offset) {
            // Move to end (LRU)
            let data = cached_block.clone();
            // We use a fresh buffer for the deserializer to avoid borrowing conflicts
            let temp_buffer = data;
            let res = BPlusTreeNode::<K, V>::deserialize_from_block_slice(&temp_buffer, file, false).ok()?;
            
            // Re-insert to mark as MRU
            if let Some(removed) = cache.shift_remove(&offset) {
                cache.insert(offset, removed);
            }
            res
        } else {
            // Disk Read
            match BPlusTreeNode::<K, V>::deserialize_from_block(file, buffer, offset, false) {
                Ok((node, pointers)) => {
                    // Update Cache
                    if cache.len() >= 1024 { // Cap at ~4MB of blocks
                        cache.shift_remove_index(0);
                    }
                    cache.insert(offset, buffer.to_owned());
                    (node, pointers)
                }
                Err(err) => {
                    error!("Failed to read id tree from file {err}");
                    return None;
                }
            }
        };

        if node.is_leaf {
            return match node.keys.binary_search(key) {
                Ok(idx) => {
                    match node.value_info.get(idx) {
                        Some(&(val_offset, val_len)) => BPlusTreeNode::<K, V>::load_value_with_len(file, val_offset, val_len).ok(),
                        None => None,
                    }
                }
                Err(_) => None,
            };
        }
        
        let child_idx = get_entry_index_upper_bound::<K>(&node.keys, key);
        if let Some(child_offsets) = pointers {
            if let Some(child_offset) = child_offsets.get(child_idx) {
                offset = *child_offset;
            } else {
                return None;
            }
        } else {
            return None;
        }
    }
}

fn query_tree_le<K, V, R: Read + Seek>(
    file: &mut R, 
    buffer: &mut [u8],
    cache: &mut IndexMap<u64, Vec<u8>>,
    key: &K, 
    start_offset: u64
) -> Option<V>
where
    K: Ord + Serialize + for<'de> Deserialize<'de> + Clone,
    V: Serialize + for<'de> Deserialize<'de> + Clone,
{
    let mut offset = start_offset;
    loop {
        let (node, pointers) = if let Some(cached_block) = cache.get_mut(&offset) {
            let temp_buffer = cached_block.clone();
            let res = BPlusTreeNode::<K, V>::deserialize_from_block_slice(&temp_buffer, file, false).ok()?;
            if let Some(removed) = cache.shift_remove(&offset) {
                cache.insert(offset, removed);
            }
            res
        } else {
            match BPlusTreeNode::<K, V>::deserialize_from_block(file, buffer, offset, false) {
                Ok((node, pointers)) => {
                    if cache.len() >= 1024 {
                        cache.shift_remove_index(0);
                    }
                    cache.insert(offset, buffer.to_owned());
                    (node, pointers)
                }
                Err(err) => {
                    error!("Failed to read id tree from file {err}");
                    return None;
                }
            }
        };

        if node.is_leaf {
            let idx = get_entry_index_upper_bound::<K>(&node.keys, key);
            if idx == 0 {
                return None;
            }
            return match node.value_info.get(idx - 1) {
                Some(&(val_offset, val_len)) => BPlusTreeNode::<K, V>::load_value_with_len(file, val_offset, val_len).ok(),
                None => None,
            };
        }
        
        let child_idx = get_entry_index_upper_bound::<K>(&node.keys, key);
        if let Some(child_offsets) = pointers {
            if let Some(child_offset) = child_offsets.get(child_idx) {
                offset = *child_offset;
            } else if let Some(last) = child_offsets.last() {
                    offset = *last;
            } else {
               return None;
            }
        } else {
            return None;
        }
    }
}

///
/// `BPlusTreeQuery` can be used to query the `BPlusTree` on-disk.
/// If you intend to do frequent queries then use `BPlusTree` instead which loads the tree into memory.
///
pub struct BPlusTreeQuery<K, V> {
    file: BufReader<File>,
    buffer: Vec<u8>,
    cache: IndexMap<u64, Vec<u8>>,
    root_offset: u64,
    _marker_k: PhantomData<K>,
    _marker_v: PhantomData<V>,
}

impl<K, V> BPlusTreeQuery<K, V>
where
    K: Ord + Serialize + for<'de> Deserialize<'de> + Clone,
    V: Serialize + for<'de> Deserialize<'de> + Clone,
{
    pub fn try_from_file(file: File) -> io::Result<Self> {
        let mut file = is_file_valid(file)?;
        
        // Verify Header
        let mut header = [0u8; 16];
        file.seek(SeekFrom::Start(0))?;
        file.read_exact(&mut header)?;
        if &header[0..4] != MAGIC {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid magic number"));
        }
        let version = u32::from_le_bytes(header[4..8].try_into().map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid version slice"))?);
        if version != STORAGE_VERSION {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("Unsupported storage version: {version}")));
        }
        let root_offset = u64::from_le_bytes(header[8..16].try_into().map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid root offset slice"))?);

        Ok(Self {
            file: utils::file_reader(file),
            buffer: vec![0u8; BLOCK_SIZE],
            cache: IndexMap::with_capacity(1024),
            root_offset,
            _marker_k: PhantomData,
            _marker_v: PhantomData,
        })
    }


    pub fn try_new(filepath: &Path) -> io::Result<Self> {
        let file = File::open(filepath)?;
        Self::try_from_file(file)
    }

    pub fn query(&mut self, key: &K) -> Option<V> {
        query_tree(&mut self.file, &mut self.buffer, &mut self.cache, key, self.root_offset)
    }

    pub fn query_le(&mut self, key: &K) -> Option<V> {
        query_tree_le(&mut self.file, &mut self.buffer, &mut self.cache, key, self.root_offset)
    }

    /// Provides a disk-backed iterator that traverses the entire tree in order.
    pub fn iter(&mut self) -> BPlusTreeDiskIterator<'_, K, V> {
        BPlusTreeDiskIterator::new(self)
    }

    /// Traverses the tree and calls the provided closure for each leaf's keys and values.
    pub fn traverse<F>(&mut self, mut f: F) -> io::Result<()>
    where
        F: FnMut(&[K], &[V]),
    {
        let mut it = self.iter();
        while !it.is_empty() {
            if let Some((keys, values)) = it.next_leaf()? {
                f(&keys, &values);
            }
        }
        Ok(())
    }

    /// Provides an owned disk-backed iterator.
    pub fn disk_iter(self) -> BPlusTreeDiskIteratorOwned<K, V> {
        BPlusTreeDiskIteratorOwned::new(self)
    }
}

pub struct BPlusTreeDiskIteratorOwned<K, V> {
    query: BPlusTreeQuery<K, V>,
    stack: Vec<(u64, usize)>,
    leaf_keys: Vec<K>,
    leaf_values: Vec<V>,
    leaf_idx: usize,
}

impl<K, V> BPlusTreeDiskIteratorOwned<K, V>
where
    K: Ord + Serialize + for<'de> Deserialize<'de> + Clone,
    V: Serialize + for<'de> Deserialize<'de> + Clone,
{
    fn new(query: BPlusTreeQuery<K, V>) -> Self {
        let root_offset = query.root_offset;
        Self {
            query,
            stack: vec![(root_offset, 0)],
            leaf_keys: Vec::new(),
            leaf_values: Vec::new(),
            leaf_idx: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.stack.is_empty() && self.leaf_idx >= self.leaf_keys.len()
    }

    fn next_leaf(&mut self) -> io::Result<Option<(Vec<K>, Vec<V>)>> {
        loop {
            let Some((offset, child_idx)) = self.stack.pop() else { return Ok(None) };

            let (node, pointers) = BPlusTreeNode::<K, V>::deserialize_from_block(
                &mut self.query.file,
                &mut self.query.buffer,
                offset,
                false,
            )?;

            if node.is_leaf {
                let mut vals = Vec::with_capacity(node.value_info.len());
                for &(val_off, val_len) in &node.value_info {
                    let v = BPlusTreeNode::<K, V>::load_value_with_len(&mut self.query.file, val_off, val_len)?;
                    vals.push(v);
                }
                return Ok(Some((node.keys, vals)));
            } else if let Some(pters) = pointers {
                if child_idx < pters.len() {
                    let next_ptr = pters[child_idx];
                    self.stack.push((offset, child_idx + 1));
                    self.stack.push((next_ptr, 0));
                }
            }
        }
    }
}

impl<K, V> Iterator for BPlusTreeDiskIteratorOwned<K, V>
where
    K: Ord + Serialize + for<'de> Deserialize<'de> + Clone,
    V: Serialize + for<'de> Deserialize<'de> + Clone,
{
    type Item = (K, V);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.leaf_idx < self.leaf_keys.len() {
                let key = self.leaf_keys[self.leaf_idx].clone();
                let value = self.leaf_values[self.leaf_idx].clone();
                self.leaf_idx += 1;
                return Some((key, value));
            }

            match self.next_leaf() {
                Ok(Some((keys, values))) => {
                    self.leaf_keys = keys;
                    self.leaf_values = values;
                    self.leaf_idx = 0;
                }
                _ => return None,
            }
        }
    }
}

pub struct BPlusTreeDiskIterator<'a, K, V> {
    query: &'a mut BPlusTreeQuery<K, V>,
    stack: Vec<(u64, usize)>, // (node_offset, next_child_index)
    leaf_keys: Vec<K>,
    leaf_values: Vec<V>,
    leaf_idx: usize,
}

impl<'a, K, V> BPlusTreeDiskIterator<'a, K, V>
where
    K: Ord + Serialize + for<'de> Deserialize<'de> + Clone,
    V: Serialize + for<'de> Deserialize<'de> + Clone,
{
    fn new(query: &'a mut BPlusTreeQuery<K, V>) -> Self {
        let root_offset = query.root_offset;
        Self {
            query,
            stack: vec![(root_offset, 0)],
            leaf_keys: Vec::new(),
            leaf_values: Vec::new(),
            leaf_idx: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.stack.is_empty() && self.leaf_idx >= self.leaf_keys.len()
    }

    /// Internal method to load the next leaf and return its content.
    fn next_leaf(&mut self) -> io::Result<Option<(Vec<K>, Vec<V>)>> {
        loop {
            let Some((offset, child_idx)) = self.stack.pop() else { return Ok(None) };

            let (node, pointers) = BPlusTreeNode::<K, V>::deserialize_from_block(
                &mut self.query.file,
                &mut self.query.buffer,
                offset,
                false,
            )?;

            if node.is_leaf {
                let mut vals = Vec::with_capacity(node.value_info.len());
                for &(val_off, val_len) in &node.value_info {
                    let v = BPlusTreeNode::<K, V>::load_value_with_len(&mut self.query.file, val_off, val_len)?;
                    vals.push(v);
                }
                return Ok(Some((node.keys, vals)));
            } else if let Some(pters) = pointers {
                if child_idx < pters.len() {
                    let next_ptr = pters[child_idx];
                    self.stack.push((offset, child_idx + 1));
                    self.stack.push((next_ptr, 0));
                }
            }
        }
    }
}

impl<K, V> Iterator for BPlusTreeDiskIterator<'_, K, V>
where
    K: Ord + Serialize + for<'de> Deserialize<'de> + Clone,
    V: Serialize + for<'de> Deserialize<'de> + Clone,
{
    type Item = (K, V);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.leaf_idx < self.leaf_keys.len() {
                let key = self.leaf_keys[self.leaf_idx].clone();
                let value = self.leaf_values[self.leaf_idx].clone();
                self.leaf_idx += 1;
                return Some((key, value));
            }

            match self.next_leaf() {
                Ok(Some((keys, values))) => {
                    self.leaf_keys = keys;
                    self.leaf_values = values;
                    self.leaf_idx = 0;
                }
                _ => return None,
            }
        }
    }
}

pub struct BPlusTreeUpdate<K, V> {
    file: File,
    read_buffer: Vec<u8>,
    write_buffer: Vec<u8>,
    cache: IndexMap<u64, Vec<u8>>,
    root_offset: u64,
    #[allow(dead_code)]
    lock: FileLock,
    _marker_k: PhantomData<K>,
    _marker_v: PhantomData<V>,
}

struct FileLock {
    path: PathBuf,
}

impl FileLock {
    fn try_lock(filepath: &Path) -> io::Result<Self> {
        let lock_path = PathBuf::from(format!("{}.lock", filepath.to_str().unwrap_or("tree")));
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)?;
        Ok(Self { path: lock_path })
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl<K, V> BPlusTreeUpdate<K, V>
where
    K: Ord + Serialize + for<'de> Deserialize<'de> + Clone,
    V: Serialize + for<'de> Deserialize<'de> + Clone,
{
    pub fn try_new(filepath: &Path) -> io::Result<Self> {
        if !filepath.exists() {
            return Err(io::Error::new(io::ErrorKind::NotFound, format!("File not found {}", filepath.to_str().unwrap_or("?"))));
        }
        // Acquire lock first
        let lock = FileLock::try_lock(filepath)?;
        
        let mut file = is_file_valid(utils::open_read_write_file(filepath)?)?;
        
        // Verify Header
        let mut header = [0u8; 16];
        file.read_exact(&mut header)?;
        if &header[0..4] != MAGIC {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid magic number"));
        }
        let version = u32::from_le_bytes(header[4..8].try_into().map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid version slice"))?);
        if version != STORAGE_VERSION {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("Unsupported storage version: {version}")));
        }
        let root_offset = u64::from_le_bytes(header[8..16].try_into().map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid root offset slice"))?);

        Ok(Self {
            file,
            read_buffer: vec![0u8; BLOCK_SIZE],
            write_buffer: vec![0u8; BLOCK_SIZE],
            cache: IndexMap::with_capacity(1024),
            root_offset,
            lock,
            _marker_k: PhantomData,
            _marker_v: PhantomData,
        })
    }

    pub fn query(&mut self, key: &K) -> Option<V> {
        let mut reader = utils::file_reader(&mut self.file);
        query_tree(&mut reader, &mut self.read_buffer, &mut self.cache, key, self.root_offset)
    }

    pub fn query_le(&mut self, key: &K) -> Option<V> {
        let mut reader = utils::file_reader(&mut self.file);
        query_tree_le(&mut reader, &mut self.read_buffer, &mut self.cache, key, self.root_offset)
    }

    fn update_recursive(
        &mut self,
        offset: u64,
        key: &K,
        value: &V,
    ) -> io::Result<u64> {
        let mut reader = utils::file_reader(&mut self.file);
        let (mut node, pointers) = BPlusTreeNode::<K, V>::deserialize_from_block(&mut reader, &mut self.read_buffer, offset, false)?;

        if node.is_leaf {
            match node.keys.binary_search(key) {
                Ok(idx) => {
                    // COW: Write new value block at end of file
                    let value_bytes = binary_serialize(value)?;
                    let value_len = u32::try_from(value_bytes.len()).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Value too large for u32"))?;
                    
                    self.file.seek(SeekFrom::End(0))?;
                    let val_offset = self.file.stream_position()?;
                    
                    let mut pos = 0;
                    while pos < value_bytes.len() {
                        let chunk = std::cmp::min(BLOCK_SIZE, value_bytes.len() - pos);
                        self.write_buffer[..chunk].copy_from_slice(&value_bytes[pos..pos + chunk]);
                        if chunk < BLOCK_SIZE {
                            self.write_buffer[chunk..BLOCK_SIZE].fill(0u8);
                        }
                        self.file.write_all(&self.write_buffer)?;
                        pos += chunk;
                    }

                    // Update leaf metadata
                    node.value_info[idx] = (val_offset, value_len);

                    // Write new leaf node at end of file
                    self.file.seek(SeekFrom::End(0))?;
                    let new_leaf_offset = self.file.stream_position()?;
                    node.serialize_to_block(&mut self.file, &mut self.write_buffer, new_leaf_offset)?;
                    Ok(new_leaf_offset)
                }
                Err(_) => Err(io::Error::new(io::ErrorKind::NotFound, "Entry not found")),
            }
        } else {
            let child_idx = get_entry_index_upper_bound::<K>(&node.keys, key);
            if let Some(mut pters) = pointers {
                if let Some(&child_offset) = pters.get(child_idx) {
                    // Recurse to get new child offset
                    let new_child_offset = self.update_recursive(child_offset, key, value)?;
                    
                    // Update current node's pointers
                    pters[child_idx] = new_child_offset;
                    
                    // COW: Write new internal node at end of file
                    self.file.seek(SeekFrom::End(0))?;
                    let new_node_offset = self.file.stream_position()?;
                    
                    // Use the robust helper to serialize the internal node with updated child pointers
                    node.serialize_internal_with_offsets(&mut self.file, &mut self.write_buffer, new_node_offset, &pters)?;
                    Ok(new_node_offset)
                } else {
                    Err(io::Error::new(io::ErrorKind::NotFound, "Child pointer not found in internal node"))
                }
            } else {
                Err(io::Error::new(io::ErrorKind::InvalidData, "Internal node missing pointers invariant violation"))
            }
        }
    }

    pub fn update(&mut self, key: &K, value: V) -> io::Result<u64> {
        let new_root_offset = self.update_recursive(self.root_offset, key, &value)?;
        
        // Atomic Header Swap
        self.file.seek(SeekFrom::Start(ROOT_OFFSET_POS))?;
        self.file.write_all(&new_root_offset.to_le_bytes())?;
        self.file.flush()?;
        self.file.sync_all()?;
        
        self.root_offset = new_root_offset;
        Ok(new_root_offset)
    }

    /// Garbage Collection: Compacts the file by rewriting only live blocks sequentially.
    pub fn compact(&mut self, filepath: &Path) -> io::Result<()> {
        // 1. Reload the current tree fully from the live root
        let mut tree = BPlusTree::<K, V>::load(filepath)?;
        // 2. Setting dirty=true forces store_internal() to rewrite the file sequentially.
        // We use store_internal because we already hold the lock in self._lock.
        tree.dirty = true;
        let new_root_offset = tree.store_internal(filepath)?;
        self.root_offset = new_root_offset;
        Ok(())
    }
}

pub struct BPlusTreeIterator<'a, K, V> {
    stack: Vec<&'a BPlusTreeNode<K, V>>,
    current_keys: Option<&'a [K]>,
    current_values: Option<&'a [V]>,
    index: usize,
}

impl<'a, K, V> BPlusTreeIterator<'a, K, V>
where
    K: Ord + Serialize + for<'de> Deserialize<'de> + Clone,
    V: Serialize + for<'de> Deserialize<'de> + Clone,
{
    pub fn new(tree: &'a BPlusTree<K, V>) -> Self {
        let stack = vec![&tree.root];
        Self {
            stack,
            current_keys: None,
            current_values: None,
            index: 0,
        }
    }
}

impl<'a, K, V> Iterator for BPlusTreeIterator<'a, K, V>
where
    K: Ord + Serialize + for<'de> Deserialize<'de> + Clone,
    V: Serialize + for<'de> Deserialize<'de> + Clone,
{
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            // Try to return next item from current leaf
            if let Some(keys) = self.current_keys {
                if let Some(values) = self.current_values {
                    if self.index < keys.len() {
                        let key = &keys[self.index];
                        let value = &values[self.index];
                        self.index += 1;
                        return Some((key, value));
                    }
                }
            }

            // Current leaf exhausted, find next leaf
            loop {
                let node = self.stack.pop()?;
                
                if node.is_leaf {
                    // Found a leaf node
                    self.current_keys = Some(&node.keys);
                    self.current_values = Some(&node.values);
                    self.index = 0;
                    break; // Exit inner loop to process this leaf
                }
                // Push children in reverse order to maintain left-to-right traversal
                for child in node.children.iter().rev() {
                    self.stack.push(child);
                }
            }
        }
    }
}

impl<K, V> BPlusTree<K, V>
where
    K: Ord + Serialize + for<'de> Deserialize<'de> + Clone,
    V: Serialize + for<'de> Deserialize<'de> + Clone,
{
    pub fn iter(&self) -> BPlusTreeIterator<'_, K, V> {
        BPlusTreeIterator::new(self)
    }
}

impl<'a, K, V> IntoIterator for &'a BPlusTree<K, V>
where
    K: Ord + Serialize + for<'de> Deserialize<'de> + Clone,
    V: Serialize + for<'de> Deserialize<'de> + Clone,
{
    type Item = (&'a K, &'a V);
    type IntoIter = BPlusTreeIterator<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::io;

    use crate::repository::bplustree::{BPlusTree, BPlusTreeQuery, BPlusTreeUpdate};
    use serde::{Deserialize, Serialize};
    use shared::utils::generate_random_string;

    // Example usage with a simple struct
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    struct Record {
        id: u32,
        data: String,
    }


    #[test]
    fn insert_test() -> io::Result<()> {
        let test_size = 500;
        let content = generate_random_string(1024);
        let mut tree = BPlusTree::<u32, Record>::new();
        for i in 0u32..=test_size {
            tree.insert(i, Record {
                id: i,
                data: format!("{content} {i}"),
            });
        }

        // // Traverse the tree
        // tree.traverse(|node| {
        //     println!("Node: {:?}", node);
        // });

        let tempdir = tempfile::tempdir()?;
        let filepath = tempdir.path().join("tree_insert_test.bin");
        // Serialize the tree to a file
        tree.store(&filepath)?;
        // Deserialize the tree from the file
        tree = BPlusTree::<u32, Record>::load(&filepath)?;

        // Query the tree
        for i in 0u32..=test_size {
            let found = tree.query(&i);
            assert!(found.is_some(), "{content} {i} not found");
            assert!(found.unwrap().eq(&Record {
                id: i,
                data: format!("{content} {i}"),
            }), "{content} {i} not found");
        }

        let mut tree_query: BPlusTreeQuery<u32, Record> = BPlusTreeQuery::try_new(&filepath)?;
        for i in 0u32..=test_size {
            let found = tree_query.query(&i);
            assert!(found.is_some(), "{content} {i} not found");
            let entry = found.unwrap();
            assert!(entry.eq(&Record {
                id: i,
                data: format!("{content} {i}"),
            }), "{content} {i} not found");
        }

        let mut tree_update: BPlusTreeUpdate<u32, Record> = BPlusTreeUpdate::try_new(&filepath)?;

        for i in 0u32..=test_size {
            if let Some(record) = tree_update.query(&i) {
                let new_record = Record {
                    id: record.id,
                    data: format!("{content} {}", record.id + 9000),
                };
                tree_update.update(&i, new_record)?;
            } else {
                panic!("{content} {i} not found");
            }
        }

        // Verify with Query
        let mut tree_query: BPlusTreeQuery<u32, Record> = BPlusTreeQuery::try_new(&filepath)?;

        for i in 0u32..=test_size {
            let found = tree_query.query(&i);
            assert!(found.is_some(), "{content} {i} not found");
            let entry = found.unwrap();
            let expected = Record {
                id: i,
                data: format!("{content} {}", i + 9000),
            };
            assert!(entry.eq(&expected), "Entry not equal {entry:?} != {expected:?}");
        }

        Ok(())
    }


    #[test]
    fn insert_duplicate_test() {
        let content = "Entry";
        let mut tree = BPlusTree::<u32, Record>::new();
        for i in 0u32..=500 {
            tree.insert(i, Record {
                id: i,
                data: format!("{content} {i}"),
            });
        }
        for i in 0u32..=500 {
            tree.insert(i, Record {
                id: i,
                data: format!("{content} {}", i + 1),
            });
        }

        tree.traverse(|keys, values| {
            keys.iter().zip(values.iter()).for_each(|(k, v)| {
                assert!(format!("{content} {}", k + 1).eq(&v.data), "Wrong entry");
            });
        });
    }

    #[test]
    fn iterator_test() -> io::Result<()> {
        let mut tree = BPlusTree::<u32, Record>::new();
        let mut entry_set = HashSet::new();
        for i in 0u32..=500 {
            tree.insert(i, Record {
                id: i,
                data: format!("Entry {i}"),
            });
            entry_set.insert(i);
        }
        let tempdir = tempfile::tempdir()?;
        let filepath = tempdir.path().join("tree_iterator_test.bin");
        // Serialize the tree to a file
        tree.store(&filepath)?;
        let tree: BPlusTree<u32, Record> = BPlusTree::load(&filepath)?;

        // Traverse the tree
        for (key, value) in &tree {
            assert!(format!("Entry {key}").eq(&value.data), "Wrong entry");
            entry_set.remove(key);
        }
        assert!(entry_set.is_empty());
        Ok(())
    }

    #[test]
    fn persistence_update_and_iterate_test() -> io::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let filepath = tempdir.path().join("tree_update_iter.bin");
        let content = "InitialContent";
        let mut tree = BPlusTree::<u32, Record>::new();
        
        // Initial store
        for i in 0u32..100 {
            tree.insert(i, Record { id: i, data: format!("{content} {i}") });
        }
        tree.store(&filepath)?;

        // Update via BPlusTreeUpdate
        let mut tree_update = BPlusTreeUpdate::<u32, Record>::try_new(&filepath)?;
        for i in 0u32..100 {
            if i % 2 == 0 {
                tree_update.update(&i, Record { id: i, data: format!("UpdatedContent {i}") })?;
            }
        }

        // Reload and Verify via Query
        let mut tree_query: BPlusTreeQuery<u32, Record> = BPlusTreeQuery::try_new(&filepath)?;
        for i in 0u32..100 {
            let val = tree_query.query(&i).expect("Should find key");
            if i % 2 == 0 {
                assert_eq!(val.data, format!("UpdatedContent {i}"));
            } else {
                assert_eq!(val.data, format!("{content} {i}"));
            }
        }

        // Reload and Verify via Iterator
        let reloaded_tree = BPlusTree::<u32, Record>::load(&filepath)?;
        let mut count = 0;
        for (key, value) in &reloaded_tree {
            if *key % 2 == 0 {
                assert_eq!(value.data, format!("UpdatedContent {key}"), "Iterator returned wrong value for updated key {key}");
            } else {
                assert_eq!(value.data, format!("{content} {key}"), "Iterator returned wrong value for original key {key}");
            }
            count += 1;
        }
        assert_eq!(count, 100, "Iterator did not yield all entries");

        Ok(())
    }

    #[test]
    fn update_inplace_size_test() -> io::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let filepath = tempdir.path().join("tree_size_test.bin");
        let mut tree = BPlusTree::<u32, Record>::new();
        
        // Use fixed size string for predictable sizing
        let padding = "x".repeat(100);
        for i in 0u32..10 {
            tree.insert(i, Record { id: i, data: padding.clone() });
        }
        tree.store(&filepath)?;
        
        let initial_size = std::fs::metadata(&filepath)?.len();
        
        // Update with same size data
        let mut tree_update = BPlusTreeUpdate::<u32, Record>::try_new(&filepath)?;
        let same_size_padding = "y".repeat(100);
        for i in 0u32..10 {
            tree_update.update(&i, Record { id: i, data: same_size_padding.clone() })?;
        }
        
        let size_after_same_update = std::fs::metadata(&filepath)?.len();
        assert!(size_after_same_update > initial_size, "File should grow during COW same-size update");
        
        // Reload and verify
        let mut tree_query = BPlusTreeQuery::<u32, Record>::try_new(&filepath)?;
        for i in 0u32..10 {
            let val = tree_query.query(&i).expect("Should find key");
            assert_eq!(val.data, same_size_padding);
        }

        // Update with smaller size data
        let smaller_padding = "z".repeat(50);
        for i in 0u32..10 {
            tree_update.update(&i, Record { id: i, data: smaller_padding.clone() })?;
        }
        
        // Update with larger size data (force append)
        let larger_padding = "w".repeat(5000); 
        for i in 0u32..1 {
            tree_update.update(&i, Record { id: i, data: larger_padding.clone() })?;
        }
        
        let size_before_compact = std::fs::metadata(&filepath)?.len();
        
        // Final verification: Compact should shrink the file
        tree_update.compact(&filepath)?;
        let size_after_compact = std::fs::metadata(&filepath)?.len();
        assert!(size_after_compact < size_before_compact, "Compaction should reduce file size");
        
        // Final data check after compact
        let mut final_query = BPlusTreeQuery::<u32, Record>::try_new(&filepath)?;
        assert_eq!(final_query.query(&0).unwrap().data, larger_padding);
        for i in 1u32..10 {
            assert_eq!(final_query.query(&i).unwrap().data, smaller_padding);
        }
        
        Ok(())
    }

    #[test]
    fn cow_deep_tree_compaction_test() -> io::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let filepath = tempdir.path().join("deep_tree.idx");
        
        let test_size = 500u32; // Enough to force multiple levels
        let mut tree = BPlusTree::new();
        for i in 0..test_size {
            tree.insert(i, Record { id: i, data: format!("Content {i}") });
        }
        tree.store(&filepath)?;

        let mut tree_update = BPlusTreeUpdate::<u32, Record>::try_new(&filepath)?;
        
        // 1. Initial Queries
        for i in (0..test_size).step_by(50) {
            let val = tree_update.query(&i).expect("Should find initial key");
            assert_eq!(val.data, format!("Content {i}"));
        }

        // 2. Multiple Updates (COW)
        for i in (0..test_size).step_by(10) {
            tree_update.update(&i, Record { id: i, data: format!("UpdatedContent {i}") })?;
        }

        // 3. Verify Query Integrity (Must return NEW values)
        for i in (0..test_size).step_by(10) {
            let val = tree_update.query(&i).expect("Should find updated key");
            assert_eq!(val.data, format!("UpdatedContent {i}"));
        }

        // 4. Verify Query Integrity for non-updated keys (Must return OLD values)
        for i in (1..test_size).step_by(11) {
            if i % 10 == 0 { continue; } // skip updated ones
            let val = tree_update.query(&i).expect("Should find original key");
            assert_eq!(val.data, format!("Content {i}"));
        }

        let size_before_compact = std::fs::metadata(&filepath)?.len();

        // 5. GC / Compaction
        tree_update.compact(&filepath)?;
        
        let size_after_compact = std::fs::metadata(&filepath)?.len();
        assert!(size_after_compact < size_before_compact, "Compaction should reclaimed space from COW path copies");

        // 6. Final verification after GC
        let mut final_query = BPlusTreeQuery::<u32, Record>::try_new(&filepath)?;
        for i in (0..test_size).step_by(10) {
            let val = final_query.query(&i).expect("Should find updated key after GC");
            assert_eq!(val.data, format!("UpdatedContent {i}"));
        }
        
        Ok(())
    }

    #[test]
    fn query_le_cow_test() -> io::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let filepath = tempdir.path().join("le_cow.idx");
        
        // 1. Build initial tree with gaps
        let mut tree = BPlusTree::new();
        for i in (0..100u32).step_by(10) {
            tree.insert(i, Record { id: i, data: format!("Content {i}") });
        }
        tree.store(&filepath)?;

        let mut tree_update = BPlusTreeUpdate::<u32, Record>::try_new(&filepath)?;
        
        // Initial LE check
        assert_eq!(tree_update.query_le(&15).unwrap().id, 10);
        assert_eq!(tree_update.query_le(&5).unwrap().id, 0);

        // 2. COW Update
        tree_update.update(&10, Record { id: 10, data: "NewVal".to_string() })?;
        
        // 3. Verify LE returns the LATEST value
        let val = tree_update.query_le(&15).expect("Should find LE key after COW update");
        assert_eq!(val.id, 10);
        assert_eq!(val.data, "NewVal");
        
        Ok(())
    }

    #[test]
    fn disk_iterator_test() -> io::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let filepath = tempdir.path().join("disk_it.idx");
        
        let mut tree = BPlusTree::new();
        let test_size = 500u32;
        for i in 0..test_size {
            tree.insert(i, Record { id: i, data: format!("Value {i}") });
        }
        tree.store(&filepath)?;

        let mut query = BPlusTreeQuery::<u32, Record>::try_new(&filepath)?;
        
        // 1. Test Iterator
        let mut count = 0;
        for (k, v) in query.iter() {
            assert_eq!(k, count);
            assert_eq!(v.data, format!("Value {count}"));
            count += 1;
        }
        assert_eq!(count, test_size);

        // 2. Test Traverse helper
        let mut traverse_count = 0;
        query.traverse(|keys, values| {
            for (k, v) in keys.iter().zip(values.iter()) {
                assert_eq!(*k, traverse_count);
                assert_eq!(v.data, format!("Value {traverse_count}"));
                traverse_count += 1;
            }
        })?;
        assert_eq!(traverse_count, test_size);
        
        Ok(())
    }
}
