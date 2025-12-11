use crate::utils;
use log::error;
use ruzstd::decoding::StreamingDecoder;
use ruzstd::encoding::CompressionLevel;
use serde::{Deserialize, Serialize};
use shared::error::{str_to_io_error, to_io_error};
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::marker::PhantomData;
use std::mem::size_of;
use std::path::Path;
use tempfile::NamedTempFile;
use crate::utils::{bincode_deserialize, bincode_serialize};

const BLOCK_SIZE: usize = 4096;
const BINCODE_OVERHEAD: usize = 8;
const LEN_SIZE: usize = 4;
const FLAG_SIZE: usize = 1;

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


fn query_tree_le<K, V, R: Read + Seek>(file: &mut R, buffer: &mut Vec<u8>, key: &K) -> Option<V>
where
    K: Ord + Serialize + for<'de> Deserialize<'de> + Clone,
    V: Serialize + for<'de> Deserialize<'de> + Clone,
{
    let mut offset = 0;
    loop {
        match BPlusTreeNode::<K, V>::deserialize_from_block(file, buffer, offset, false) {
            Ok((node, pointers)) => {
                if node.is_leaf {
                    let idx = get_entry_index_upper_bound::<K>(&node.keys, key);
                    if idx == 0 {
                        return None;
                    }
                    return node.values.get(idx - 1).cloned();
                }
                let child_idx = get_entry_index_upper_bound::<K>(&node.keys, key);
                if let Some(child_offsets) = pointers {
                    if let Some(child_offset) = child_offsets.get(child_idx) {
                        offset = *child_offset;
                    } else {
                        // defensive: if out of bounds try last pointer
                        if let Some(last) = child_offsets.last() {
                            offset = *last;
                        } else {
                            return None;
                        }
                    }
                } else {
                    return None;
                }
            }
            Err(err) => {
                error!("Failed to read id tree from file {err}");
                return None;
            }
        }
    }
}



#[derive(Serialize, Deserialize, Debug, Clone)]
struct BPlusTreeNode<K, V> {
    keys: Vec<K>,
    children: Vec<BPlusTreeNode<K, V>>,
    is_leaf: bool,
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

    fn get_equal_entry_index(&self, key: &K) -> Option<usize>
    where
        K: Ord,
    {
        let mut left = 0;
        let mut right = self.keys.len().checked_sub(1)?;
        while left <= right {
            let mid = left + ((right - left) >> 1);
            let mid_key = &self.keys[mid];

            match mid_key.cmp(key) {
                std::cmp::Ordering::Equal => return Some(mid),
                std::cmp::Ordering::Greater => right = mid.checked_sub(1)?,
                std::cmp::Ordering::Less => left = mid + 1,
            }
        }
        None
    }

    fn get_entry_index_upper_bound(&self, key: &K) -> usize {
        get_entry_index_upper_bound::<K>(&self.keys, key)
    }

