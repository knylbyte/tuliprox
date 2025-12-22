use crate::api::model::{AppState, PlaylistM3uStorage, PlaylistStorage, PlaylistStorageState, PlaylistXtreamStorage};
use crate::model::{AppConfig, ConfigTarget, TargetOutput};
use crate::model::{Epg};
use crate::processing::processor::playlist::apply_filter_to_playlist;
use crate::repository::bplustree::BPlusTree;
use crate::repository::epg_repository::epg_write;
use crate::repository::indexed_document::IndexedDocumentIterator;
use crate::repository::m3u_repository::{m3u_get_file_paths, m3u_write_playlist};
use crate::repository::storage::{ensure_target_storage_path, get_target_id_mapping_file, get_target_storage_path};
use crate::repository::strm_repository::write_strm_playlist;
use crate::repository::target_id_mapping::{TargetIdMapping, VirtualIdRecord};
use crate::repository::xtream_repository::{xtream_get_file_paths, xtream_get_storage_path, xtream_write_playlist};
use crate::utils;
use log::info;
use shared::create_tuliprox_error;
use shared::error::TuliproxError;
use shared::error::{info_err, TuliproxErrorKind};
use shared::model::{M3uPlaylistItem, PlaylistGroup, PlaylistItemHeader, PlaylistItemType, StreamProperties, XtreamCluster, XtreamPlaylistItem};
use shared::utils::{is_dash_url, is_hls_url};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

struct LocalEpisodeKey {
    path: String,
    virtual_id: u32,
}

