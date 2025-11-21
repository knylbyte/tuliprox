use crate::utils::file_reader;
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Write;
use std::path::Path;

pub fn json_filter_file<S: ::std::hash::BuildHasher>(file_path: &Path, filter: &HashMap<&str, HashSet<String, S>, S>) -> Vec<serde_json::Value> {
    let mut filtered: Vec<serde_json::Value> = Vec::with_capacity(1024);
    if !file_path.exists() {
        return filtered; // Return early if the file does not exist
    }

    let Ok(file) = File::open(file_path) else {
        return filtered;
    };

    let reader = file_reader(file);
    match serde_json::from_reader(reader) {
        Ok(value) => {
            if let Value::Array(list) = value {
                for entry in list {
                    if let Some(item) = entry.as_object() {
                        if filter.iter().all(|(&key, filter_set)| {
                            item.get(key).is_some_and(|field_value| match field_value {
                                Value::String(s) => filter_set.contains(s.as_str()),
                                Value::Number(n) => filter_set.contains(n.as_str()),
                                _ => false,
                            })
                        }) {
                            filtered.push(entry);
                        }
                    }
                }
            }
        }
        Err(_err) => {}
    }

    filtered
}

pub fn json_write_documents_to_file<T>(
    path: &std::path::Path,
    value: &T,
) -> std::io::Result<()>
where
    T: Serialize,
{
    let file = std::fs::File::create(path)?;
    let mut buf_writer = std::io::BufWriter::new(file);
    serde_json::to_writer(&mut buf_writer, value)?;
    buf_writer.flush()
}
