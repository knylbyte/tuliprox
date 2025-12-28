use crate::utils;
use crate::utils::{binary_deserialize, binary_serialize};
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

const BLOCK_SIZE: usize = 4096;
const LEN_SIZE: usize = 4;
const FLAG_SIZE: usize = 1;
const MAGIC: &[u8; 4] = b"BTRE";
const STORAGE_VERSION: u32 = 1;
const HEADER_SIZE: u64 = BLOCK_SIZE as u64;
const ROOT_OFFSET_POS: u64 = 8;

const POINTER_SIZE: usize = 8;
const INFO_SIZE: usize = 12; // (u64, u32)

// MessagePack overhead estimation
const MSGPACK_OVERHEAD_PER_ENTRY: usize = 16;


// Value packing configuration
const SMALL_VALUE_THRESHOLD: usize = 256;  // Pack values <= 256 bytes
const PACK_BLOCK_HEADER_SIZE: usize = 4;   // u32 for value count
const PACK_VALUE_HEADER_SIZE: usize = 4;   // u32 for each value length

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


/// Represents how a value is stored on disk
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum ValueStorageMode {
    /// Multiple small values packed in one block
    /// (`block_offset`, `value_index_in_block`)
    Packed(u64, u16),

    /// Single value in dedicated block(s)
    /// (`block_offset`)
    Single(u64),
}

/// Extended value info that includes storage mode and length
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ValueInfo {
    mode: ValueStorageMode,
    length: u32,
}


#[derive(Debug, Clone)]
struct BPlusTreeNode<K, V> {
    keys: Vec<K>,
    children: Vec<BPlusTreeNode<K, V>>,
    is_leaf: bool,
    value_info: Vec<ValueInfo>,
    values: Vec<V>, // only used in leaf nodes
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

    /// Write a packed value block to disk
    fn write_packed_block<W: Write + Seek>(
        file: &mut W,
        buffer: &mut [u8],
        offset: u64,
        values: &[(u16, &[u8])],
    ) -> io::Result<()> {
        file.seek(SeekFrom::Start(offset))?;

        // Write count
        let count = u32::try_from(values.len()).map_err(to_io_error)?;
        buffer[0..4].copy_from_slice(&count.to_le_bytes());
        let mut pos = 4;

        // Write each value: length + data
        for (_, value_bytes) in values {
            let len = u32::try_from(value_bytes.len()).map_err(to_io_error)?;
            buffer[pos..pos + 4].copy_from_slice(&len.to_le_bytes());
            pos += 4;
            buffer[pos..pos + value_bytes.len()].copy_from_slice(value_bytes);
            pos += value_bytes.len();
        }

        // Zero remaining space
        if pos < BLOCK_SIZE {
            buffer[pos..BLOCK_SIZE].fill(0u8);
        }

        file.write_all(&buffer[..BLOCK_SIZE])?;
        Ok(())
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
        buffer: &mut Vec<u8>,
        offset: u64,
    ) -> io::Result<u64> {
        let keys_encoded = binary_serialize(&self.keys)?;
        let keys_len = u32::try_from(keys_encoded.len()).map_err(to_io_error)?;
        
        if self.is_leaf {
             let info_encoded = binary_serialize(&self.value_info)?;
             let info_len = u32::try_from(info_encoded.len()).map_err(to_io_error)?;
             
             let content_size = FLAG_SIZE + LEN_SIZE + keys_encoded.len() + LEN_SIZE + info_encoded.len();
             let blocks = content_size.div_ceil(BLOCK_SIZE);
             
             file.seek(SeekFrom::Start(offset))?;
             
             let mut data = Vec::with_capacity(blocks * BLOCK_SIZE);
             data.push(1u8);
             data.extend_from_slice(&keys_len.to_le_bytes());
             data.extend_from_slice(&keys_encoded);
             data.extend_from_slice(&info_len.to_le_bytes());
             data.extend_from_slice(&info_encoded);
             
             let pad_len = (blocks * BLOCK_SIZE) - data.len();
             if pad_len > 0 {
                 data.extend(std::iter::repeat(0).take(pad_len));
             }
             
             file.write_all(&data)?;
             
             Ok(offset + (blocks as u64 * BLOCK_SIZE as u64))
        } else {
             let ptr_count = self.children.len();
             let ptr_encoded_size = 8 + 8 * ptr_count;
             
             let content_size = FLAG_SIZE + LEN_SIZE + keys_encoded.len() + LEN_SIZE + ptr_encoded_size;
             let blocks_needed = content_size.div_ceil(BLOCK_SIZE);
             
             let parent_start = offset;
             let mut current_offset = parent_start + (blocks_needed as u64 * BLOCK_SIZE as u64);
             
             let mut pointers = Vec::with_capacity(ptr_count);
             for child in &self.children {
                 pointers.push(current_offset);
                 current_offset = child.serialize_to_block(file, buffer, current_offset)?;
             }
             
             let pointers_encoded = binary_serialize(&pointers)?;
             let pointers_len = u32::try_from(pointers_encoded.len()).map_err(to_io_error)?;
             
             file.seek(SeekFrom::Start(parent_start))?;
             let mut data = Vec::with_capacity(blocks_needed * BLOCK_SIZE);
             data.push(0u8);
             data.extend_from_slice(&keys_len.to_le_bytes());
             data.extend_from_slice(&keys_encoded);
             data.extend_from_slice(&pointers_len.to_le_bytes());
             data.extend_from_slice(&pointers_encoded);
             
             let pad_len = (blocks_needed * BLOCK_SIZE) - data.len();
             if pad_len > 0 {
                 data.extend(std::iter::repeat(0).take(pad_len));
             }
             file.write_all(&data)?;
             
             Ok(current_offset)
        }
    }

