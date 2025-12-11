use shared::model::InputType;
use shared::error::{TuliproxError};
use crate::model::{AppConfig, ConfigTarget};
use crate::model::{FetchedPlaylist};
use shared::model::{PlaylistItem, PlaylistItemType, XtreamCluster};
use crate::processing::processor::xtream::{create_resolve_info_wal_files, playlist_resolve_download_playlist_item, read_processed_info_ids, should_update_info};
use crate::repository::xtream_repository::{write_vod_info_to_wal_file, xtream_update_input_info_file, xtream_update_input_vod_record_from_wal_file, InputVodInfoRecord};
use shared::error::{notify_err};
use crate::processing::processor::{handle_error, handle_error_and_return, create_resolve_options_function_for_xtream_target};
use shared::utils::{get_u32_from_serde_value, get_u64_from_serde_value, get_string_from_serde_value};
use serde_json::{from_str, Map, Value};
use std::collections::{HashMap, HashSet};
use std::io::{Write};
use std::time::Instant;
use log::{info, log_enabled, Level};
use crate::utils;
use crate::processing::processor::xtream::normalize_json_content;

create_resolve_options_function_for_xtream_target!(vod);

async fn read_processed_vod_info_ids(cfg: &AppConfig, errors: &mut Vec<TuliproxError>, fpl: &FetchedPlaylist<'_>) -> HashMap<u32, u64> {
    read_processed_info_ids(cfg, errors, fpl, PlaylistItemType::Video, |record: &InputVodInfoRecord| record.ts).await
}

fn extract_info_record_from_vod_info(content: &str) -> Option<(u32, InputVodInfoRecord)> {
    let doc = serde_json::from_str::<Map<String, Value>>(content).ok()?;

    let movie_data = doc.get(crate::model::XC_TAG_VOD_INFO_MOVIE_DATA)?.as_object()?;
    let provider_id = get_u32_from_serde_value(
        movie_data.get(crate::model::XC_TAG_VOD_INFO_STREAM_ID)?,
    )?;

    let added = movie_data
        .get(crate::model::XC_TAG_VOD_INFO_ADDED)
        .and_then(get_u64_from_serde_value)
        .unwrap_or(0);

    let info_section = doc.get(crate::model::XC_TAG_VOD_INFO_INFO)?.as_object()?;

    let tmdb_id = info_section
        .get(crate::model::XC_TAG_VOD_INFO_TMDB_ID)
        .and_then(get_u32_from_serde_value)
        .filter(|&id| id != 0)
        .or_else(|| {
            info_section
                .get(crate::model::XC_TAG_VOD_INFO_TMDB)
                .and_then(get_u32_from_serde_value)
        })
        .unwrap_or(0);

    let release_date = info_section
        .get(crate::model::XC_TAG_VOD_INFO_RELEASEDATE)
        .and_then(get_string_from_serde_value);

    Some((provider_id, InputVodInfoRecord {
        tmdb_id,
        ts: added,
        release_date,
    }))
}

fn should_update_vod_info(pli: &mut PlaylistItem, processed_provider_ids: &HashMap<u32, u64>) -> (bool, u32, u64) {
    should_update_info(pli, processed_provider_ids, crate::model::XC_TAG_VOD_INFO_ADDED)
}

const FLUSH_INTERVAL: usize = 50;

