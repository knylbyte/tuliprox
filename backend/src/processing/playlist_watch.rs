use std::collections::BTreeSet;
use std::path::{Path};
use std::sync::Arc;
use log::{error, info};
use shared::model::{MsgKind, PlaylistGroup};
use crate::messaging::{send_message};
use crate::model::Config;
use crate::utils;
use crate::utils::{bincode_deserialize, bincode_serialize};

pub fn process_group_watch(client: &Arc<reqwest::Client>, cfg: &Config, target_name: &str, pl: &PlaylistGroup) {
    let mut new_tree = BTreeSet::new();
    pl.channels.iter().for_each(|chan| {
        let header = &chan.header;
        let title = if header.title.is_empty() { header.name.clone() } else { header.title.clone() };
        new_tree.insert(title);
    });

    let watch_filename = format!("{}/{}.bin", utils::sanitize_filename(target_name), utils::sanitize_filename(&pl.title));
    match utils::get_file_path(&cfg.working_dir, Some(std::path::PathBuf::from(&watch_filename))) {
        Some(path) => {
            let save_path = path.as_path();
            let mut changed = false;
            if path.exists() {
                if let Some(loaded_tree) = load_watch_tree(&path) {
                    // Find elements in set2 but not in set1
                    let added_difference: BTreeSet<String> = new_tree.difference(&loaded_tree).cloned().collect();
                    let removed_difference: BTreeSet<String> = loaded_tree.difference(&new_tree).cloned().collect();
                    if !added_difference.is_empty() || !removed_difference.is_empty() {
                        changed = true;
                        handle_watch_notification(client, cfg, &added_difference, &removed_difference, target_name, &pl.title);
                    }
                } else {
                    error!("failed to load watch_file {}", &path.to_str().unwrap_or_default());
                    changed = true;
                }
            } else {
                changed = true;
            }
            if changed {
                match save_watch_tree(save_path, &new_tree) {
                    Ok(()) => {}
                    Err(err) => {
                        error!("failed to write watch_file {}: {}", save_path.to_str().unwrap_or_default(), err);
                    }
                }
            }
        }
        None => {
            error!("failed to write watch_file {}", &watch_filename);
        }
    }
}

#[derive(Debug, serde::Serialize)]
struct WatchChanges {
    pub target: String,
    pub group: String,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

fn handle_watch_notification(client: &Arc<reqwest::Client>, cfg: &Config, added: &BTreeSet<String>, removed: &BTreeSet<String>, target_name: &str, group_name: &str) {
    let added = added.iter().map(std::string::ToString::to_string).collect::<Vec<String>>();
    let removed = removed.iter().map(std::string::ToString::to_string).collect::<Vec<String>>();
    if !added.is_empty() || !removed.is_empty() {
        let changes = WatchChanges {
            target: target_name.to_string(),
            group: group_name.to_string(),
            added,
            removed
        };

        let msg = serde_json::to_string_pretty(&changes).unwrap_or_else(|_| "Error: Failed to serialize watch changes".to_string());
        info!("{}", &msg);
        send_message(client, &MsgKind::Watch, cfg.messaging.as_ref(), &msg);
    }
}

fn load_watch_tree(path: &Path) -> Option<BTreeSet<String>> {
    std::fs::read(path).map_or(None, |encoded| {
            let decoded = bincode_deserialize(&encoded[..]).ok()?;
            Some(decoded)
        })
}

fn save_watch_tree(path: &Path, tree: &BTreeSet<String>) -> std::io::Result<()> {
    let encoded: Vec<u8> = bincode_serialize(&tree)?;
    std::fs::write(path, encoded)
}

