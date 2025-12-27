use crate::api::model::AppState;
use crate::model::PlaylistXtreamCategory;
use crate::model::{AppConfig, ProxyUserCredentials};
use crate::model::{Config, ConfigInput, ConfigTarget};
use crate::repository::bplustree::{BPlusTree, BPlusTreeQuery, BPlusTreeUpdate};
use crate::repository::playlist_scratch::PlaylistScratch;
use crate::repository::storage::{get_input_storage_path, get_target_id_mapping_file, get_target_storage_path};
use crate::repository::storage_const;
use crate::repository::target_id_mapping::VirtualIdRecord;
use crate::repository::xtream_playlist_iterator::XtreamPlaylistJsonIterator;
use crate::utils::json_write_documents_to_file;
use crate::utils::FileReadGuard;
use crate::utils::{async_file_reader, async_open_readonly_file, file_reader};
use bytes::Bytes;
use futures::{stream, Stream, StreamExt};
use indexmap::IndexMap;
use log::error;
use serde::Serialize;
use serde_json::{json, Value};
use shared::error::{create_tuliprox_error, create_tuliprox_error_result, info_err, notify_err, str_to_io_error, to_io_error, TuliproxError, TuliproxErrorKind};
use shared::model::xtream_const::XTREAM_CLUSTER;
use shared::model::{PlaylistGroup, PlaylistItem, PlaylistItemType, SeriesStreamProperties, StreamProperties, VideoStreamProperties, XtreamCluster, XtreamPlaylistItem};
use shared::utils::get_u32_from_serde_value;
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::task;

macro_rules! cant_write_result {
    ($path:expr, $err:expr) => {
        create_tuliprox_error!(
            TuliproxErrorKind::Notify,
            "failed to write xtream playlist: {} - {}",
            $path.display(),
            $err
        )
    };
}

macro_rules! try_option_ok {
    ($option:expr) => {
        match $option {
            Some(value) => value,
            None => return Ok(()),
        }
    };
}

#[inline]
fn get_collection_path(path: &Path, collection: &str) -> PathBuf {
    path.join(format!("{collection}.json"))
}

#[inline]
fn get_live_cat_collection_path(path: &Path) -> PathBuf {
    get_collection_path(path, storage_const::COL_CAT_LIVE)
}

#[inline]
fn get_vod_cat_collection_path(path: &Path) -> PathBuf {
    get_collection_path(path, storage_const::COL_CAT_VOD)
}

#[inline]
fn get_series_cat_collection_path(path: &Path) -> PathBuf {
    get_collection_path(path, storage_const::COL_CAT_SERIES)
}


fn ensure_xtream_storage_path(cfg: &Config, target_name: &str) -> Result<PathBuf, TuliproxError> {
    if let Some(path) = xtream_get_storage_path(cfg, target_name) {
        if std::fs::create_dir_all(&path).is_err() {
            let msg = format!(
                "Failed to save xtream data, can't create directory {}",
                &path.display()
            );
            return Err(notify_err!(msg));
        }
        Ok(path)
    } else {
        let msg = format!("Failed to save xtream data, can't create directory for target {target_name}");
        Err(notify_err!(msg))
    }
}

pub fn xtream_get_info_file_path(
    storage_path: &Path,
    cluster: XtreamCluster,
) -> Option<PathBuf> {
    if cluster == XtreamCluster::Series {
        return Some(storage_path.join(format!("{}.{}", storage_const::FILE_SERIES_INFO, storage_const::FILE_SUFFIX_DB)));
    } else if cluster == XtreamCluster::Video {
        return Some(storage_path.join(format!("{}.{}", storage_const::FILE_VOD_INFO, storage_const::FILE_SUFFIX_DB)));
    }
    None
}

pub fn xtream_get_record_file_path(storage_path: &Path, item_type: PlaylistItemType) -> Option<PathBuf> {
    match item_type {
        PlaylistItemType::Video
        | PlaylistItemType::LocalVideo => Some(storage_path.join(format!("{}.{}", storage_const::FILE_VOD_INFO_RECORD, storage_const::FILE_SUFFIX_DB))),
        PlaylistItemType::SeriesInfo
        | PlaylistItemType::LocalSeriesInfo => Some(storage_path.join(format!("{}.{}", storage_const::FILE_SERIES_INFO_RECORD, storage_const::FILE_SUFFIX_DB))),
        PlaylistItemType::Series
        | PlaylistItemType::LocalSeries => Some(storage_path.join(format!("{}.{}", storage_const::FILE_SERIES_EPISODE_RECORD, storage_const::FILE_SUFFIX_DB))),
        _ => None,
    }
}
async fn write_playlists_to_file(
    app_config: &Arc<AppConfig>,
    storage_path: &Path,
    collections: Vec<(XtreamCluster, &[&mut PlaylistItem])>,
) -> Result<(), TuliproxError> {
    for (cluster, playlist) in collections {
        if playlist.is_empty() {
            continue;
        }
        let xtream_path = xtream_get_file_path(storage_path, cluster);
        {
            let _file_lock = app_config.file_locks.write_lock(&xtream_path).await;
            let mut tree = BPlusTree::new();
            for item in playlist {
                tree.insert(item.header.virtual_id, item.to_xtream());
            }
            tree.store(&xtream_path).map_err(|err| cant_write_result!(&xtream_path, err))?;
        }
    }
    Ok(())
}

