use std::collections::HashMap;
use tokio::sync::RwLock;
use shared::model::{M3uPlaylistItem, XtreamPlaylistItem};
use crate::repository::bplustree::BPlusTree;
use crate::repository::target_id_mapping::VirtualIdRecord;

pub struct PlaylistXtreamStorage {
    pub id_mapping: BPlusTree<u32, VirtualIdRecord>,
    pub live: BPlusTree<u32, XtreamPlaylistItem>,
    pub vod: BPlusTree<u32, XtreamPlaylistItem>,
    pub series: BPlusTree<u32, XtreamPlaylistItem>,
}

pub type PlaylistM3uStorage = BPlusTree<u32, M3uPlaylistItem>;

pub enum PlaylistStorage {
    M3uPlaylist(Box<PlaylistM3uStorage>),
    XtreamPlaylist(Box<PlaylistXtreamStorage>),
}

pub struct TargetPlaylistStorage {
    pub xtream: Option<PlaylistXtreamStorage>,
    pub m3u: Option<PlaylistM3uStorage>,
}

pub type TargetPlaylistStorageMap = HashMap<String, TargetPlaylistStorage>;

pub struct PlaylistStorageState {
    pub data: RwLock<TargetPlaylistStorageMap>,
}

impl PlaylistStorageState {

    pub(crate) fn new() -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
        }
    }

    pub async fn cache_playlist(&self, target_name: &str, playlist: PlaylistStorage) {
        match playlist {
            PlaylistStorage::M3uPlaylist(m3u_playlist) => {
                match self.data.write().await.entry(target_name.to_string()) {
                    std::collections::hash_map::Entry::Occupied(mut entry) => {
                        let storage = entry.get_mut();
                        storage.m3u = Some(*m3u_playlist);
                    }
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(TargetPlaylistStorage {
                            xtream: None,
                            m3u: Some(*m3u_playlist),
                        });
                    }
                }
            }
            PlaylistStorage::XtreamPlaylist(xtream_playlist) => {
                match self.data.write().await.entry(target_name.to_string()) {
                    std::collections::hash_map::Entry::Occupied(mut entry) => {
                        let storage = entry.get_mut();
                        storage.xtream = Some(*xtream_playlist);
                    }
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(TargetPlaylistStorage {
                            xtream: Some(*xtream_playlist),
                            m3u: None,
                        });
                    }
                }
            }
        }
    }
}