pub async fn persist_playlist(app_config: &AppConfig, playlist: &mut [PlaylistGroup], epg: Option<&Epg>,
                              target: &ConfigTarget, playlist_state: Option<&Arc<PlaylistStorageState>>) -> Result<(), Vec<TuliproxError>> {
    let mut errors = vec![];
    let config = &app_config.config.load();
    let target_path = match ensure_target_storage_path(config, &target.name) {
        Ok(path) => path,
        Err(err) => return Err(vec![err]),
    };

    let (mut target_id_mapping, file_lock) = get_target_id_mapping(app_config, &target_path).await;

    let mut local_library_series = HashMap::<String, Vec<LocalEpisodeKey>>::new();

    // Virtual IDs assignment
    for group in playlist.iter_mut() {
        for channel in &mut group.channels {
            let header = &mut channel.header;
            let provider_id = header.get_provider_id().unwrap_or_default();
            if provider_id == 0 {
                header.item_type = match (is_hls_url(&header.url), header.item_type) {
                    (true, _) => PlaylistItemType::LiveHls,
                    (false, PlaylistItemType::Live) => {
                        if is_dash_url(&header.url) {
                            PlaylistItemType::LiveDash
                        } else {
                            PlaylistItemType::LiveUnknown
                        }
                    }
                    _ => header.item_type,
                };
            }

            let uuid = header.get_uuid();
            let item_type = header.item_type;
            header.virtual_id = target_id_mapping.get_and_update_virtual_id(uuid, provider_id, item_type, 0);

            assign_local_series_info_episode_key(&mut local_library_series, header, item_type);
        }
    }

    rewrite_local_series_info_episode_virtual_id(playlist, &mut local_library_series);

    for output in &target.output {
        let mut filtered = match output {
            TargetOutput::Xtream(out) => out.filter.as_ref().and_then(|flt| apply_filter_to_playlist(playlist, flt)),
            TargetOutput::M3u(out) => out.filter.as_ref().and_then(|flt| apply_filter_to_playlist(playlist, flt)),
            TargetOutput::Strm(out) => out.filter.as_ref().and_then(|flt| apply_filter_to_playlist(playlist, flt)),
            TargetOutput::HdHomeRun(_) => None,
        };

        let pl: &mut [PlaylistGroup] = if let Some(filtered_playlist) = filtered.as_mut() {
            filtered_playlist.as_mut_slice()
        } else {
            playlist
        };

        let result = match output {
            TargetOutput::Xtream(_xtream_output) => xtream_write_playlist(app_config, target, pl).await,
            TargetOutput::M3u(m3u_output) => m3u_write_playlist(app_config, target, m3u_output, &target_path, pl).await,
            TargetOutput::Strm(strm_output) => write_strm_playlist(app_config, target, strm_output, pl).await,
            TargetOutput::HdHomeRun(_hdhomerun_output) => Ok(()),
        };

        match result {
            Ok(()) => {
                if !playlist.is_empty() {
                    if let Err(err) = epg_write(config, target, &target_path, epg, output).await {
                        errors.push(err);
                    }
                }
            }
            Err(err) => errors.push(err)
        }
    }

    if let Err(err) = target_id_mapping.persist() {
        errors.push(info_err!(err.to_string()));
    }
    drop(file_lock);

    if target.use_memory_cache {
        if let Some(playlist_storage) = playlist_state {
            for output in &target.output {
                match output {
                    TargetOutput::Xtream(_) => {
                        if let Ok(storage) = load_xtream_target_storage(app_config, target).await {
                            playlist_storage.cache_playlist(&target.name, PlaylistStorage::XtreamPlaylist(Box::new(storage))).await;
                        }
                    }
                    TargetOutput::M3u(_) => {
                        if let Ok(storage) = load_m3u_target_storage(app_config, target).await {
                            playlist_storage.cache_playlist(&target.name, PlaylistStorage::M3uPlaylist(Box::new(storage))).await;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

fn assign_local_series_info_episode_key(local_library_series: &mut HashMap<String, Vec<LocalEpisodeKey>>, header: &mut PlaylistItemHeader, item_type: PlaylistItemType) {
    // we need to rewrite local series info with the new virtual ids
    if item_type == PlaylistItemType::LocalSeries {
        local_library_series
            .entry(header.parent_code.clone())
            .or_default()
            .push(LocalEpisodeKey {
                path: header.url.clone(),
                virtual_id: header.virtual_id,
            });
    }
}

fn rewrite_local_series_info_episode_virtual_id(playlist: &mut [PlaylistGroup], local_library_series: &mut HashMap<String, Vec<LocalEpisodeKey>>) {
    // assign local series virtual ids
    for group in playlist.iter_mut() {
        for channel in &mut group.channels {
            let header = &mut channel.header;
            if header.item_type == PlaylistItemType::LocalSeriesInfo {
                if let Some(episode_keys) = local_library_series.get(&header.id) {
                    if let Some(stream_props) = header.additional_properties.as_mut() {
                        match stream_props {
                            StreamProperties::Live(_)
                            | StreamProperties::Video(_)
                            | StreamProperties::Episode(_) => {}
                            StreamProperties::Series(series) => {
                                if let Some(episodes) =
                                    series.details.as_mut().and_then(|d| d.episodes.as_mut())
                                {
                                    for episode in episodes.iter_mut() {
                                        for episode_key in episode_keys {
                                            if episode.direct_source == episode_key.path {
                                                episode.id = episode_key.virtual_id;
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub async fn get_target_id_mapping(cfg: &AppConfig, target_path: &Path) -> (TargetIdMapping, utils::FileWriteGuard) {
    let target_id_mapping_file = get_target_id_mapping_file(target_path);
    let file_lock = cfg.file_locks.write_lock(&target_id_mapping_file).await;
    (TargetIdMapping::new(&target_id_mapping_file), file_lock)
}


async fn load_target_id_mapping_as_tree(app_config: &AppConfig, target_path: &Path, target: &ConfigTarget) -> Result<BPlusTree<u32, VirtualIdRecord>, TuliproxError> {
    let target_id_mapping_file = get_target_id_mapping_file(target_path);
    let _file_lock = app_config.file_locks.read_lock(&target_id_mapping_file).await;

    BPlusTree::<u32, VirtualIdRecord>::load(&target_id_mapping_file).map_err(|err|
        create_tuliprox_error!(
                                TuliproxErrorKind::Info,
                                "Could not find path for target {} err:{err}", &target.name
                            ))
}

async fn load_xtream_playlist_as_tree(app_config: &AppConfig, storage_path: &Path, cluster: XtreamCluster) -> BPlusTree<u32, XtreamPlaylistItem> {
    let (main_path, index_path) = xtream_get_file_paths(storage_path, cluster);
    let _file_lock = app_config.file_locks.read_lock(&main_path).await;
    let mut tree = BPlusTree::<u32, XtreamPlaylistItem>::new();
    if let Ok(reader) = IndexedDocumentIterator::<u32, XtreamPlaylistItem>::new(&main_path, &index_path) {
        for (doc, _has_next) in reader {
            tree.insert(doc.virtual_id, doc);
        }
    }
    tree
}

async fn load_xtream_target_storage(app_config: &AppConfig, target: &ConfigTarget) -> Result<PlaylistXtreamStorage, TuliproxError> {
    let config = app_config.config.load();
    let target_path = get_target_storage_path(&config, target.name.as_str()).ok_or_else(||
        create_tuliprox_error!(
                                TuliproxErrorKind::Info,
                                "Could not find path for target {}", &target.name
                            ))?;

    let storage_path = xtream_get_storage_path(&config, target.name.as_str()).ok_or_else(||
        create_tuliprox_error!(
                                TuliproxErrorKind::Info,
                            "Could not find path for target {} xtream output", &target.name))?;

    let target_id_mapping = load_target_id_mapping_as_tree(app_config, &target_path, target).await?;
    let live_storage = load_xtream_playlist_as_tree(app_config, &storage_path, XtreamCluster::Live).await;
    let vod_storage = load_xtream_playlist_as_tree(app_config, &storage_path, XtreamCluster::Video).await;
    let series_storage = load_xtream_playlist_as_tree(app_config, &storage_path, XtreamCluster::Series).await;

    Ok(PlaylistXtreamStorage {
        id_mapping: target_id_mapping,
        live: live_storage,
        vod: vod_storage,
        series: series_storage,
    })
}

async fn load_m3u_target_storage(app_config: &AppConfig, target: &ConfigTarget) -> Result<PlaylistM3uStorage, TuliproxError> {
    let config = app_config.config.load();
    let target_path = get_target_storage_path(&config, target.name.as_str()).ok_or_else(||
        create_tuliprox_error!(
                                TuliproxErrorKind::Info,
                                "Could not find path for target {}", &target.name
                            ))?;

    let (main_path, index_path) = m3u_get_file_paths(&target_path);
    let _file_lock = app_config.file_locks.read_lock(&main_path).await;
    let mut tree = BPlusTree::<u32, M3uPlaylistItem>::new();
    if let Ok(reader) = IndexedDocumentIterator::<u32, M3uPlaylistItem>::new(&main_path, &index_path) {
        for (doc, _has_next) in reader {
            tree.insert(doc.virtual_id, doc);
        }
    }
    Ok(tree)
}


pub async fn load_playlists_into_memory_cache(app_state: &AppState) -> Result<(), TuliproxError> {
    for sources in &app_state.app_config.sources.load().sources {
        for target in &sources.targets {
            load_target_into_memory_cache(app_state, target).await;
        }
    }
    Ok(())
}

pub async fn load_target_into_memory_cache(app_state: &AppState, target: &Arc<ConfigTarget>) {
    if target.use_memory_cache {
        info!("Loading target {} into memory cache", target.name);
        for output in &target.output {
            match output {
                TargetOutput::Xtream(_) => {
                    if let Ok(storage) = load_xtream_target_storage(&app_state.app_config, target).await {
                        app_state.cache_playlist(&target.name, PlaylistStorage::XtreamPlaylist(Box::new(storage))).await;
                    }
                }
                TargetOutput::M3u(_) => {
                    if let Ok(storage) = load_m3u_target_storage(&app_state.app_config, target).await {
                        app_state.cache_playlist(&target.name, PlaylistStorage::M3uPlaylist(Box::new(storage))).await;
                    }
                }
                _ => {}
            }
        };
    }
}