async fn write_playlists_to_file_2(
    app_config: &Arc<AppConfig>,
    storage_path: &Path,
    cluster: XtreamCluster,
    playlist: &[PlaylistItem],
) -> Result<(), TuliproxError> {
    if !playlist.is_empty() {
        let xtream_path = xtream_get_file_path(storage_path, cluster);
        let _file_lock = app_config.file_locks.write_lock(&xtream_path).await;
        let mut tree = BPlusTree::new();
        for item in playlist {
            tree.insert(item.header.virtual_id, item.to_xtream());
        }
        tree.store(&xtream_path).map_err(|err| cant_write_result!(&xtream_path, err))?;
    }
    Ok(())
}

pub async fn write_playlist_item_to_file(
    app_config: &Arc<AppConfig>,
    target_name: &str,
    pli: &XtreamPlaylistItem,
) -> Result<(), TuliproxError> {
    let storage_path = {
        let config = app_config.config.load();
        ensure_xtream_storage_path(&config, target_name)?
    };
    let xtream_path = xtream_get_file_path(&storage_path, pli.xtream_cluster);
    {
        let _file_lock = app_config.file_locks.write_lock(&xtream_path).await;
        let mut tree = if xtream_path.exists() {
            BPlusTreeUpdate::try_new(&xtream_path).map_err(|err| cant_write_result!(&xtream_path, err))?
        } else {
            // This case should rarely happen as the file is usually pre-created, but for safety:
            return Err(cant_write_result!(&xtream_path, "BPlusTree file not found for append"));
        };
        tree.update(&pli.virtual_id, pli.clone()).map_err(|err| cant_write_result!(&xtream_path, err))?;
    }
    Ok(())
}


fn get_map_item_as_str(map: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    if let Some(value) = map.get(key) {
        if let Some(result) = value.as_str() {
            return Some(result.to_string());
        }
    }
    None
}

