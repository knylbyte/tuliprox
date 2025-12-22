use crate::model::FetchedPlaylist;
use crate::model::{AppConfig, ConfigTarget};
use crate::processing::processor::xtream::{create_resolve_info_wal_files, playlist_resolve_download_playlist_item, read_processed_info_ids, should_update_info};
use crate::processing::processor::{create_resolve_options_function_for_xtream_target, handle_error, handle_error_and_return};
use crate::repository::xtream_repository::{write_vod_info_to_wal_file, xtream_update_input_info_file, xtream_update_input_vod_record_from_wal_file, InputVodInfoRecord};
use crate::utils;
use crate::utils::IO_BUFFER_SIZE;
use log::{info, log_enabled, Level};
use shared::error::notify_err;
use shared::error::TuliproxError;
use shared::model::{InputType, PlaylistEntry, StreamProperties, VideoStreamProperties, XtreamVideoInfo};
use shared::model::{PlaylistItemType, XtreamCluster};
use std::collections::{HashMap, HashSet};
use std::time::Instant;
use tokio::io::AsyncWriteExt;

create_resolve_options_function_for_xtream_target!(vod);

async fn read_processed_vod_info_ids(cfg: &AppConfig, errors: &mut Vec<TuliproxError>, fpl: &FetchedPlaylist<'_>) -> HashMap<u32, u64> {
    read_processed_info_ids(cfg, errors, fpl, PlaylistItemType::Video, |record: &InputVodInfoRecord| record.ts).await
}

pub async fn playlist_resolve_vod(app_config: &AppConfig, client: &reqwest::Client, target: &ConfigTarget, errors: &mut Vec<TuliproxError>, fpl: &mut FetchedPlaylist<'_>) {
    let (resolve_movies, resolve_delay) = get_resolve_vod_options(target, fpl);
    if !resolve_movies { return; }

    // TODO read existing WAL File and import it to avoid duplicate requests

    // we cant write to the indexed-document directly because of the write lock and time-consuming operation.
    // All readers would be waiting for the lock and the app would be unresponsive.
    // We collect the content into a wal file and write it once we collected everything.
    let config = app_config.config.load();
    let Some((mut wal_content_file, mut wal_record_file, wal_content_path, wal_record_path))
        = create_resolve_info_wal_files(&config, fpl.input, XtreamCluster::Video).await
    else { return; };

    let mut processed_info_ids: HashMap<u32, u64> = read_processed_vod_info_ids(app_config, errors, fpl).await;
    let mut fetched_in_run: HashSet<u32> = HashSet::new();
    let mut content_writer = utils::async_file_writer(&mut wal_content_file);
    let mut record_writer = utils::async_file_writer(&mut wal_record_file);
    let mut content_updated = false;

    // LocalVideo entries are not resolved!

    // TODO merge both filters to one
    let vod_info_count = fpl.playlistgroups.iter()
        .flat_map(|plg| &plg.channels)
        .filter(|&pli| pli.header.xtream_cluster == XtreamCluster::Video && pli.header.item_type == PlaylistItemType::Video).count();

    info!("Found {vod_info_count} vod info to resolve");
    let mut last_log_time = Instant::now();
    let mut processed_vod_info_count = 0;
    let mut write_counter = 0usize;

    for plg in &mut fpl.playlistgroups {
        for pli in &mut plg.channels {
            // LocalVideo files are not resolved
            if pli.header.item_type != PlaylistItemType::Video {
                continue;
            }
            let (should_update, provider_id, _ts) = should_update_info(pli, &processed_info_ids);
            if should_update && provider_id != 0 && fetched_in_run.insert(provider_id) {
                if pli.get_item_type().is_local() {
                    continue;
                }
                if let Some(content) = playlist_resolve_download_playlist_item(client, pli, fpl.input, errors, resolve_delay, XtreamCluster::Video).await {
                    if let Ok(info) = serde_json::from_str::<XtreamVideoInfo>(&content) {
                        let provider_id = info.movie_data.stream_id;
                        let added = info.movie_data.added.parse::<u64>().unwrap_or(0);
                        let tmdb_id = info.info.tmdb_id.parse::<u32>().unwrap_or(0);
                        let info_record = InputVodInfoRecord {
                            tmdb_id,
                            ts: added,
                            release_date: info.info.releasedate.clone(),
                        };

                        let video_stream_props = VideoStreamProperties::from_info(&info, pli);
                        handle_error_and_return!(write_vod_info_to_wal_file(provider_id, &serde_json::to_string(&video_stream_props).unwrap_or_else(|_|String::new()),
                            &info_record, &mut content_writer, &mut record_writer).await,
                            |err| errors.push(notify_err!(format!("Failed to resolve vod, could not write to wal file {err}"))));
                        processed_info_ids.insert(provider_id, added);
                        content_updated = true;
                        write_counter += content.len();
                        // periodic flush to bound BufWriter memory
                        if write_counter >= IO_BUFFER_SIZE {
                            write_counter = 0;
                            if let Err(err) = content_writer.flush().await {
                                errors.push(notify_err!(format!("Failed periodic flush of wal content writer {err}")));
                            }
                            if let Err(err) = record_writer.flush().await {
                                errors.push(notify_err!(format!("Failed periodic flush of wal record writer {err}")));
                            }
                        }

                        // Update in-memory playlist items with the newly fetched vod info.
                        // This makes the data available for subsequent processing steps like STRM export.
                        pli.header.additional_properties = Some(StreamProperties::Video(Box::new(video_stream_props)));
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
        handle_error!(content_writer.flush().await,
            |err| errors.push(notify_err!(format!("Failed to resolve vod, could not write to wal file {err}"))));
        handle_error!(record_writer.flush().await,
            |err| errors.push(notify_err!(format!("Failed to resolve vod tmdb, could not write to wal file {err}"))));
        handle_error!(content_writer.get_ref().sync_all().await, |err| errors.push(notify_err!(format!("Failed to sync vod info to wal file {err}"))));
        handle_error!(record_writer.get_ref().sync_all().await, |err| errors.push(notify_err!(format!("Failed to sync vod info record to wal file {err}"))));
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