    /// Serialize the tree in breadth-first order for better disk locality
    /// This improves query performance by keeping nodes at the same level contiguous
    #[allow(clippy::too_many_lines)]
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

        // Pass 2: Pack values and calculate offsets (Mutable)
        {
            let mut current_level_mut = vec![&mut *self];
            while !current_level_mut.is_empty() {
                let mut next_level_mut = Vec::new();
                for node in current_level_mut {
                    if node.is_leaf {
                        node.value_info.clear();

                        // Serialize all values first to determine sizes
                        let mut serialized_values: Vec<Vec<u8>> = Vec::new();
                        for value in &node.values {
                            let value_bytes = binary_serialize(value)?;
                            serialized_values.push(value_bytes);
                        }

                        // Pack small values together, keep large values separate
                        let mut current_pack_offset: Option<u64> = None;
                        let mut current_pack_values: Vec<Vec<u8>> = Vec::new();
                        let mut current_pack_size = PACK_BLOCK_HEADER_SIZE;

                        for value_bytes in serialized_values {
                            let size = value_bytes.len();

                            if size <= SMALL_VALUE_THRESHOLD {
                                let entry_size = PACK_VALUE_HEADER_SIZE + size;

                                if current_pack_size + entry_size <= BLOCK_SIZE {
                                    // Add to current pack
                                    if current_pack_offset.is_none() {
                                        current_pack_offset = Some(current_offset);
                                    }
                                    let pack_index = u16::try_from(current_pack_values.len()).map_err(to_io_error)?;
                                    current_pack_values.push(value_bytes);
                                    current_pack_size += entry_size;

                                    node.value_info.push(ValueInfo {
                                        mode: ValueStorageMode::Packed(current_pack_offset.unwrap(), pack_index),
                                        length: u32::try_from(size).map_err(to_io_error)?,
                                    });
                                } else {
                                    // Flush current pack and start new one
                                    if !current_pack_values.is_empty() {
                                        current_offset += BLOCK_SIZE as u64;
                                    }
                                    current_pack_offset = Some(current_offset);
                                    current_pack_values.clear();
                                    current_pack_values.push(value_bytes);
                                    current_pack_size = PACK_BLOCK_HEADER_SIZE + entry_size;

                                    node.value_info.push(ValueInfo {
                                        mode: ValueStorageMode::Packed(current_pack_offset.unwrap(), 0),
                                        length: u32::try_from(size).map_err(to_io_error)?,
                                    });
                                }
                            } else {
                                // Flush any pending pack
                                if !current_pack_values.is_empty() {
                                    current_offset += BLOCK_SIZE as u64;
                                    current_pack_values.clear();
                                    current_pack_offset = None;
                                    current_pack_size = PACK_BLOCK_HEADER_SIZE;
                                }

                                // Write as single value
                                node.value_info.push(ValueInfo {
                                    mode: ValueStorageMode::Single(current_offset),
                                    length: u32::try_from(size).map_err(to_io_error)?,
                                });

                                let blocks_needed = size.div_ceil(BLOCK_SIZE);
                                current_offset += (blocks_needed * BLOCK_SIZE) as u64;
                            }
                        }

                        // Flush final pack if any
                        if !current_pack_values.is_empty() {
                            current_offset += BLOCK_SIZE as u64;
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

        // Pass 4: Write all value blocks (packed and single) (Immutable)
        {
            let mut current_level_values = vec![&*self];
            while !current_level_values.is_empty() {
                let mut next_level = Vec::new();
                for node in current_level_values {
                    if node.is_leaf {
                        // Group values by their storage location
                        let mut pack_blocks: HashMap<u64, Vec<(u16, Vec<u8>)>> = HashMap::new();

                        for (value, info) in node.values.iter().zip(node.value_info.iter()) {
                            let value_bytes = binary_serialize(value)?;

                            match info.mode {
                                ValueStorageMode::Packed(block_offset, index) => {
                                    pack_blocks.entry(block_offset)
                                        .or_default()
                                        .push((index, value_bytes));
                                }
                                ValueStorageMode::Single(block_offset) => {
                                    // Write single value (existing logic)
                                    file.seek(SeekFrom::Start(block_offset))?;
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
                            }
                        }

                        // Write packed blocks
                        for (block_offset, mut values) in pack_blocks {
                            // Sort by index to ensure correct order
                            values.sort_by_key(|(idx, _)| *idx);

                            // Convert to slice references
                            let value_refs: Vec<(u16, &[u8])> = values.iter()
                                .map(|(idx, bytes)| (*idx, bytes.as_slice()))
                                .collect();

                            Self::write_packed_block(file, buffer, block_offset, &value_refs)?;
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
        buffer: &mut Vec<u8>,
        offset: u64,
        nested: bool,
    ) -> io::Result<(Self, Option<Vec<u64>>)> {
        file.seek(SeekFrom::Start(offset))?;
        
        let header_required = FLAG_SIZE + LEN_SIZE;
        if buffer.len() < header_required {
            buffer.resize(header_required, 0);
        }
        
        file.read_exact(&mut buffer[0..header_required])?;
        
        let is_leaf = buffer[0] != 0;
        let keys_len = u32_from_bytes(&buffer[FLAG_SIZE..FLAG_SIZE + LEN_SIZE])? as usize;
        
        let min_required = header_required + keys_len + LEN_SIZE;
        if buffer.len() < min_required {
            buffer.resize(min_required, 0);
        }
        
        file.read_exact(&mut buffer[header_required..min_required])?;
        
        let mut read_pos = FLAG_SIZE + LEN_SIZE;
        let keys: Vec<K> = binary_deserialize(&buffer[read_pos..read_pos + keys_len])?;
        read_pos += keys_len;
        
        let payload_len = u32_from_bytes(&buffer[read_pos..read_pos + LEN_SIZE])? as usize;
        read_pos += LEN_SIZE;
        
        let total_required = min_required + payload_len;
        if buffer.len() < total_required {
            buffer.resize(total_required, 0);
        }
        
        file.read_exact(&mut buffer[min_required..total_required])?;
        
        let (value_info, values, children, children_pointer) = if is_leaf {
             let info: Vec<ValueInfo> = binary_deserialize(&buffer[read_pos..read_pos + payload_len])?;
             let vals = if nested {
                 let mut v = Vec::with_capacity(info.len());
                 for i in &info {
                     v.push(Self::load_value_from_info(file, i)?);
                 }
                 v
             } else {
                 Vec::new()
             };
             (info, vals, Vec::new(), None)
        } else {
             let pointers: Vec<u64> = binary_deserialize(&buffer[read_pos..read_pos + payload_len])?;
             let nodes = if nested {
                 let mut n = Vec::with_capacity(pointers.len());
                 let mut child_buf = Vec::with_capacity(BLOCK_SIZE);
                 for &ptr in &pointers {
                     let (child, _) = Self::deserialize_from_block(file, &mut child_buf, ptr, nested)?;
                     n.push(child);
                 }
                 n
             } else {
                 Vec::new()
             };
             (Vec::new(), Vec::new(), nodes, Some(pointers))
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
        let (value_info, values): (Vec<ValueInfo>, Vec<V>) = if is_leaf {
            // Read value_info
            let info_length = u32_from_bytes(&slice[read_pos..read_pos + LEN_SIZE])? as usize;
            read_pos += LEN_SIZE;
            let info: Vec<ValueInfo> = binary_deserialize(&slice[read_pos..read_pos + info_length])?;

            // Values are loaded on-demand when nested=true
            if nested {
                let mut vals = Vec::with_capacity(info.len());
                for value_info in &info {
                    let value = Self::load_value_from_info(file, value_info)?;
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

    /// Load a value based on its storage info
    fn load_value_from_info<R: Read + Seek>(file: &mut R, info: &ValueInfo) -> io::Result<V> {
        match info.mode {
            ValueStorageMode::Single(offset) => {
                // Load single value (existing logic)
                Self::load_value_with_len(file, offset, info.length)
            }
            ValueStorageMode::Packed(block_offset, index) => {
                // Load from packed block
                Self::load_value_from_packed_block(file, block_offset, index, info.length)
            }
        }
    }

    /// Load a value from a packed block
    fn load_value_from_packed_block<R: Read + Seek>(
        file: &mut R,
        block_offset: u64,
        value_index: u16,
        _expected_length: u32,
    ) -> io::Result<V> {
        file.seek(SeekFrom::Start(block_offset))?;

        let mut block_buffer = vec![0u8; BLOCK_SIZE];
        file.read_exact(&mut block_buffer)?;

        // Read count
        let count = u32::from_le_bytes(block_buffer[0..4].try_into().map_err(to_io_error)?);
        let mut pos = 4;

        // Skip to target value
        for i in 0..=value_index {
            if pos + 4 > BLOCK_SIZE {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Packed block corrupted: position {pos} exceeds block size"),
                ));
            }

            let len = u32::from_le_bytes(block_buffer[pos..pos + 4].try_into().map_err(to_io_error)?) as usize;
            pos += 4;

            if i == value_index {
                // Found target value
                if pos + len > BLOCK_SIZE {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("Packed value corrupted: length {len} at position {pos} exceeds block size"),
                    ));
                }
                let value_data = &block_buffer[pos..pos + len];
                return binary_deserialize(value_data);
            }

            pos += len;
        }

        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Value index {value_index} not found in packed block (count: {count})"),
        ))
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

    let inner_order = (BLOCK_SIZE - base_overhead) / (key_size + POINTER_SIZE + MSGPACK_OVERHEAD_PER_ENTRY);
    let leaf_order = (BLOCK_SIZE - base_overhead) / (key_size + INFO_SIZE + MSGPACK_OVERHEAD_PER_ENTRY);

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
            dirty: true, // an empty tree is stored!
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
    buffer: &mut Vec<u8>,
    cache: &mut IndexMap<u64, Vec<u8>>,
    key: &K,
    start_offset: u64,
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
                        Some(info) => BPlusTreeNode::<K, V>::load_value_from_info(file, info).ok(),
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
    buffer: &mut Vec<u8>,
    cache: &mut IndexMap<u64, Vec<u8>>,
    key: &K,
    start_offset: u64,
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
                Some(info) => BPlusTreeNode::<K, V>::load_value_from_info(file, info).ok(),
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
                for value_info in &node.value_info {
                    let v = BPlusTreeNode::<K, V>::load_value_from_info(&mut self.query.file, value_info)?;
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
                for value_info in &node.value_info {
                    let v = BPlusTreeNode::<K, V>::load_value_from_info(&mut self.query.file, value_info)?;
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
                Err(err) => {
                    error!("BPlusTreeDiskIterator Failed to read next entry: {err}");
                    return None;
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
    inner_order: usize,
    leaf_order: usize,
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
        let (inner_order, leaf_order) = calc_order::<K>();

        Ok(Self {
            file,
            read_buffer: vec![0u8; BLOCK_SIZE],
            write_buffer: vec![0u8; BLOCK_SIZE],
            cache: IndexMap::with_capacity(1024),
            root_offset,
            lock,
            inner_order,
            leaf_order,
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
                    node.value_info[idx] = ValueInfo {
                        mode: ValueStorageMode::Single(val_offset),
                        length: value_len,
                    };

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
                    // Recurse to get a new child offset
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

    /// Update multiple items in batch. This is more efficient than calling `update()` multiple times
    /// as it performs all updates and then commits the final root offset once.
    /// returns The final root offset after all updates, or an error if any update fails
    pub fn update_batch(&mut self, items: &[(&K, &V)]) -> io::Result<u64> {
        if items.is_empty() {
            return Ok(self.root_offset);
        }

        let mut current_root = self.root_offset;

        // Perform all updates sequentially
        for (key, value) in items {
            current_root = self.update_recursive(current_root, key, value)?;
        }

        // Atomic Header Swap - only once at the end
        self.file.seek(SeekFrom::Start(ROOT_OFFSET_POS))?;
        self.file.write_all(&current_root.to_le_bytes())?;
        self.file.flush()?;
        self.file.sync_all()?;

        self.root_offset = current_root;
        Ok(current_root)
    }

    /// Insert or update multiple items in batch (upsert). If a key exists, it will be updated;
    /// if it doesn't exist, it will be inserted. This is more efficient than calling `update()`
    /// or `insert()` multiple times as it loads the tree once, performs all operations, and saves once.
    /// returns The final root offset after all upserts, or an error if any operation fails
    fn insert_value_to_disk(&mut self, value: &V) -> io::Result<(u64, u32)> {
        let value_bytes = binary_serialize(value)?;
        let value_len = u32::try_from(value_bytes.len()).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Value too large"))?;
        
        self.file.seek(SeekFrom::End(0))?;
        let offset = self.file.stream_position()?;
        
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
        Ok((offset, value_len))
    }

    fn write_node(&mut self, node: &BPlusTreeNode<K, V>) -> io::Result<u64> {
        self.file.seek(SeekFrom::End(0))?;
        let offset = self.file.stream_position()?;
        node.serialize_to_block(&mut self.file, &mut self.write_buffer, offset)?;
        Ok(offset)
    }

    fn write_internal_node(&mut self, node: &BPlusTreeNode<K, V>, pointers: &[u64]) -> io::Result<u64> {
        self.file.seek(SeekFrom::End(0))?;
        let offset = self.file.stream_position()?;
        node.serialize_internal_with_offsets(&mut self.file, &mut self.write_buffer, offset, pointers)?;
        Ok(offset)
    }

    fn upsert_recursive(&mut self, offset: u64, key: K, value: V) -> io::Result<(u64, Option<(K, u64)>)> {
        let (mut node, pointers_opt) = BPlusTreeNode::<K, V>::deserialize_from_block(
            &mut self.file,
            &mut self.read_buffer,
            offset,
            false // shallow
        )?;

        if node.is_leaf {
             match node.keys.binary_search(&key) {
                 Ok(idx) => {
                     let (val_off, val_len) = self.insert_value_to_disk(&value)?;
                     let new_info = ValueInfo { mode: ValueStorageMode::Single(val_off), length: val_len };
                     node.value_info[idx] = new_info;
                     
                     let new_offset = self.write_node(&node)?;
                     Ok((new_offset, None))
                 },
                 Err(idx) => {
                     let (val_off, val_len) = self.insert_value_to_disk(&value)?;
                     let new_info = ValueInfo { mode: ValueStorageMode::Single(val_off), length: val_len };
                     node.keys.insert(idx, key);
                     node.value_info.insert(idx, new_info);
                     
                     if node.keys.len() > self.leaf_order {
                         let median = self.leaf_order >> 1;
                         let mut right_node = BPlusTreeNode::new(true);
                         right_node.keys = node.keys.split_off(median);
                         right_node.value_info = node.value_info.split_off(median);
                         
                         let promoted_key = right_node.keys.first().ok_or_else(|| io::Error::other("Split resulted in empty right node keys"))?.clone();
                         
                         let left_offset = self.write_node(&node)?;
                         let right_offset = self.write_node(&right_node)?;
                         
                         Ok((left_offset, Some((promoted_key, right_offset))))
                     } else {
                         let new_offset = self.write_node(&node)?;
                         Ok((new_offset, None))
                     }
                 }
             }
        } else {
            let mut pointers = pointers_opt.unwrap();
            let idx = get_entry_index_upper_bound(&node.keys, &key);
            let child_offset = *pointers.get(idx).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Pointer index out of bounds"))?;
            
            let (new_child_offset, split_res) = self.upsert_recursive(child_offset, key, value)?;
            pointers[idx] = new_child_offset;
            
            if let Some((median_key, right_child_offset)) = split_res {
                node.keys.insert(idx, median_key);
                pointers.insert(idx + 1, right_child_offset);
                
                if node.keys.len() > self.inner_order {
                    let median = self.inner_order >> 1;
                    let mut right_node = BPlusTreeNode::new(false);
                    let right_pointers = pointers.split_off(median + 1);
                    right_node.keys = node.keys.split_off(median + 1);
                    
                    let promoted_key = node.keys.pop().unwrap();
                    
                    let left_offset = self.write_internal_node(&node, &pointers)?;
                    let right_offset = self.write_internal_node(&right_node, &right_pointers)?;
                    
                    Ok((left_offset, Some((promoted_key, right_offset))))
                } else {
                    let new_offset = self.write_internal_node(&node, &pointers)?;
                    Ok((new_offset, None))
                }
            } else {
                let new_offset = self.write_internal_node(&node, &pointers)?;
                Ok((new_offset, None))
            }
        }
    }

    /// Insert or update multiple items in batch (upsert).
    /// Uses disk-based recursive insertion (COW) to avoid loading full tree.
    pub fn upsert_batch(&mut self, items: &[(&K, &V)]) -> io::Result<u64> {
        if items.is_empty() {
             return Ok(self.root_offset);
        }

        let mut current_root = self.root_offset;

        for (key, value) in items {
             let (new_root, split) = self.upsert_recursive(current_root, (*key).clone(), (*value).clone())?;
             current_root = new_root;
             
             if let Some((median_key, right_child)) = split {
                 let mut new_root_node = BPlusTreeNode::<K, V>::new(false);
                 new_root_node.keys.push(median_key);
                 let pointers = vec![current_root, right_child];
                 
                 current_root = self.write_internal_node(&new_root_node, &pointers)?;
             }
        }
        
        self.file.seek(SeekFrom::Start(ROOT_OFFSET_POS))?;
        self.file.write_all(&current_root.to_le_bytes())?;
        self.file.flush()?;
        self.file.sync_all()?;
        
        self.root_offset = current_root;
        Ok(current_root)
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
        drop(tree);

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
        drop(tree);

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

    #[test]
    fn update_batch_basic_test() -> io::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let filepath = tempdir.path().join("tree_update_batch.bin");
        let mut tree = BPlusTree::<u32, Record>::new();

        // Create initial tree
        for i in 0u32..50 {
            tree.insert(i, Record {
                id: i,
                data: format!("Initial {i}"),
            });
        }
        tree.store(&filepath)?;
        drop(tree);

        // Test batch update
        let mut tree_update = BPlusTreeUpdate::<u32, Record>::try_new(&filepath)?;

        // Prepare batch updates
        let updates: Vec<(u32, Record)> = (0u32..50)
            .filter(|i| i % 5 == 0)
            .map(|i| (i, Record { id: i, data: format!("BatchUpdated {i}") }))
            .collect();

        let update_refs: Vec<(&u32, &Record)> = updates.iter()
            .map(|(k, v)| (k, v))
            .collect();

        tree_update.update_batch(&update_refs)?;
        drop(tree_update);

        // Verify all updates
        let mut tree_query = BPlusTreeQuery::<u32, Record>::try_new(&filepath)?;
        for i in 0u32..50 {
            let val = tree_query.query(&i).expect("Should find key");
            if i % 5 == 0 {
                assert_eq!(val.data, format!("BatchUpdated {i}"), "Batch updated key {i} should have new value");
            } else {
                assert_eq!(val.data, format!("Initial {i}"), "Non-updated key {i} should have original value");
            }
        }

        Ok(())
    }

    #[test]
    fn update_batch_empty_test() -> io::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let filepath = tempdir.path().join("tree_update_batch_empty.bin");
        let mut tree = BPlusTree::<u32, Record>::new();

        // Create initial tree
        for i in 0u32..10 {
            tree.insert(i, Record {
                id: i,
                data: format!("Initial {i}"),
            });
        }
        tree.store(&filepath)?;
        drop(tree);

        let mut tree_update = BPlusTreeUpdate::<u32, Record>::try_new(&filepath)?;
        let initial_root = tree_update.root_offset;

        // Test empty batch - should be no-op
        let empty_batch: Vec<(&u32, &Record)> = vec![];
        let result = tree_update.update_batch(&empty_batch)?;

        assert_eq!(result, initial_root, "Empty batch should not change root offset");

        // Verify data unchanged
        for i in 0u32..10 {
            let val = tree_update.query(&i).expect("Should find key");
            assert_eq!(val.data, format!("Initial {i}"));
        }

        Ok(())
    }

    #[test]
    fn update_batch_large_test() -> io::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let filepath = tempdir.path().join("tree_update_batch_large.bin");
        let mut tree = BPlusTree::<u32, Record>::new();

        let test_size = 200u32;

        // Create initial tree
        for i in 0..test_size {
            tree.insert(i, Record {
                id: i,
                data: format!("Initial {i}"),
            });
        }
        tree.store(&filepath)?;
        drop(tree);

        let mut tree_update = BPlusTreeUpdate::<u32, Record>::try_new(&filepath)?;

        // Prepare large batch update (every other item)
        let updates: Vec<(u32, Record)> = (0..test_size)
            .filter(|i| i % 2 == 0)
            .map(|i| (i, Record { id: i, data: format!("BatchUpdated {i}") }))
            .collect();

        let update_refs: Vec<(&u32, &Record)> = updates.iter()
            .map(|(k, v)| (k, v))
            .collect();

        // Perform batch update
        tree_update.update_batch(&update_refs)?;
        drop(tree_update);

        // Verify all updates via iterator
        let reloaded_tree = BPlusTree::<u32, Record>::load(&filepath)?;
        let mut count = 0;
        for (key, value) in &reloaded_tree {
            if *key % 2 == 0 {
                assert_eq!(value.data, format!("BatchUpdated {key}"), "Even keys should be batch updated");
            } else {
                assert_eq!(value.data, format!("Initial {key}"), "Odd keys should remain unchanged");
            }
            count += 1;
        }
        assert_eq!(count, test_size, "Should have all entries");

        Ok(())
    }

    #[test]
    fn update_batch_with_compaction_test() -> io::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let filepath = tempdir.path().join("tree_update_batch_compact.bin");
        let mut tree = BPlusTree::<u32, Record>::new();

        // Create initial tree with larger data
        let large_data = "x".repeat(1000);
        for i in 0u32..100 {
            tree.insert(i, Record {
                id: i,
                data: large_data.clone(),
            });
        }
        tree.store(&filepath)?;
        drop(tree);

        let mut tree_update = BPlusTreeUpdate::<u32, Record>::try_new(&filepath)?;

        // Batch update with smaller data
        let small_data = "y".repeat(50);
        let updates: Vec<(u32, Record)> = (0u32..100)
            .map(|i| (i, Record { id: i, data: small_data.clone() }))
            .collect();

        let update_refs: Vec<(&u32, &Record)> = updates.iter()
            .map(|(k, v)| (k, v))
            .collect();

        tree_update.update_batch(&update_refs)?;

        let size_before_compact = std::fs::metadata(&filepath)?.len();

        // Compact to reclaim space
        tree_update.compact(&filepath)?;

        let size_after_compact = std::fs::metadata(&filepath)?.len();
        assert!(size_after_compact < size_before_compact,
                "Compaction should reduce file size after batch update");

        // Verify all data is correct after compaction
        drop(tree_update);
        let mut tree_query = BPlusTreeQuery::<u32, Record>::try_new(&filepath)?;
        for i in 0u32..100 {
            let val = tree_query.query(&i).expect("Should find key after compaction");
            assert_eq!(val.data, small_data, "Data should be updated after compaction");
        }

        Ok(())
    }

    #[test]
    fn upsert_batch_mixed_test() -> io::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let filepath = tempdir.path().join("tree_upsert_batch_mixed.bin");
        let mut tree = BPlusTree::<u32, Record>::new();

        // Create initial tree with keys 0-49
        for i in 0u32..50 {
            tree.insert(i, Record {
                id: i,
                data: format!("Initial {i}"),
            });
        }
        tree.store(&filepath)?;
        drop(tree);

        let mut tree_update = BPlusTreeUpdate::<u32, Record>::try_new(&filepath)?;

        // Prepare upsert batch: update existing keys 0-24, insert new keys 50-74
        let mut updates: Vec<(u32, Record)> = Vec::new();

        // Updates to existing keys
        for i in 0u32..25 {
            updates.push((i, Record { id: i, data: format!("Updated {i}") }));
        }

        // Inserts for new keys
        for i in 50u32..75 {
            updates.push((i, Record { id: i, data: format!("Inserted {i}") }));
        }

        let update_refs: Vec<(&u32, &Record)> = updates.iter()
            .map(|(k, v)| (k, v))
            .collect();

        tree_update.upsert_batch(&update_refs)?;
        drop(tree_update);

        // Verify all 75 entries exist with correct values
        let mut tree_query = BPlusTreeQuery::<u32, Record>::try_new(&filepath)?;

        // Check updated keys (0-24)
        for i in 0u32..25 {
            let val = tree_query.query(&i).expect("Should find updated key");
            assert_eq!(val.data, format!("Updated {i}"), "Key {i} should be updated");
        }

        // Check unchanged keys (25-49)
        for i in 25u32..50 {
            let val = tree_query.query(&i).expect("Should find unchanged key");
            assert_eq!(val.data, format!("Initial {i}"), "Key {i} should remain unchanged");
        }

        // Check inserted keys (50-74)
        for i in 50u32..75 {
            let val = tree_query.query(&i).expect("Should find inserted key");
            assert_eq!(val.data, format!("Inserted {i}"), "Key {i} should be inserted");
        }

        Ok(())
    }

    #[test]
    fn upsert_batch_all_new_test() -> io::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let filepath = tempdir.path().join("tree_upsert_batch_new.bin");
        let mut tree = BPlusTree::<u32, Record>::new();

        // Create initial tree with unrelated keys
        for i in 0u32..10 {
            tree.insert(i, Record {
                id: i,
                data: format!("Initial {i}"),
            });
        }
        tree.store(&filepath)?;
        drop(tree);

        let mut tree_update = BPlusTreeUpdate::<u32, Record>::try_new(&filepath)?;

        // Upsert all new keys (100-149)
        let updates: Vec<(u32, Record)> = (100u32..150)
            .map(|i| (i, Record { id: i, data: format!("New {i}") }))
            .collect();

        let update_refs: Vec<(&u32, &Record)> = updates.iter()
            .map(|(k, v)| (k, v))
            .collect();

        tree_update.upsert_batch(&update_refs)?;
        drop(tree_update);

        // Verify all keys exist
        let mut tree_query = BPlusTreeQuery::<u32, Record>::try_new(&filepath)?;

        // Original keys should still exist
        for i in 0u32..10 {
            let val = tree_query.query(&i).expect("Should find original key");
            assert_eq!(val.data, format!("Initial {i}"));
        }

        // New keys should be inserted
        for i in 100u32..150 {
            let val = tree_query.query(&i).expect("Should find new key");
            assert_eq!(val.data, format!("New {i}"));
        }

        Ok(())
    }

    #[test]
    fn upsert_batch_all_existing_test() -> io::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let filepath = tempdir.path().join("tree_upsert_batch_existing.bin");
        let mut tree = BPlusTree::<u32, Record>::new();

        // Create initial tree
        for i in 0u32..100 {
            tree.insert(i, Record {
                id: i,
                data: format!("Initial {i}"),
            });
        }
        tree.store(&filepath)?;
        drop(tree);

        let mut tree_update = BPlusTreeUpdate::<u32, Record>::try_new(&filepath)?;

        // Upsert all existing keys (should behave like update)
        let updates: Vec<(u32, Record)> = (0u32..100)
            .map(|i| (i, Record { id: i, data: format!("Updated {i}") }))
            .collect();

        let update_refs: Vec<(&u32, &Record)> = updates.iter()
            .map(|(k, v)| (k, v))
            .collect();

        tree_update.upsert_batch(&update_refs)?;
        drop(tree_update);

        // Verify all values were updated
        let reloaded_tree = BPlusTree::<u32, Record>::load(&filepath)?;
        let mut count = 0;
        for (key, value) in &reloaded_tree {
            assert_eq!(value.data, format!("Updated {key}"), "All keys should be updated");
            count += 1;
        }
        assert_eq!(count, 100, "Should have exactly 100 entries");

        Ok(())
    }

    #[test]
    fn test_value_packing_efficiency() -> io::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let filepath = tempdir.path().join("packing_test.bin");
        let mut tree = BPlusTree::<u32, String>::new();
        
        // Insert 1000 small values (approx 50 bytes each)
        let small_value = "x".repeat(50);
        let count = 1000;
        for i in 0..count {
            tree.insert(i, small_value.clone());
        }
        
        tree.store(&filepath)?;
        
        let file_size = std::fs::metadata(&filepath)?.len();
        
        // Expected size without packing: 
        // 1000 items * 4096 bytes/block = 4,096,000 bytes (~4MB)
        // Plus internal nodes
        let unpacked_size_estimate = count as u64 * super::BLOCK_SIZE as u64;
        
        println!("File size with packing: {} bytes", file_size);
        println!("Estimated unpacked size: {} bytes", unpacked_size_estimate);
        
        // We expect significant savings. 
        // 1000 items * ~60 bytes / 4096 bytes/block ~= 15 blocks
        // Plus tree structure overhead. 
        // Let's be conservative and say it should be less than 10% of unpacked size.
        assert!(file_size < unpacked_size_estimate / 10, "Packing should reduce size by at least 90%");
        
        Ok(())
    }

    #[test]
    fn test_mixed_value_packing() -> io::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let filepath = tempdir.path().join("mixed_packing.bin");
        let mut tree = BPlusTree::<u32, String>::new();
        
        // Insert mixed values:
        // 0-99: Small (50 bytes) -> Packed
        // 100-109: Large (5000 bytes) -> Single (2 blocks)
        // 110-209: Small (50 bytes) -> Packed
        
        // Insert in order
        for i in 0..100 {
            tree.insert(i, "s".repeat(50));
        }
        for i in 100..110 {
            tree.insert(i, "L".repeat(5000));
        }
        for i in 110..210 {
            tree.insert(i, "s".repeat(50));
        }
        
        tree.store(&filepath)?;
        
        // Verify we can read them back correctly
        let mut query = BPlusTreeQuery::<u32, String>::try_new(&filepath)?;
        
        for i in 0..100 {
            let val = query.query(&i).expect("Should find small value");
            assert_eq!(val.len(), 50);
        }
        for i in 100..110 {
            let val = query.query(&i).expect("Should find large value");
            assert_eq!(val.len(), 5000);
        }
        for i in 110..210 {
            let val = query.query(&i).expect("Should find small value 2");
            assert_eq!(val.len(), 50);
        }
        
        Ok(())
    }
    #[test]
    fn test_upsert_huge_values_chunking() -> io::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let filepath = tempdir.path().join("tree_upsert_huge.bin");
        
        // Initialize tree
        let mut tree = BPlusTree::<u32, String>::new();
        tree.store(&filepath)?;
        drop(tree);
        
        let mut tree_update = BPlusTreeUpdate::<u32, String>::try_new(&filepath)?;
        
        // Insert values > BLOCK_SIZE (4096).
        // 10K value -> 3 chunks.
        let val1 = "A".repeat(10000);
        let val2 = "B".repeat(10000);
        
        let updates = vec![
            (1, val1.clone()),
            (2, val2.clone()),
        ];
        
        let update_refs: Vec<(&u32, &String)> = updates.iter().map(|(k,v)| (k,v)).collect();
        tree_update.upsert_batch(&update_refs)?;
        drop(tree_update);
        
        let mut query = BPlusTreeQuery::<u32, String>::try_new(&filepath)?;
        assert_eq!(query.query(&1).unwrap(), val1);
        assert_eq!(query.query(&2).unwrap(), val2);
        
        Ok(())
    }

    #[test]
    fn test_upsert_deep_split() -> io::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let filepath = tempdir.path().join("tree_upsert_split.bin");
        let mut tree = BPlusTree::<u32, u32>::new();
        tree.store(&filepath)?;
        drop(tree);
        
        let mut tree_update = BPlusTreeUpdate::<u32, u32>::try_new(&filepath)?;
        
        // Insert 5000 items. 
        // 5000 items ensures at least Root -> Internal -> Leaf split (Height 2 or 3).
        
        let count = 5000;
        let mut updates = Vec::with_capacity(count);
        for i in 0..count {
            let val = u32::try_from(i).unwrap();
            updates.push((val, val)); // value matches key
        }
        
        // Split into batches to test multiple batch ops
        for chunk in updates.chunks(1000) {
            let chunk_refs: Vec<(&u32, &u32)> = chunk.iter().map(|(k,v)| (k,v)).collect();
            tree_update.upsert_batch(&chunk_refs)?;
        }
        drop(tree_update);
        
        // Validation
        let mut query = BPlusTreeQuery::<u32, u32>::try_new(&filepath)?;
        for i in 0..count {
            let k = u32::try_from(i).unwrap();
            let val = query.query(&k).unwrap();
            assert_eq!(val, k);
        }
        Ok(())
    }
    
    #[test]
    fn test_upsert_batch_overwrites() -> io::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let filepath = tempdir.path().join("tree_upsert_overwrite.bin");
        let mut tree = BPlusTree::<u32, String>::new();
        tree.store(&filepath)?;
        drop(tree);
        
        let mut tree_update = BPlusTreeUpdate::<u32, String>::try_new(&filepath)?;
        
        // Batch contains same key multiple times
        let updates = vec![
            (1, "First".to_string()),
            (1, "Second".to_string()),
            (2, "Two".to_string()),
            (1, "Third".to_string()), 
        ];
        
        let update_refs: Vec<(&u32, &String)> = updates.iter().map(|(k,v)| (k,v)).collect();
        tree_update.upsert_batch(&update_refs)?;
        drop(tree_update);
        
        let mut query = BPlusTreeQuery::<u32, String>::try_new(&filepath)?;
        assert_eq!(query.query(&1).unwrap(), "Third");
        assert_eq!(query.query(&2).unwrap(), "Two");
        Ok(())
    }
    
    #[test]
    fn test_compaction_packing_limits() -> io::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let filepath = tempdir.path().join("tree_compact_pack.bin");
        let mut tree = BPlusTree::<u32, String>::new();
        tree.store(&filepath)?;
        drop(tree);
        
        let mut tree_update = BPlusTreeUpdate::<u32, String>::try_new(&filepath)?;
        
        // Insert 200 items of 100 bytes.
        // Upsert creates Single blocks blocks block-aligned.
        // File size > 200 * 4096 = 800KB.
        
        let val = "x".repeat(100);
        let count = 200;
        let mut updates = Vec::new();
        for i in 0..count {
            updates.push((i, val.clone()));
        }
        let refs: Vec<(&u32, &String)> = updates.iter().map(|(k,v)| (k,v)).collect();
        tree_update.upsert_batch(&refs)?;
        
        let size_before = std::fs::metadata(&filepath)?.len();
        assert!(size_before > count as u64 * 4000); 
        
        // Now Compact
        tree_update.compact(&filepath)?;
        drop(tree_update);
        
        let size_after = std::fs::metadata(&filepath)?.len();
        // 200 items * 100 bytes = 20KB payload.
        // Should pack into ~5-6 blocks (4KB each).
        
        println!("Size before: {}, Size after: {}", size_before, size_after);
        assert!(size_after < size_before / 10, "Compaction should pack values");
        assert!(size_after < 100 * 1024, "File should be small"); // < 100KB
        
        // Verify data
        let mut query = BPlusTreeQuery::<u32, String>::try_new(&filepath)?;
        for i in 0..count {
            assert_eq!(query.query(&i).unwrap(), val);
        }
        
        Ok(())
    }

    #[test]
    fn test_large_keys_multiblock_node() -> io::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let filepath = tempdir.path().join("tree_multiblock.bin");
        
        let mut tree = BPlusTree::<String, u32>::new();
        
        // 5 keys of 2000 bytes each. Total ~10KB keys.
        // Should span ~3 blocks (4KB each).
        for i in 0..5 {
            let key = format!("{:04}{}", i, "a".repeat(2000));
            tree.insert(key, i);
        }
        
        tree.store(&filepath)?;
        drop(tree);
        
        let loaded = BPlusTree::<String, u32>::load(&filepath)?;
        for i in 0..5 {
            let key = format!("{:04}{}", i, "a".repeat(2000));
            assert_eq!(loaded.query(&key), Some(i).as_ref());
        }
        Ok(())
    }

    #[test]
    fn test_upsert_multiblock_node() -> io::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let filepath = tempdir.path().join("tree_multiblock_upsert.bin");
        
        let mut tree = BPlusTree::<String, u32>::new();
        tree.store(&filepath)?;
        drop(tree);
        
        let mut updater = BPlusTreeUpdate::<String, u32>::try_new(&filepath)?;
        
        // Upsert large keys
        let mut batch = Vec::new();
        let keys: Vec<String> = (0..5).map(|i| format!("{:04}{}", i, "b".repeat(2000))).collect();
        let vals: Vec<u32> = (0..5).collect();
        
        for i in 0..5 {
            batch.push((&keys[i], &vals[i]));
        }
        
        updater.upsert_batch(&batch)?;
        drop(updater);
        
        let mut query = BPlusTreeQuery::<String, u32>::try_new(&filepath)?;
        for i in 0..5 {
            let val = query.query(&keys[i]).unwrap();
            assert_eq!(val, vals[i]);
        }
        Ok(())
    }
}