    fn insert(&mut self, key: K, v: V, inner_order: usize, leaf_order: usize) -> Option<Self> {
        if self.is_leaf {
            if let Ok(pos) = self.keys.binary_search(&key) {
                self.values[pos] = v;
                return None;
            }
            if let Some(eq_entry_index) = self.get_equal_entry_index(&key) {
                self.values.insert(eq_entry_index, v);
                return None;
            }
            let pos = self.get_entry_index_upper_bound(&key);
            self.keys.insert(pos, key);
            self.values.insert(pos, v);
            if self.is_overflow(leaf_order) {
                return Some(self.split(leaf_order));
            }
        } else {
            let pos = self.get_entry_index_upper_bound(&key);
            let child = self.children.get_mut(pos)?;
            let node = child.insert(key.clone(), v, inner_order, leaf_order);
            if let Some(tree_node) = node {
                if let Some(leaf_key) = Self::find_leaf_entry(&tree_node) {
                    let idx = self.get_entry_index_upper_bound(leaf_key);
                    if self.keys.binary_search(&key).is_err() {
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
            if let Some(child) = node.children.first() {
                self.children.push(child.clone());
            }
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

    fn serialize_to_block<W: Write + Seek>(
        &self,
        file: &mut W,
        buffer: &mut Vec<u8>, // must be length BLOCK_SIZE
        offset: u64,
    ) -> io::Result<u64> {
        // Keep backward-compatible on-disk layout with minimal allocations.
        let mut current_offset = offset;

        // zero the working block and set node type
        buffer.fill(0_u8);
        let buffer_slice = &mut buffer[..];
        buffer_slice[0] = u8::from(self.is_leaf);
        let mut write_pos = FLAG_SIZE;

        // ---- Write keys (length + bytes) into the first block ----
        let keys_encoded = bincode_serialize(&self.keys)?;
        let keys_len = keys_encoded.len();
        buffer_slice[write_pos..write_pos + LEN_SIZE]
            .copy_from_slice(&u32::try_from(keys_len).map_err(to_io_error)?.to_le_bytes());
        write_pos += LEN_SIZE;
        // NOTE: By design of the legacy layout, keys are expected to fit into the first block.
        buffer_slice[write_pos..write_pos + keys_len].copy_from_slice(&keys_encoded);
        write_pos += keys_len;
        drop(keys_encoded);

        // Prepare pointer offset for internal nodes (must be within first block)
        let pointer_offset_within_first_block = if self.is_leaf { 0u64 } else { offset + write_pos as u64 };

        // ---- Leaf values (optional) ----
        if self.is_leaf {
            // Encode values and decide compression exactly like the old layout
            let values_encoded = bincode_serialize(&self.values)?;
            let use_compression = values_encoded.len() + write_pos >= BLOCK_SIZE;

            // Compression flag
            buffer_slice[write_pos] = u8::from(use_compression);
            write_pos += FLAG_SIZE;

            // Content bytes (possibly compressed)
            let content_bytes = if use_compression {
                // ruzstd expects a Read implementor; wrap the slice in a Cursor
                ruzstd::encoding::compress_to_vec(std::io::Cursor::new(values_encoded.as_slice()), CompressionLevel::Fastest)
            } else {
                values_encoded
            };

            // Write content length
            let content_len = content_bytes.len();
            buffer_slice[write_pos..write_pos + LEN_SIZE]
                .copy_from_slice(&u32::try_from(content_len).map_err(to_io_error)?.to_le_bytes());
            write_pos += LEN_SIZE;

            // Copy as many content bytes as fit into the first block
            let space_left = BLOCK_SIZE.saturating_sub(write_pos);
            let first_copy = std::cmp::min(space_left, content_len);
            buffer_slice[write_pos..write_pos + first_copy]
                .copy_from_slice(&content_bytes[..first_copy]);

            // Write the full first block
            file.seek(SeekFrom::Start(offset))?;
            file.write_all(&buffer_slice[..BLOCK_SIZE])?;
            current_offset += BLOCK_SIZE as u64;

            // Stream remaining content (if any) block-by-block without extra allocations
            let mut pos = first_copy;
            while pos < content_len {
                let remaining = content_len - pos;
                let chunk = std::cmp::min(remaining, BLOCK_SIZE);
                // Copy the next chunk
                buffer_slice[..chunk].copy_from_slice(&content_bytes[pos..pos + chunk]);
                // If this is a partial block, zero the tail only once
                if chunk < BLOCK_SIZE {
                    buffer_slice[chunk..BLOCK_SIZE].fill(0u8);
                }
                file.write_all(&buffer_slice[..BLOCK_SIZE])?; // always write full block (pad with zeros)
                current_offset += BLOCK_SIZE as u64;
                pos += chunk;
            }

            // Free content buffer ASAP
            drop(content_bytes);
        } else {
            // Internal node: write out the full first block now
            file.seek(SeekFrom::Start(offset))?;
            file.write_all(&buffer_slice[..BLOCK_SIZE])?;
            current_offset += BLOCK_SIZE as u64;
        }

        // ---- Internal node pointers: serialize children, then write pointer list into first block ----
        if !self.is_leaf {
            let pointer_offset = pointer_offset_within_first_block; // inside the node's first block
            let mut pointer_vec: Vec<u64> = Vec::with_capacity(self.children.len());
            for child in &self.children {
                pointer_vec.push(current_offset);
                current_offset = child.serialize_to_block(file, buffer, current_offset)?;
            }
            let pointer_encoded = bincode_serialize(&pointer_vec)?;
            let pointer_len = u32::try_from(pointer_encoded.len()).map_err(to_io_error)?;
            file.seek(SeekFrom::Start(pointer_offset))?;
            file.write_all(&pointer_len.to_le_bytes())?;
            file.write_all(&pointer_encoded)?;
        }

        Ok(current_offset)
    }

    fn deserialize_from_block<R: Read + Seek>(
        file: &mut R,
        buffer: &mut Vec<u8>,
        offset: u64,
        nested: bool,
    ) -> io::Result<(Self, Option<Vec<u64>>)> {
        // Read the full first block into buffer (always aligned on-disk)
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(buffer)?;

        // Node type
        let is_leaf = buffer[0] == 1u8;
        let mut read_pos = FLAG_SIZE;

        // ---- Keys ----
        let keys_length = u32_from_bytes(&buffer[read_pos..read_pos + LEN_SIZE])? as usize;
        read_pos += LEN_SIZE;
        let keys: Vec<K> = bincode_deserialize(&buffer[read_pos..read_pos + keys_length])?;
        read_pos += keys_length;

        // ---- Values for leaf nodes ----
        let values: Vec<V> = if is_leaf {
            // compression flag
            let use_compression = buffer[read_pos] == 1u8;
            read_pos += FLAG_SIZE;

            // content length
            let values_length = u32_from_bytes(&buffer[read_pos..read_pos + LEN_SIZE])? as usize;
            read_pos += LEN_SIZE;

            // Fast path: everything is inside the first block
            let bytes_available_on_block = BLOCK_SIZE - read_pos;
            let mut content_bytes: Vec<u8> = vec![0u8; values_length];
            if values_length <= bytes_available_on_block {
                content_bytes[..values_length]
                    .copy_from_slice(&buffer[read_pos..read_pos + values_length]);
            } else {
                // Copy what's left in the first block
                content_bytes[..bytes_available_on_block]
                    .copy_from_slice(&buffer[read_pos..read_pos + bytes_available_on_block]);
                // Read the remaining bytes directly into the final buffer
                file.read_exact(&mut content_bytes[bytes_available_on_block..])?;
            }

            let values_bytes = if use_compression {
                decode_content(&content_bytes).unwrap_or(content_bytes)
            } else {
                content_bytes
            };

            bincode_deserialize(&values_bytes)?
        } else {
            Vec::new()
        };

        // ---- Pointers for internal nodes ----
        let (children, children_pointer): (Vec<Self>, Option<Vec<u64>>) = if is_leaf {
            (Vec::new(), None)
        } else {
            let pointers_length = u32_from_bytes(&buffer[read_pos..read_pos + LEN_SIZE])? as usize;
            read_pos += LEN_SIZE;
            let pointers: Vec<u64> = bincode_deserialize(&buffer[read_pos..read_pos + pointers_length])?;
            if nested {
                let mut nodes = Vec::with_capacity(pointers.len());
                for &ptr in &pointers {
                    let (child, _) = Self::deserialize_from_block(file, buffer, ptr, nested)?;
                    nodes.push(child);
                }
                (nodes, None)
            } else {
                (Vec::new(), Some(pointers))
            }
        };

        Ok((Self { keys, children, is_leaf, values }, children_pointer))
    }
}

fn decode_content(content_bytes: &[u8]) -> Option<Vec<u8>> {
    if let Ok(mut decoder) = StreamingDecoder::new(content_bytes) {
        let mut result = Vec::with_capacity(content_bytes.len());
        if decoder.read_to_end(&mut result).is_ok() {
            return Some(result);
        }
    }

    // TODO remove at next deployment, this is only fallback for older compressed files
    let mut decoder = flate2::write::ZlibDecoder::new(Vec::with_capacity(content_bytes.len()));
    if let Ok(()) = decoder.write_all(content_bytes) {
        if let Ok(decoded) = decoder.finish() {
            return Some(decoded);
        }
    }
    None
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BPlusTree<K, V> {
    root: BPlusTreeNode<K, V>,
    inner_order: usize,
    leaf_order: usize,
    dirty: bool,
}

const fn calc_order<K, V>() -> (usize, usize) {
    let overhead_size = BINCODE_OVERHEAD + LEN_SIZE + FLAG_SIZE;
    let key_size = size_of::<K>() + overhead_size;
    let value_size = key_size + size_of::<V>() + overhead_size;
    let inner_order = BLOCK_SIZE / key_size;
    let leaf_order = BLOCK_SIZE / (key_size + value_size);
    (inner_order, leaf_order)
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
        let (inner_order, leaf_order) = calc_order::<K, V>();
        Self {
            root: BPlusTreeNode::<K, V>::new(true),
            inner_order,
            leaf_order,
            dirty: false,
        }
    }

    const fn new_with_root(root: BPlusTreeNode<K, V>) -> Self {
        let (inner_order, leaf_order) = calc_order::<K, V>();
        Self {
            root,
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
            let tempfile = NamedTempFile::new()?;
            let mut file = utils::file_writer(&tempfile); //create_new_file_for_write(&tempfile)?);
            let mut buffer = vec![0u8; BLOCK_SIZE];
            match self.root.serialize_to_block(&mut file, &mut buffer, 0u64) {
                Ok(result) => {
                    file.flush()?;
                    drop(file);
                    if let Err(err) = utils::rename_or_copy(tempfile.path(), filepath, false) {
                        return Err(str_to_io_error(&format!("Temp file rename/copy did not work {} {err}", tempfile.path().to_string_lossy())));
                    }
                    self.dirty = false;
                    Ok(result)
                }
                Err(err) => {
                    Err(err)
                }
            }
        } else {
            Ok(0)
        }
    }

    pub fn load(filepath: &Path) -> io::Result<Self> {
        let file = is_file_valid(File::open(filepath)?)?;
        let mut reader = utils::file_reader(file);
        let mut buffer = vec![0u8; BLOCK_SIZE];
        let (root, _) = BPlusTreeNode::deserialize_from_block(&mut reader, &mut buffer, 0, true)?;
        Ok(Self::new_with_root(root))
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

fn query_tree<K, V, R: Read + Seek>(file: &mut R, buffer: &mut Vec<u8>, key: &K) -> Option<V>
where
    K: Ord + Serialize + for<'de> Deserialize<'de> + Clone,
    V: Serialize + for<'de> Deserialize<'de> + Clone,
{
    let mut offset = 0;
    loop {
        match BPlusTreeNode::<K, V>::deserialize_from_block(file, buffer, offset, false) {
            Ok((node, pointers)) => {
                if node.is_leaf {
                    return match node.keys.binary_search(key) {
                        Ok(idx) => node.values.get(idx).cloned(),
                        Err(_) => None,
                    };
                }
                let child_idx = get_entry_index_upper_bound::<K>(&node.keys, key);
                if let Some(child_offset) = pointers.unwrap().get(child_idx) {
                    offset = *child_offset;
                } else {
                    return None;
                }
            }
            Err(err) => {
                error!("Failed to read id tree from file {err}");
                return None;
            }
        }
    }
}
//
// fn traverse_tree<K, V, R: Read + Seek, F>(file: &mut R, offset: u64, callback: &mut F)
// where
//     K: Ord + Serialize + for<'de> Deserialize<'de> + Clone,
//     V: Serialize + for<'de> Deserialize<'de> + Clone,
//     F: FnMut(&Vec<K>, &Vec<V>),
// {
//     let current_offset = offset;
//     let mut buffer = vec![0u8; BLOCK_SIZE];
//
//     match BPlusTreeNode::<K, V>::deserialize_from_block(file, &mut buffer, current_offset, false) {
//         Ok((node, pointers)) => {
//             if node.is_leaf {
//                 callback(&node.keys, &node.values);
//             } else if let Some(child_pointers) = pointers {
//                 for &child_offset in &child_pointers {
//                     traverse_tree(file, child_offset, callback);
//                 }
//             }
//             // if it's a leaf we return.
//         }
//         Err(err) => {
//             error!("Failed to read tree node at offset {current_offset}: {err}");
//         }
//     }
// }

///
/// `BPlusTreeQuery` can be used to query the `BPlusTree` on-disk.
/// If you intend to do frequent queries then use `BPlusTree` instead which loads the tree into memory.
///
pub struct BPlusTreeQuery<K, V> {
    file: BufReader<File>,
    buffer: Vec<u8>,
    _marker_k: PhantomData<K>,
    _marker_v: PhantomData<V>,
}

impl<K, V> BPlusTreeQuery<K, V>
where
    K: Ord + Serialize + for<'de> Deserialize<'de> + Clone,
    V: Serialize + for<'de> Deserialize<'de> + Clone,
{
    pub fn try_from_file(file: File) -> io::Result<Self> {
        let file = is_file_valid(file)?;
        Ok(Self {
            file: utils::file_reader(file),
            buffer: vec![0u8; BLOCK_SIZE],
            _marker_k: PhantomData,
            _marker_v: PhantomData,
        })
    }


    pub fn try_new(filepath: &Path) -> io::Result<Self> {
        Self::try_from_file(File::open(filepath)?)
    }

    pub fn query(&mut self, key: &K) -> Option<V> {
        query_tree(&mut self.file, &mut self.buffer, key)
    }

    /// On-disk: find largest key <= `key` and return owned V (cloned/deserialized)
    pub fn query_le(&mut self, key: &K) -> Option<V> {
        // use the same buffer/reader pattern as query()
        // we need a mutable reader over the inner BufReader<File>
        let file = &mut self.file;
        // Seek to start to be safe
        if file.seek(SeekFrom::Start(0)).is_err() {
            // if seek fails, still try to query — but bail out with None
            return None;
        }
        query_tree_le(file, &mut self.buffer, key)
    }

    // pub fn traverse<F>(&mut self, mut visit: F)
    // where
    //     F: FnMut(&Vec<K>, &Vec<V>),
    // {
    //     traverse_tree(&mut self.file, 0, &mut visit);
    // }
}

pub struct BPlusTreeUpdate<K, V> {
    file: File,
    read_buffer: Vec<u8>,
    write_buffer: Vec<u8>,
    _marker_k: PhantomData<K>,
    _marker_v: PhantomData<V>,
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
        let file = is_file_valid(utils::open_read_write_file(filepath)?)?;
        Ok(Self {
            file,
            read_buffer: vec![0u8; BLOCK_SIZE],
            write_buffer: vec![0u8; BLOCK_SIZE],
            _marker_k: PhantomData,
            _marker_v: PhantomData,
        })
    }

    pub fn query(&mut self, key: &K) -> Option<V> {
        let mut reader = utils::file_reader(&mut self.file);
        query_tree(&mut reader, &mut self.read_buffer, key)
    }

    fn serialize_node(&mut self, offset: u64, node: &BPlusTreeNode<K, V>) -> io::Result<u64> {
        let result = node.serialize_to_block(&mut self.file, &mut self.write_buffer, offset);
        self.file.flush()?;
        result
    }

    pub fn update(&mut self, key: &K, value: V) -> io::Result<u64> {
        let mut offset = 0;
        let mut reader = utils::file_reader(&mut self.file);
        loop {
            match BPlusTreeNode::<K, V>::deserialize_from_block(&mut reader, &mut self.read_buffer, offset, false) {
                Ok((mut node, pointers)) => {
                    if node.is_leaf {
                        return match node.keys.binary_search(key) {
                            Ok(idx) => {
                                let old_value = node.values.get(idx);
                                if old_value.is_some() {
                                    node.values[idx] = value;
                                    return self.serialize_node(offset, &node);
                                }
                                Err(io::Error::new(io::ErrorKind::NotFound, "Entry not found"))
                            }
                            Err(_) => Err(io::Error::new(io::ErrorKind::NotFound, "Entry not found")),
                        };
                    }
                    let child_idx = get_entry_index_upper_bound::<K>(&node.keys, key);
                    if let Some(pters) = pointers {
                        if let Some(child_idx) = pters.get(child_idx) {
                            offset = *child_idx;
                        }
                    }
                }
                Err(err) => {
                    error!("Failed to read id tree from file {err}");
                    return Err(io::Error::new(io::ErrorKind::NotFound, format!("Failed to read id tree from file {err}")));
                }
            }
        }
    }

    /// On-disk update helper: find largest key <= `key`.
    pub fn query_le(&mut self, key: &K) -> Option<V> {
        let mut reader = utils::file_reader(&mut self.file);
        query_tree_le(&mut reader, &mut self.read_buffer, key)
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
        // Iterate over the current node
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

        // Move to the next node
        while let Some(node) = self.stack.pop() {
            if !node.is_leaf {
                // Push children in reverse order to maintain traversal order
                for child in node.children.iter().rev() {
                    self.stack.push(child);
                }
            }

            if node.is_leaf {
                self.current_keys = Some(&node.keys);
                self.current_values = Some(&node.values);
                self.index = 0;
                return self.next(); // Process the new leaf node
            }
        }

        None // No more elements
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
    use std::path::PathBuf;

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

        let filepath = PathBuf::from("/tmp/tree.bin");
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
    fn insert_dulplicate_test() {
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
        let filepath = PathBuf::from("/tmp/tree.bin");
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
}