async fn load_old_category_ids(path: &Path) -> (u32, HashMap<String, u32>) {
    let old_path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut result: HashMap<String, u32> = HashMap::new();
        let mut max_id: u32 = 0;
        for (cluster, cat) in [(XtreamCluster::Live, storage_const::COL_CAT_LIVE), (XtreamCluster::Video, storage_const::COL_CAT_VOD), (XtreamCluster::Series, storage_const::COL_CAT_SERIES)] {
            let col_path = get_collection_path(&old_path, cat);
            if col_path.exists() {
                if let Ok(file) = File::open(col_path) {
                    let reader = file_reader(file);
                    match serde_json::from_reader(reader) {
                        Ok(value) => {
                            if let Value::Array(list) = value {
                                for entry in list {
                                    if let Some(category_id) = entry.get(crate::model::XC_TAG_CATEGORY_ID).and_then(get_u32_from_serde_value) {
                                        if let Value::Object(item) = entry {
                                            if let Some(category_name) = get_map_item_as_str(&item, crate::model::XC_TAG_CATEGORY_NAME) {
                                                result.insert(format!("{cluster}{category_name}"), category_id);
                                                max_id = max_id.max(category_id);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Err(_err) => {}
                    }
                }
            }
        }
        (max_id, result)
    }).await.unwrap_or_else(|_| (0, HashMap::new()))
}

pub fn xtream_get_storage_path(cfg: &Config, target_name: &str) -> Option<PathBuf> {
    get_target_storage_path(cfg, target_name).map(|target_path| target_path.join(PathBuf::from(storage_const::PATH_XTREAM)))
}

pub fn xtream_get_epg_file_path(path: &Path) -> PathBuf {
    path.join(storage_const::FILE_EPG)
}

fn xtream_get_file_path_for_name(storage_path: &Path, name: &str) -> PathBuf {
    storage_path.join(format!("{name}.{}", storage_const::FILE_SUFFIX_DB))
}

pub fn xtream_get_file_path(storage_path: &Path, cluster: XtreamCluster) -> PathBuf {
    xtream_get_file_path_for_name(storage_path, &cluster.as_str().to_lowercase())
}

// pub fn xtream_get_file_paths_for_series(storage_path: &Path) -> (PathBuf, PathBuf) {
//     xtream_get_file_paths_for_name(storage_path, storage_const::FILE_SERIES)
// }

async fn xtream_garbage_collect(app_cfg: &Arc<AppConfig>, target_name: &str) -> std::io::Result<()> {
    // Garbage collect series
    let storage_path = {
        let cfg = app_cfg.config.load();
        try_option_ok!(xtream_get_storage_path(&cfg, target_name))
    };
    let info_path = try_option_ok!(xtream_get_info_file_path(
        &storage_path,
        XtreamCluster::Series
    ));
    if info_path.exists() {
        let _file_lock = app_cfg.file_locks.write_lock(&info_path).await;
        let mut tree = BPlusTreeUpdate::<u32, StreamProperties>::try_new(&info_path)?;
        tree.compact(&info_path)?;
    }
    Ok(())
}

#[derive(Serialize)]
struct CategoryEntry {
    category_id: u32,
    category_name: String,
    parent_id: u32,
}

pub async fn xtream_write_playlist(
    app_cfg: &Arc<AppConfig>,
    target: &ConfigTarget,
    playlist: &mut [PlaylistGroup],
) -> Result<(), TuliproxError> {
    let path = {
        let config = app_cfg.config.load();
        ensure_xtream_storage_path(&config, target.name.as_str())?
    };
    let mut errors = Vec::new();
    let mut cat_live_col = Vec::with_capacity(1_000);
    let mut cat_series_col = Vec::with_capacity(1_000);
    let mut cat_vod_col = Vec::with_capacity(1_000);
    let mut live_col = Vec::with_capacity(50_000);
    let mut series_col = Vec::with_capacity(50_000);
    let mut vod_col = Vec::with_capacity(50_000);

    // preserve category_ids
    let (max_cat_id, existing_cat_ids) = load_old_category_ids(&path).await;
    let mut cat_id_counter = max_cat_id;
    for plg in playlist.iter_mut() {
        if !&plg.channels.is_empty() {
            let cat_key = format!("{}{}", plg.xtream_cluster, &plg.title);
            let cat_id = existing_cat_ids.get(&cat_key).unwrap_or_else(|| {
                cat_id_counter += 1;
                &cat_id_counter
            });
            plg.id = *cat_id;

            match &plg.xtream_cluster {
                XtreamCluster::Live => &mut cat_live_col,
                XtreamCluster::Series => &mut cat_series_col,
                XtreamCluster::Video => &mut cat_vod_col,
            }.push(json!(CategoryEntry {
                category_id: *cat_id,
                category_name: plg.title.clone(),
                parent_id: 0
            }));

            for pli in &mut plg.channels {
                let header = &mut pli.header;
                header.category_id = *cat_id;
                let col = match header.xtream_cluster {
                    XtreamCluster::Live => &mut live_col,
                    XtreamCluster::Series => &mut series_col,
                    XtreamCluster::Video => &mut vod_col,
                };
                col.push(pli);
            }
        }
    }

    let root_path = path.clone();
    let app_config = app_cfg.clone();
    let write_errors = task::spawn_blocking(move || {
        let mut write_errors = vec![];
        for (col_path, data) in [
            (get_live_cat_collection_path(&root_path), &cat_live_col),
            (get_vod_cat_collection_path(&root_path), &cat_vod_col),
            (get_series_cat_collection_path(&root_path), &cat_series_col),
        ] {
            let lock = app_config.file_locks.write_lock(&col_path);
            match json_write_documents_to_file(&col_path, data) {
                Ok(()) => {}
                Err(err) => {
                    write_errors.push(format!("Persisting collection failed: {}: {err}", col_path.display()));
                }
            }
            drop(lock);
        }
        write_errors
    }).await.map_err(|e| notify_err!(format!("Task panicked: {}", e)))?;
    errors.extend(write_errors);

    match write_playlists_to_file(
        app_cfg,
        &path,
        vec![
            (XtreamCluster::Live, &live_col),
            (XtreamCluster::Video, &vod_col),
            (XtreamCluster::Series, &series_col),
        ],
    ).await {
        Ok(()) => {
            if let Err(err) = xtream_garbage_collect(app_cfg, &target.name).await {
                if err.kind() != ErrorKind::NotFound {
                    errors.push(format!("Garbage collection failed:{err}"));
                }
            }
        }
        Err(err) => {
            errors.push(format!("Persisting collection failed:{err}"));
        }
    }

    if !errors.is_empty() {
        return create_tuliprox_error_result!(
            TuliproxErrorKind::Notify,
            "{}",
            errors.join("\n")
        );
    }

    Ok(())
}

pub fn xtream_get_collection_path(
    cfg: &Config,
    target_name: &str,
    collection_name: &str,
) -> Result<(Option<PathBuf>, Option<String>), Error> {
    if let Some(path) = xtream_get_storage_path(cfg, target_name) {
        let col_path = get_collection_path(&path, collection_name);
        if col_path.exists() {
            return Ok((Some(col_path), None));
        }
    }
    Err(str_to_io_error(&format!("Cant find collection: {target_name}/{collection_name}")))
}

async fn xtream_read_item_for_stream_id(
    cfg: &AppConfig,
    stream_id: u32,
    storage_path: &Path,
    cluster: XtreamCluster,
) -> Result<XtreamPlaylistItem, Error> {
    let xtream_path = xtream_get_file_path(storage_path, cluster);
    {
        let _file_lock = cfg.file_locks.read_lock(&xtream_path).await;
        let mut query = BPlusTreeQuery::<u32, XtreamPlaylistItem>::try_new(&xtream_path)?;
        query.query(&stream_id).ok_or_else(|| Error::new(ErrorKind::NotFound, format!("Item {stream_id} not found in {cluster}")))
    }
}

async fn xtream_read_series_item_for_stream_id(
    cfg: &AppConfig,
    stream_id: u32,
    storage_path: &Path,
) -> Result<XtreamPlaylistItem, Error> {
    let xtream_path = xtream_get_file_path(storage_path, XtreamCluster::Series);
    {
        let _file_lock = cfg.file_locks.read_lock(&xtream_path).await;
        let mut query = BPlusTreeQuery::<u32, XtreamPlaylistItem>::try_new(&xtream_path)?;
        query.query(&stream_id).ok_or_else(|| Error::new(ErrorKind::NotFound, format!("Item {stream_id} not found in series")))
    }
}

macro_rules! try_cluster {
    ($xtream_cluster:expr, $item_type:expr, $virtual_id:expr) => {
        $xtream_cluster
            .or_else(|| XtreamCluster::try_from($item_type).ok())
            .ok_or_else(|| str_to_io_error(&format!("Could not determine cluster for xtream item with stream-id {}",$virtual_id)))
    };
}

async fn xtream_get_item_for_stream_id_from_memory(
    virtual_id: u32,
    app_state: &Arc<AppState>,
    target: &ConfigTarget,
    xtream_cluster: Option<XtreamCluster>,
) -> Result<Option<(XtreamPlaylistItem, VirtualIdRecord)>, Error> {
    if let Some(playlist) = app_state.playlists.data.read().await.get(target.name.as_str()) {
        return match playlist.xtream.as_ref() {
            None => {
                Ok(None)
            }
            Some(xtream_storage) => {
                let mapping = xtream_storage.id_mapping.query(&virtual_id).ok_or_else(|| str_to_io_error(&format!("Could not find mapping for target {} and id {}", target.name, virtual_id)))?.clone();
                let result = match mapping.item_type {
                    PlaylistItemType::SeriesInfo
                    | PlaylistItemType::LocalSeriesInfo => {
                        Ok(xtream_storage.series.query(&mapping.virtual_id)
                            .ok_or_else(|| str_to_io_error(&format!("Failed to read xtream item for id {virtual_id}")))?
                            .clone())
                    }
                    PlaylistItemType::Series
                    | PlaylistItemType::LocalSeries => {
                        if let Some(item) = xtream_storage.series.query(&mapping.parent_virtual_id) {
                            let mut xc_item = item.clone();
                            xc_item.provider_id = mapping.provider_id;
                            xc_item.item_type = PlaylistItemType::Series;
                            xc_item.virtual_id = mapping.virtual_id;
                            Ok(xc_item)
                        } else {
                            Ok(xtream_storage.series.query(&virtual_id)
                                .ok_or_else(|| str_to_io_error(&format!("Failed to read xtream item for id {virtual_id}")))?
                                .clone())
                        }
                    }
                    PlaylistItemType::Catchup => {
                        let cluster = try_cluster!(xtream_cluster, mapping.item_type, virtual_id)?;
                        let item = match cluster {
                            XtreamCluster::Live => xtream_storage.live.query(&mapping.parent_virtual_id),
                            XtreamCluster::Video => xtream_storage.vod.query(&mapping.parent_virtual_id),
                            XtreamCluster::Series => xtream_storage.series.query(&mapping.parent_virtual_id),
                        };

                        if let Some(pl_item) = item {
                            let mut xc_item = pl_item.clone();
                            xc_item.provider_id = mapping.provider_id;
                            xc_item.item_type = PlaylistItemType::Catchup;
                            xc_item.virtual_id = mapping.virtual_id;
                            Ok(xc_item)
                        } else {
                            Err(str_to_io_error(&format!("Failed to read xtream item for id {virtual_id}")))
                        }
                    }
                    _ => {
                        let cluster = try_cluster!(xtream_cluster, mapping.item_type, virtual_id)?;
                        Ok((match cluster {
                            XtreamCluster::Live => xtream_storage.live.query(&virtual_id),
                            XtreamCluster::Video => xtream_storage.vod.query(&virtual_id),
                            XtreamCluster::Series => xtream_storage.series.query(&virtual_id),
                        }).ok_or_else(|| str_to_io_error(&format!("Failed to read xtream item for id {virtual_id}")))?
                            .clone())
                    }
                };

                result.map(|xpli| Some((xpli, mapping)))
            }
        };
    }
    //Err(str_to_io_error(&format!("Failed to read xtream item for id {virtual_id}. No entry found.")))
    Ok(None)
}

pub async fn xtream_get_item_for_stream_id(
    virtual_id: u32,
    app_state: &Arc<AppState>,
    target: &ConfigTarget,
    xtream_cluster: Option<XtreamCluster>,
) -> Result<(XtreamPlaylistItem, VirtualIdRecord), Error> {
    if target.use_memory_cache {
        if let Ok(Some((playlist_item, virtual_record))) =
            xtream_get_item_for_stream_id_from_memory(virtual_id, app_state, target, xtream_cluster).await {
            return Ok((playlist_item, virtual_record));
        }
        // fall through to disk lookup on cache miss
    }

    let app_config: &AppConfig = &app_state.app_config;
    let config = app_config.config.load();
    let target_path = get_target_storage_path(&config, target.name.as_str()).ok_or_else(|| str_to_io_error(&format!("Could not find path for target {}", &target.name)))?;
    let storage_path = xtream_get_storage_path(&config, target.name.as_str()).ok_or_else(|| str_to_io_error(&format!("Could not find path for target {} xtream output", &target.name)))?;
    {
        let target_id_mapping_file = get_target_id_mapping_file(&target_path);
        let _file_lock = app_config.file_locks.read_lock(&target_id_mapping_file).await;

        let mut target_id_mapping = BPlusTreeQuery::<u32, VirtualIdRecord>::try_new(&target_id_mapping_file).map_err(|err| str_to_io_error(&format!("Could not load id mapping for target {} err:{err}", target.name)))?;
        let mapping = target_id_mapping.query(&virtual_id).ok_or_else(|| str_to_io_error(&format!("Could not find mapping for target {} and id {}", target.name, virtual_id)))?;
        let result = match mapping.item_type {
            PlaylistItemType::SeriesInfo
            | PlaylistItemType::LocalSeriesInfo => {
                xtream_read_series_item_for_stream_id(app_config, virtual_id, &storage_path).await
            }
            PlaylistItemType::Series
            | PlaylistItemType::LocalSeries => {
                if let Ok(mut item) = xtream_read_series_item_for_stream_id(app_config, mapping.parent_virtual_id, &storage_path).await {
                    item.provider_id = mapping.provider_id;
                    item.item_type = PlaylistItemType::Series;
                    item.virtual_id = mapping.virtual_id;
                    Ok(item)
                } else {
                    xtream_read_item_for_stream_id(app_config, virtual_id, &storage_path, XtreamCluster::Series).await
                }
            }
            PlaylistItemType::Catchup => {
                let cluster = try_cluster!(xtream_cluster, mapping.item_type, virtual_id)?;
                let mut item = xtream_read_item_for_stream_id(app_config, mapping.parent_virtual_id, &storage_path, cluster).await?;
                item.provider_id = mapping.provider_id;
                item.item_type = PlaylistItemType::Catchup;
                item.virtual_id = mapping.virtual_id;
                Ok(item)
            }
            _ => {
                let cluster = try_cluster!(xtream_cluster, mapping.item_type, virtual_id)?;
                xtream_read_item_for_stream_id(app_config, virtual_id, &storage_path, cluster).await
            }
        };

        result.map(|xpli| (xpli, mapping))
    }
}

pub async fn xtream_load_rewrite_playlist(
    cluster: XtreamCluster,
    config: &AppConfig,
    target: &ConfigTarget,
    category_id: Option<u32>,
    user: &ProxyUserCredentials,
) -> Result<XtreamPlaylistJsonIterator, TuliproxError> {
    XtreamPlaylistJsonIterator::new(cluster, config, target, category_id, user).await
}

pub async fn xtream_write_series_info(
    app_config: &AppConfig,
    target_name: &str,
    series_info_id: u32,
    content: &StreamProperties,
) -> Result<(), Error> {
    if let StreamProperties::Series(_series) = content {
        let config = app_config.config.load();
        let target_path = try_option_ok!(get_target_storage_path(&config, target_name));
        let storage_path = try_option_ok!(xtream_get_storage_path(&config, target_name));
        let info_path = try_option_ok!(xtream_get_info_file_path(
            &storage_path,
            XtreamCluster::Series
        ));

        {
            let _file_lock = app_config.file_locks.write_lock(&info_path).await;
            let mut tree = BPlusTreeUpdate::try_new(&info_path).map_err(|_| str_to_io_error("failed to open series info for update"))?;
            tree.update(&series_info_id, content.clone()).map_err(|_| str_to_io_error("failed to update series info"))?;
        }
        {
            let target_id_mapping_file = get_target_id_mapping_file(&target_path);
            let _file_lock = app_config.file_locks.write_lock(&target_id_mapping_file).await;
            if let Ok(mut target_id_mapping) = BPlusTreeUpdate::<u32, VirtualIdRecord>::try_new(&target_id_mapping_file) {
                if let Some(record) = target_id_mapping.query(&series_info_id) {
                    let new_record = record.copy_update_timestamp();
                    let _ = target_id_mapping.update(&series_info_id, new_record);
                }
            }
        }

        Ok(())
    } else {
        Err(std::io::Error::new(ErrorKind::InvalidData, "No series info data found!".to_string()))
    }
}

pub async fn xtream_write_vod_info(
    app_config: &AppConfig,
    target_name: &str,
    virtual_id: u32,
    content: &StreamProperties,
) -> Result<(), Error> {
    if let StreamProperties::Video(video) = content {
        let config = app_config.config.load();
        let storage_path = try_option_ok!(xtream_get_storage_path(&config, target_name));
        let info_path = try_option_ok!(xtream_get_info_file_path(&storage_path, XtreamCluster::Video));
        {
            let _file_lock = app_config.file_locks.write_lock(&info_path).await;
            let mut tree = BPlusTreeUpdate::try_new(&info_path).map_err(|_| str_to_io_error("failed to open vod info for update"))?;
            tree.update(&virtual_id, StreamProperties::Video(video.clone())).map_err(|_| str_to_io_error("failed to update vod info"))?;
        }
        Ok(())
    } else {
        Err(std::io::Error::new(ErrorKind::InvalidData, "No video info data found!".to_string()))
    }
}

pub async fn xtream_get_input_info(
    cfg: &AppConfig,
    input: &ConfigInput,
    provider_id: u32,
    cluster: XtreamCluster,
) -> Option<StreamProperties> {
    if let Ok(Some(info_path)) = get_input_storage_path(&input.name, &cfg.config.load().working_dir).map(|storage_path| xtream_get_info_file_path(&storage_path, cluster))
    {
        let _file_lock = cfg.file_locks.read_lock(&info_path).await;
        let mut query = BPlusTreeQuery::<u32, StreamProperties>::try_new(&info_path).ok()?;
        return query.query(&provider_id);
    }
    None
}

pub async fn xtream_update_input_info_file(
    cfg: &AppConfig,
    input: &ConfigInput,
    wal_path: &Path,
    cluster: XtreamCluster,
) -> Result<(), TuliproxError> {
    let config = cfg.config.load();
    if let Ok(Some(info_path)) = get_input_storage_path(&input.name, &config.working_dir).map(|storage_path| xtream_get_info_file_path(&storage_path, cluster)) {
        let _file_lock = cfg.file_locks.write_lock(&info_path).await;
        let mut reader = async_file_reader(async_open_readonly_file(wal_path).await.map_err(|err| notify_err!(format!("Could not read {cluster} info {err}")))?);

        let mut tree = if info_path.exists() {
            BPlusTreeUpdate::<u32, String>::try_new(&info_path).map_err(|err| notify_err!(format!("Could not open {cluster} info for update {err}")))?
        } else {
            return Err(notify_err!(format!("BPlusTree file not found for {cluster} info")));
        };

        let mut provider_id_bytes = [0u8; 4];
        let mut length_bytes = [0u8; 4];
        loop {
            if reader.read_exact(&mut provider_id_bytes).await.is_err() {
                break; // End of file
            }
            let provider_id = u32::from_le_bytes(provider_id_bytes);
            reader.read_exact(&mut length_bytes).await.map_err(|err| notify_err!(format!("Could not read temporary {cluster} info {err}")))?;
            let length = u32::from_le_bytes(length_bytes) as usize;
            let mut buffer = vec![0u8; length];
            reader.read_exact(&mut buffer).await.map_err(|err| notify_err!(format!("Could not read temporary {cluster} info {err}")))?;
            if let Ok(content) = String::from_utf8(buffer) {
                let _ = tree.update(&provider_id, content);
            }
        }
        drop(reader);
        if let Err(err) = fs::remove_file(wal_path) {
            error!("Failed to delete WAL file for {cluster} {err}");
        }
        Ok(())
    } else {
        Err(notify_err!(format ! ("Could not determine storage path for input {}", & input.name)))
    }
}

pub async fn xtream_update_input_series_record_from_wal_file(
    cfg: &AppConfig,
    input: &ConfigInput,
    wal_path: &Path,
) -> Result<(), TuliproxError> {
    let config = cfg.config.load();
    let record_path = get_input_storage_path(&input.name, &config.working_dir).map(|storage_path| xtream_get_record_file_path(&storage_path, PlaylistItemType::SeriesInfo))
        .map_err(|err| notify_err!(format!("Error accessing storage path: {err}")))
        .and_then(|opt| opt.ok_or_else(|| notify_err!(format!("Error accessing storage path for input: {}", &input.name))))?;
    {
        let _file_lock = cfg.file_locks.write_lock(&record_path).await;
        let mut reader = async_file_reader(async_open_readonly_file(wal_path).await.map_err(|err| notify_err!(format!("Could not read series wal info {err}")))?);
        let mut provider_id_bytes = [0u8; 4];
        let mut ts_bytes = [0u8; 8];
        let mut tree_record_index: BPlusTree<u32, u64> = BPlusTree::load(&record_path).unwrap_or_else(|_| BPlusTree::new());
        loop {
            if reader.read_exact(&mut provider_id_bytes).await.is_err() {
                break; // End of file
            }
            let provider_id = u32::from_le_bytes(provider_id_bytes);
            if reader.read_exact(&mut ts_bytes).await.is_err() {
                break; // End of file
            }
            let ts = u64::from_le_bytes(ts_bytes);
            tree_record_index.insert(provider_id, ts);
        }
        tree_record_index.store(&record_path).map_err(|err| notify_err!(format!("Could not store series record info {err}")))?;
        drop(reader);
        if let Err(err) = tokio::fs::remove_file(wal_path).await {
            error!("Failed to delete record WAL file for series {err}");
        }
        Ok(())
    }
}

pub async fn iter_raw_xtream_playlist(app_config: &AppConfig, target: &ConfigTarget, cluster: XtreamCluster) -> Option<(FileReadGuard, impl Iterator<Item=(XtreamPlaylistItem, bool)>)> {
    let config = app_config.config.load();
    if let Some(storage_path) = xtream_get_storage_path(&config, target.name.as_str()) {
        let xtream_path = xtream_get_file_path(&storage_path, cluster);
        if !xtream_path.exists() {
            return None;
        }
        let file_lock = app_config.file_locks.read_lock(&xtream_path).await;
        match BPlusTreeQuery::<u32, XtreamPlaylistItem>::try_new(&xtream_path)
            .map_err(|err| info_err!(format!("Could not open BPlusTreeQuery {xtream_path:?} - {err}"))) {
            Ok(mut query) => {
                let items: Vec<XtreamPlaylistItem> = query.iter().map(|(_, v)| v).collect();
                let len = items.len();
                Some((file_lock, items.into_iter().enumerate().map(move |(i, v)| (v, i < len - 1))))
            }
            Err(_) => None
        }
    } else {
        None
    }
}

pub fn playlist_iter_to_stream<I, P>(channels: Option<(FileReadGuard, I)>) -> impl Stream<Item=Result<Bytes, String>>
where
    I: Iterator<Item=(P, bool)> + 'static,
    P: Serialize,
{
    match channels {
        Some((_, chans)) => {
            // Convert iterator items to Result<Bytes, String> with minimal allocations
            let mapped = chans.map(move |(item, has_next)| {
                match serde_json::to_string(&item) {
                    Ok(mut content) => {
                        if has_next { content.push(','); }
                        Ok(Bytes::from(content))
                    }
                    Err(_) => Ok(Bytes::from("")),
                }
            });
            stream::iter(mapped).left_stream()
        }
        None => {
            stream::once(async { Ok(Bytes::from("")) }).right_stream()
        }
    }
}

pub(crate) async fn xtream_get_playlist_categories(config: &Config, target_name: &str, cluster: XtreamCluster) -> Option<Vec<PlaylistXtreamCategory>> {
    let path = xtream_get_collection_path(config, target_name, match cluster {
        XtreamCluster::Live => storage_const::COL_CAT_LIVE,
        XtreamCluster::Video => storage_const::COL_CAT_VOD,
        XtreamCluster::Series => storage_const::COL_CAT_SERIES,
    });
    if let Ok((Some(file_path), _content)) = path {
        if let Ok(content) = tokio::fs::read_to_string(&file_path).await {
            return serde_json::from_str::<Vec<PlaylistXtreamCategory>>(&content).ok();
        }
    }
    None
}

pub async fn write_series_info_to_wal_file(provider_id: u32, ts: u64, content: &str,
                                           content_write: &mut tokio::io::BufWriter<&mut tokio::fs::File>,
                                           record_writer: &mut tokio::io::BufWriter<&mut tokio::fs::File>) -> std::io::Result<()> {
    let encoded_content = encode_info_content_for_wal_file(provider_id, content)?;
    let encoded_record = encode_series_info_record_for_wal_file(provider_id, ts);
    content_write.write_all(&encoded_content).await?;
    record_writer.write_all(&encoded_record).await?;
    Ok(())
}

fn encode_info_content_for_wal_file(provider_id: u32, content: &str) -> std::io::Result<Vec<u8>> {
    let length = u32::try_from(content.len()).map_err(to_io_error)?;
    let mut buffer = Vec::with_capacity(8 + content.len());

    buffer.extend_from_slice(&provider_id.to_le_bytes());
    buffer.extend_from_slice(&length.to_le_bytes());
    buffer.extend_from_slice(content.as_bytes());

    Ok(buffer)
}

fn encode_series_info_record_for_wal_file(provider_id: u32, ts: u64) -> Vec<u8> {
    let mut buffer = Vec::with_capacity(12);
    buffer.extend_from_slice(&provider_id.to_le_bytes());
    buffer.extend_from_slice(&ts.to_le_bytes());
    buffer
}

#[allow(clippy::too_many_lines)]
pub async fn persist_input_xtream_playlist(app_config: &Arc<AppConfig>, storage_path: &Path,
                                           playlist: Vec<PlaylistGroup>) -> (Vec<PlaylistGroup>, Option<TuliproxError>) {
    let mut errors = Vec::new();

    let mut fetched_categories = PlaylistScratch::<Vec<Value>>::new(1_000);
    let mut fetched_scratch = PlaylistScratch::<Vec<PlaylistItem>>::new(50_000);
    let mut stored_scratch = PlaylistScratch::<IndexMap::<u32, PlaylistItem>>::new(50_000);

    // load
    for cluster in XTREAM_CLUSTER {
        let xtream_path = xtream_get_file_path(storage_path, cluster);
        if xtream_path.exists() {
            let file_lock = app_config.file_locks.read_lock(&xtream_path).await;
            let stored_entries = stored_scratch.get_mut(cluster);
            if let Ok(mut query) = BPlusTreeQuery::<u32, PlaylistItem>::try_new(&xtream_path) {
                for (_, doc) in query.iter() {
                    if let Ok(provider_id) = doc.header.id.parse::<u32>() {
                        stored_entries.insert(provider_id, doc);
                    }
                }
            }
            drop(file_lock);
        }
    }

    let mut groups = IndexMap::new();

    for mut plg in playlist {
        if !&plg.channels.is_empty() {
            fetched_categories.get_mut(plg.xtream_cluster).push(json!(CategoryEntry {
                category_id: plg.id,
                category_name: plg.title.clone(),
                parent_id: 0
            }));

            let channels = std::mem::take(&mut plg.channels);
            for mut pli in channels {
                let stored_col = stored_scratch.get_mut(plg.xtream_cluster);
                let fetched_col = fetched_scratch.get_mut(plg.xtream_cluster);

                if let Ok(provider_id) = pli.header.id.parse::<u32>() {
                    if let Some(stored_pli) = stored_col.get_mut(&provider_id) {
                        if let (Some(new_stream_props), Some(old_stream_props)) = (&mut pli.header.additional_properties, stored_pli.header.additional_properties.take()) {
                            if !needs_update_info_details(new_stream_props, &old_stream_props) {
                                match (new_stream_props, old_stream_props) {
                                    (StreamProperties::Video(value_1), StreamProperties::Video(value_2)) => {
                                        value_1.details = value_2.details;
                                    }
                                    (StreamProperties::Series(value_1), StreamProperties::Series(value_2)) => {
                                        value_1.details = value_2.details;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                fetched_col.push(pli);
            }
            groups.insert(plg.id, plg);
        }
    }

    let mut processed_scratch = PlaylistScratch::<Vec<PlaylistItem>>::new(0);
    for xc in XTREAM_CLUSTER {
        processed_scratch.set(xc, if !stored_scratch.is_empty(xc) && fetched_scratch.is_empty(xc) {
            stored_scratch.take(xc).into_values().collect()
        } else {
            fetched_scratch.take(xc)
        });
    }
    drop(stored_scratch);
    drop(fetched_scratch);

    let root_path = storage_path.to_path_buf();
    let app_cfg = app_config.clone();
    let write_errors = task::spawn_blocking(move || {
        let mut write_errors = vec![];
        for cluster in XTREAM_CLUSTER {
            let col_path = match cluster {
                XtreamCluster::Live => get_collection_path(&root_path, storage_const::COL_CAT_LIVE),
                XtreamCluster::Video => get_collection_path(&root_path, storage_const::COL_CAT_VOD),
                XtreamCluster::Series => get_collection_path(&root_path, storage_const::COL_CAT_SERIES),
            };
            let data = fetched_categories.get_mut(cluster);
            let lock = app_cfg.file_locks.write_lock(&col_path);
            if let Err(err) = json_write_documents_to_file(&col_path, data) {
                write_errors.push(format!(
                    "Persisting collection failed: {}: {err}",
                    col_path.display()
                ));
            }
            drop(lock);
        }
        write_errors
    }).await.map_err(|e| notify_err!(format!("Task panicked: {}", e)));

    for cluster in XTREAM_CLUSTER {
        let data = processed_scratch.get(cluster);
        match write_playlists_to_file_2(
            app_config,
            storage_path,
            cluster,
            data,
        ).await {
            Ok(()) => {

                // TODO GARBAGE collect

                // if let Err(err) = xtream_garbage_collect(app_config, &target.name).await {
                //     if err.kind() != ErrorKind::NotFound {
                //         errors.push(format!("Garbage collection failed:{err}"));
                //     }
                // }
            }
            Err(err) => {
                errors.push(format!("Persisting collection failed:{err}"));
            }
        }
    }

    match write_errors {
        Ok(write_err) => errors.extend(write_err),
        Err(err) => errors.push(err.to_string()),
    }

    for xc in XTREAM_CLUSTER {
        let col = processed_scratch.take(xc);
        for item in col {
            groups
                .entry(item.header.category_id)
                .or_insert_with(|| PlaylistGroup {
                    id: item.header.category_id,
                    title: item.header.group.clone(),
                    channels: Vec::new(),
                    xtream_cluster: item.header.xtream_cluster,
                })
                .channels
                .push(item);
        }
    }

    let result = groups.into_iter().map(|(_, group)| group).collect();

    let err = if errors.is_empty() {
        None
    } else {
        Some(create_tuliprox_error!(TuliproxErrorKind::Notify, "{}", errors.join("\n")))
    };

    (result, err)
}

// Checks if the info has changed after the last update
fn needs_update_info_details(
    new_stream_props: &StreamProperties,
    old_stream_props: &StreamProperties,
) -> bool {
    let new_modified = new_stream_props.get_last_modified();
    let old_modified = old_stream_props.get_last_modified();

    match (new_modified, old_modified) {
        (Some(new_ts), Some(old_ts)) => new_ts > old_ts,
        (None, Some(_)) => true,
        _ => false,
    }
}

async fn persist_input_info<T>(app_config: &Arc<AppConfig>, storage_path: &Path, cluster: XtreamCluster,
                               input_name: &str, provider_id: u32, props: T) -> Result<(), Error>
where
    T: serde::Serialize + for<'de> serde::Deserialize<'de> + Clone,
{
    let xtream_path = xtream_get_file_path(storage_path, cluster);
    if xtream_path.exists() {
        {
            let _file_lock = app_config.file_locks.write_lock(&xtream_path).await;
            let mut tree = BPlusTreeUpdate::try_new(&xtream_path).map_err(|err| Error::other(format!("failed to open BPlusTree for input {input_name}: {err}")))?;
            tree.update(&provider_id, props).map_err(|err| Error::other(format!("failed to write {cluster} info for input {input_name}: {err}")))?;
        }
    }
    Ok(())
}

pub async fn persists_input_vod_info(app_config: &Arc<AppConfig>, storage_path: &Path,
                                     cluster: XtreamCluster, input_name: &str, provider_id: u32,
                                     props: &VideoStreamProperties) -> Result<(), Error> {
    persist_input_info::<VideoStreamProperties>(app_config, storage_path, cluster, input_name, provider_id, props.clone()).await
}

pub async fn persists_input_series_info(app_config: &Arc<AppConfig>, storage_path: &Path,
                                        cluster: XtreamCluster, input_name: &str, provider_id: u32,
                                        props: &SeriesStreamProperties) -> Result<(), Error> {
    persist_input_info::<SeriesStreamProperties>(app_config, storage_path, cluster, input_name, provider_id, props.clone()).await
}