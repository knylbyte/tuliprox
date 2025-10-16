use shared::error::{info_err, TuliproxErrorKind};
use shared::error::{TuliproxError};
use crate::model::{AppConfig, ConfigTarget, TargetOutput};
use shared::model::{PlaylistGroup, PlaylistItemType, XtreamCluster, XtreamPlaylistItem};
use crate::model::Epg;
use crate::repository::epg_repository::epg_write;
use crate::repository::strm_repository::write_strm_playlist;
use crate::repository::m3u_repository::m3u_write_playlist;
use crate::repository::storage::{ensure_target_storage_path, get_target_id_mapping_file, get_target_storage_path};
use crate::repository::target_id_mapping::{TargetIdMapping, VirtualIdRecord};
use crate::repository::xtream_repository::{xtream_get_file_paths, xtream_get_storage_path, xtream_write_playlist};
use std::path::Path;
use shared::{create_tuliprox_error};
use shared::utils::{is_dash_url, is_hls_url};
use crate::api::model::{AppState, PlaylistStorage, PlaylistXtreamStorage};
use crate::repository::bplustree::{BPlusTree};
use crate::repository::indexed_document::{IndexedDocumentIterator};
use crate::utils;

pub async fn persist_playlist(app_config: &AppConfig, playlist: &mut [PlaylistGroup], epg: Option<&Epg>,
                              target: &ConfigTarget) -> Result<(), Vec<TuliproxError>> {
    let mut errors = vec![];
    let config = &app_config.config.load();
    let target_path = match ensure_target_storage_path(config, &target.name) {
        Ok(path) => path,
        Err(err) => return Err(vec![err]),
    };

    let (mut target_id_mapping, file_lock) = get_target_id_mapping(app_config, &target_path).await;

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
        }
    }

    for output in &target.output {
        let result = match output {
            TargetOutput::Xtream(_xtream_output) => xtream_write_playlist(app_config, target, playlist).await,
            TargetOutput::M3u(m3u_output) => m3u_write_playlist(app_config, target, m3u_output, &target_path, playlist).await,
            TargetOutput::Strm(strm_output) => write_strm_playlist(app_config, target, strm_output, playlist).await,
            TargetOutput::HdHomeRun(_hdhomerun_output) => Ok(()),
        };

        if let Err(err) = result {
            errors.push(err);
        } else if !playlist.is_empty() {
            if let Err(err) = epg_write(config, target, &target_path, epg, output) {
                errors.push(err);
            }
        }
    }

    if let Err(err) = target_id_mapping.persist() {
        errors.push(info_err!(err.to_string()));
    }
    drop(file_lock);

    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

pub async fn get_target_id_mapping(cfg: &AppConfig, target_path: &Path) -> (TargetIdMapping, utils::FileWriteGuard) {
    let target_id_mapping_file = get_target_id_mapping_file(target_path);
    let file_lock = cfg.file_locks.write_lock(&target_id_mapping_file).await;
    (TargetIdMapping::new(&target_id_mapping_file), file_lock)
}


fn load_target_id_mapping_as_tree(app_config: &AppConfig, target_path: &Path, target: &ConfigTarget) -> Result<BPlusTree<u32, VirtualIdRecord>, TuliproxError> {
    let target_id_mapping_file = get_target_id_mapping_file(&target_path);
    let _file_lock = app_config.file_locks.read_lock(&target_id_mapping_file);

    Ok(BPlusTree::<u32, VirtualIdRecord>::load(&target_id_mapping_file).map_err(|err|
        create_tuliprox_error!(
                                TuliproxErrorKind::Info,
                                "Could not find path for target {} err:{err}", &target.name
                            ))?)
}

fn load_xtream_playlist_as_tree(app_config: &AppConfig, storage_path: &Path, cluster: XtreamCluster) -> BPlusTree<u32, XtreamPlaylistItem> {
    let (main_path, index_path) = xtream_get_file_paths(storage_path, cluster);
    let _file_lock = app_config.file_locks.read_lock(&main_path);
    let mut tree = BPlusTree::<u32, XtreamPlaylistItem>::new();
    match IndexedDocumentIterator::<u32, XtreamPlaylistItem>::new(&main_path, &index_path) {
        Ok(reader) => {
            for (doc, _has_next) in reader {
                tree.insert(doc.virtual_id, doc);
            }
        }
        Err(_) => {}
    };
    tree
}

fn load_xtream_target_storage(app_config: &AppConfig, target: &ConfigTarget) -> Result<PlaylistXtreamStorage, TuliproxError> {
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

    let target_id_mapping = load_target_id_mapping_as_tree(app_config, &target_path, target)?;
    let live_storage = load_xtream_playlist_as_tree(app_config, &storage_path, XtreamCluster::Live);
    let vod_storage = load_xtream_playlist_as_tree(app_config, &storage_path, XtreamCluster::Video);
    let series_storage = load_xtream_playlist_as_tree(app_config, &storage_path, XtreamCluster::Series);

    Ok(PlaylistXtreamStorage {
        id_mapping: target_id_mapping,
        live: live_storage,
        vod: vod_storage,
        series: series_storage,
    })
}

pub async fn load_playlists_into_memory_cache(app_state: &AppState) -> Result<(), TuliproxError>{
    let app_config: &AppConfig = &app_state.app_config;
    for sources in app_state.app_config.sources.load().sources.iter() {
        for target in sources.targets.iter() {
            if target.use_memory_cache {
                for output in target.output.iter() {
                    match output {
                        TargetOutput::Xtream(_) => {
                            if let Ok(storage) = load_xtream_target_storage(app_config, target) {
                                app_state.cache_playlist(&target.name, PlaylistStorage::XtreamPlaylist(storage)).await;
                            }
                        }
                        TargetOutput::M3u(_) => {
                            if let Ok(storage) = load_m3u_target_storage(app_config, target) {
                                app_state.cache_playlist(&target.name, PlaylistStorage::M3uPlaylist(storage)).await;
                            }
                        }
                        _ => {}
                    };
                };
            }
        }
    }
    Ok(())
}