pub async fn playlist_resolve_vod(app_config: &AppConfig, client: &reqwest::Client, target: &ConfigTarget, errors: &mut Vec<TuliproxError>, fpl: &mut FetchedPlaylist<'_>) {
    let (resolve_movies, resolve_delay) = get_resolve_vod_options(target, fpl);
    if !resolve_movies { return; }

    // we cant write to the indexed-document directly because of the write lock and time-consuming operation.
    // All readers would be waiting for the lock and the app would be unresponsive.
    // We collect the content into a wal file and write it once we collected everything.
    let config = app_config.config.load();
    let Some((wal_content_file, wal_record_file, wal_content_path, wal_record_path)) = create_resolve_info_wal_files(&config, fpl.input, XtreamCluster::Video)
    else { return; };

    let mut processed_info_ids: HashMap<u32, u64> = read_processed_vod_info_ids(app_config, errors, fpl).await;
    let mut fetched_in_run: HashSet<u32> = HashSet::new();
    let mut content_writer = utils::file_writer(&wal_content_file);
    let mut record_writer = utils::file_writer(&wal_record_file);
    let mut content_updated = false;

    // TODO merge both filters to one
    let vod_info_count = fpl.playlistgroups.iter()
        .flat_map(|plg| &plg.channels)
        .filter(|&pli| pli.header.xtream_cluster == XtreamCluster::Video).count();

    info!("Found {vod_info_count} vod info to resolve");
    let mut last_log_time = Instant::now();
    let mut processed_vod_info_count = 0;
    let mut write_counter = 0usize;

    for plg in &mut fpl.playlistgroups {
        for pli in &mut plg.channels {
            if pli.header.xtream_cluster != XtreamCluster::Video {
                continue;
            }
            let (should_update, provider_id, _ts) = should_update_vod_info(pli, &processed_info_ids);
            if should_update && provider_id != 0 && fetched_in_run.insert(provider_id) {
                if let Some(content) = playlist_resolve_download_playlist_item(client, pli, fpl.input, errors, resolve_delay, XtreamCluster::Video).await {
                    let normalized_content = normalize_json_content(content);
                    let normalized_str: &str = &normalized_content;
                    if let Some((provider_id, info_record)) = extract_info_record_from_vod_info(normalized_str) {
                        let ts = info_record.ts;
                        handle_error_and_return!(write_vod_info_to_wal_file(provider_id, normalized_str, &info_record, &mut content_writer, &mut record_writer),
                            |err| errors.push(notify_err!(format!("Failed to resolve vod, could not write to wal file {err}"))));
                        processed_info_ids.insert(provider_id, ts);
                        content_updated = true;
                        write_counter += 1;
                        // periodic flush to bound BufWriter memory
                        if write_counter >= FLUSH_INTERVAL {
                            write_counter = 0;
                            if let Err(err) = content_writer.flush() {
                                errors.push(notify_err!(format!("Failed periodic flush of wal content writer {err}")));
                            }
                            if let Err(err) = record_writer.flush() {
                                errors.push(notify_err!(format!("Failed periodic flush of wal record writer {err}")));
                            }
                        }

                        // Update in-memory playlist items with the newly fetched vod info.
                        // This makes the data available for subsequent processing steps like STRM export.
                        pli.header.additional_properties = from_str::<Map<String, Value>>(normalized_str).ok().and_then(|info_doc| {
                            info_doc.get("info").cloned().map(|info_content| {
                                let mut wrapped_info = Map::new();
                                wrapped_info.insert("info".to_string(), info_content);
                                Value::Object(wrapped_info)
                            })
                        });
                    }
                }
            }
            if log_enabled!(Level::Info) {
                processed_vod_info_count += 1;
                if last_log_time.elapsed().as_secs() >= 30 {
                    info!("resolved {processed_vod_info_count}/{vod_info_count} vod info");
                    last_log_time = Instant::now();
                }
            }
        }
    }
    info!("resolved {processed_vod_info_count}/{vod_info_count} vod info");
    if content_updated {
        // TODO better approach for transactional updates is multiplexed WAL file.
        // final flush & sync with proper error handling
        handle_error!(content_writer.flush(),
            |err| errors.push(notify_err!(format!("Failed to resolve vod, could not write to wal file {err}"))));
        handle_error!(record_writer.flush(),
            |err| errors.push(notify_err!(format!("Failed to resolve vod tmdb, could not write to wal file {err}"))));
        handle_error!(content_writer.get_ref().sync_all(), |err| errors.push(notify_err!(format!("Failed to sync vod info to wal file {err}"))));
        handle_error!(record_writer.get_ref().sync_all(), |err| errors.push(notify_err!(format!("Failed to sync vod info record to wal file {err}"))));
        // drop writers and files to release handles
        drop(content_writer);
        drop(record_writer);
        drop(wal_content_file);
        drop(wal_record_file);

        handle_error!(xtream_update_input_info_file(app_config, fpl.input, &wal_content_path, XtreamCluster::Video).await,
            |err| errors.push(err));
        handle_error!(xtream_update_input_vod_record_from_wal_file(app_config, fpl.input, &wal_record_path).await,
            |err| errors.push(err));
    }
}
