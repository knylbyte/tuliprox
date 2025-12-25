use crate::model::{AppConfig, ConfigTarget};
use crate::model::FetchedPlaylist;
use crate::processing::parser::xtream::parse_xtream_series_info;
use crate::processing::processor::playlist::ProcessingPipe;
use crate::processing::processor::xtream::{playlist_resolve_download_playlist_item};
use crate::processing::processor::{create_resolve_options_function_for_xtream_target};
use log::{error, info, log_enabled, Level};
use shared::error::TuliproxError;
use shared::model::{InputType, PlaylistEntry, SeriesStreamProperties, StreamProperties, XtreamSeriesInfo};
use shared::model::{PlaylistGroup, PlaylistItemType, XtreamCluster};
use std::sync::Arc;
use std::time::Instant;
use crate::repository::storage::get_input_storage_path;
use crate::repository::xtream_repository::persists_input_series_info;

create_resolve_options_function_for_xtream_target!(series);

async fn playlist_resolve_series_info(app_config: &Arc<AppConfig>, client: &reqwest::Client, errors: &mut Vec<TuliproxError>,
                                      fpl: &mut FetchedPlaylist<'_>, resolve_delay: u16) -> Vec<PlaylistGroup> {

    let input = fpl.input;
    let working_dir = &app_config.config.load().working_dir;
    let storage_path = match get_input_storage_path(&input.name, working_dir) {
        Ok(storage_path) => storage_path,
        Err(err) => {
            error!("Can't resolve vod, input storage directory for input '{}' failed: {err}", input.name);
            return vec![];
        }
    };

    let series_info_count = fpl.playlistgroups.iter()
        .flat_map(|plg| &plg.channels)
        .filter(|&pli| pli.header.xtream_cluster == XtreamCluster::Series
            && pli.header.item_type == PlaylistItemType::SeriesInfo
            && !pli.has_details()).count();

    info!("Found {series_info_count} series info to resolve");
    let mut last_log_time = Instant::now();
    let mut processed_series_info_count = 0;
    let input = fpl.input;
    let mut result: Vec<PlaylistGroup> = vec![];

    for plg in &mut fpl.playlistgroups {
        let mut group_series = vec![];
        for pli in &mut plg.channels {
            processed_series_info_count += 1;
            if pli.header.xtream_cluster != XtreamCluster::Series
                || pli.header.item_type != PlaylistItemType::SeriesInfo
                || pli.has_details() {
                continue;
            }
            let Some(provider_id) = pli.get_provider_id() else { continue; };
            let (group, series_name) = {
                let header = &pli.header;
                (header.group.clone(), if header.name.is_empty() { header.title.clone() } else { header.name.clone() })
            };
            if provider_id != 0 {
                if let Some(content) = playlist_resolve_download_playlist_item(client, pli, fpl.input, errors, resolve_delay, XtreamCluster::Series).await {
                    if let Ok(info) = serde_json::from_str::<XtreamSeriesInfo>(&content) {
                        let series_stream_props = SeriesStreamProperties::from_info(&info, pli);
                        // the input db needs to be updated
                        let _ = persists_input_series_info(app_config, &storage_path, pli.header.xtream_cluster, &input.name, provider_id, &series_stream_props).await;
                        // extract episodes from info
                        if let Some(episodes) = parse_xtream_series_info(&pli.get_uuid(), &series_stream_props, &group, &series_name, input) {
                            group_series.extend(episodes.into_iter());
                        }

                        // Update in-memory playlist items with the newly fetched vod info.
                        // This makes the data available for subsequent processing steps like STRM export.
                        pli.header.additional_properties = Some(StreamProperties::Series(Box::new(series_stream_props)));
                    }
                }
            }
            if log_enabled!(Level::Info)
                && last_log_time.elapsed().as_secs() >= 30 {
                    info!("resolved {processed_series_info_count}/{series_info_count} series info");
                    last_log_time = Instant::now();
                }
        }
        if !group_series.is_empty() {
            result.push(PlaylistGroup {
                id: plg.id,
                title: plg.title.clone(),
                channels: group_series,
                xtream_cluster: XtreamCluster::Series,
            });
        }
    }
    info!("resolved {processed_series_info_count}/{series_info_count} series info");
    result
}
//
// async fn process_series_info(
//     app_config: &AppConfig,
//     fpl: &mut FetchedPlaylist<'_>,
//     errors: &mut Vec<TuliproxError>,
// ) -> Vec<PlaylistGroup> {
//     let mut result: Vec<PlaylistGroup> = vec![];
//     let input = fpl.input;
//     let config = app_config.config.load();
//     let Ok(Some((info_path, idx_path))) = get_input_storage_path(&input.name, &config.working_dir)
//         .map(|storage_path| xtream_get_info_file_paths(&storage_path, XtreamCluster::Series))
//     else {
//         errors.push(notify_err!("Failed to open input info file for series".to_string()));
//         return result;
//     };
//
//     let mut write_counter = 0usize;
//
//     let _file_lock = app_config.file_locks.read_lock(&info_path).await;
//
//     // Contains the Series Info with episode listing
//     let Ok(mut info_reader) = IndexedDocumentReader::<u32, String>::new(&info_path, &idx_path) else { return result; };
//
//     let Some((mut wal_file, wal_path)) = create_resolve_episode_wal_files(&config, input).await else {
//         errors.push(notify_err!("Could not create wal file for series episodes record".to_string()));
//         return result;
//     };
//     let mut wal_writer = utils::async_file_writer(&mut wal_file);
//
//     for plg in fpl
//         .playlistgroups
//         .iter_mut()
//         .filter(|plg| plg.xtream_cluster == XtreamCluster::Series)
//     {
//         let mut group_series = vec![];
//
//         // Resolve does not handle LocalSeriesInfo
//         for pli in plg
//             .channels
//             .iter_mut()
//             .filter(|pli| pli.header.item_type == PlaylistItemType::SeriesInfo)
//         {
//             let Some(provider_id) = pli.header.get_provider_id() else { continue; };
//             let Ok(content) = info_reader.get(&provider_id)  else { continue; };
//             if content.is_empty() {
//                 warn!("Series info content is empty, skipping series with provider id: {provider_id}");
//                 continue;
//             }
//             match serde_json::from_str::<SeriesStreamProperties>(&content) {
//                 Ok(series_content) => {
//                     let (group, series_name) = {
//                         let header = &pli.header;
//                         (header.group.clone(), if header.name.is_empty() { header.title.clone() } else { header.name.clone() })
//                     };
//                     if let Some(mut series) = parse_xtream_series_info(&series_content, &group, &series_name, input) {
//                         for pli_episode in &mut series {
//                             let Some(provider_id) = &pli_episode.header.get_provider_id() else { continue; };
//                             match write_series_episode_record_to_wal_file(&mut wal_writer, *provider_id, pli.header.additional_properties.as_ref()).await {
//                                 Ok(written_bytes) => {
//                                     write_counter += written_bytes;
//                                     // periodic flush to bound BufWriter memory
//                                     if write_counter >= IO_BUFFER_SIZE {
//                                         write_counter = 0;
//                                         if let Err(err) = wal_writer.flush().await {
//                                             errors.push(notify_err!(format!("Failed periodic flush of wal content writer {err}")));
//                                         }
//                                     }
//                                 }
//                                 Err(err) => { errors.push(info_err!(format!("Failed to write to series episode wal file: {err}"))) }
//                             }
//                         }
//                         group_series.extend(series.into_iter());
//                     }
//                 }
//                 Err(err) => errors.push(info_err!(format!("Failed to parse JSON: {err}"))),
//             }
//         }
//         if !group_series.is_empty() {
//             result.push(PlaylistGroup {
//                 id: plg.id,
//                 title: plg.title.clone(),
//                 channels: group_series,
//                 xtream_cluster: XtreamCluster::Series,
//             });
//         }
//     }
//
//     handle_error!(wal_writer.flush().await, |err| errors.push(notify_err!(format!("Failed to resolve series episodes, could not write to wal file {err}"))));
//     handle_error!(wal_writer.get_ref().sync_all().await, |err| errors.push(notify_err!(format!("Failed to sync series info to wal file {err}"))));
//
//     drop(wal_writer);
//     drop(wal_file);
//     handle_error!(xtream_update_input_series_episodes_record_from_wal_file(app_config, input, &wal_path).await,
//             |err| errors.push(err));
//     result
// }

pub async fn playlist_resolve_series(cfg: &Arc<AppConfig>,
                                     client: &reqwest::Client,
                                     target: &ConfigTarget,
                                     errors: &mut Vec<TuliproxError>,
                                     pipe: &ProcessingPipe,
                                     provider_fpl: &mut FetchedPlaylist<'_>,
                                     processed_fpl: &mut FetchedPlaylist<'_>,
) {
    let (resolve_series, resolve_delay) = get_resolve_series_options(target, processed_fpl);
    if !resolve_series { return; }

    let series_playlist = playlist_resolve_series_info(cfg, client, errors, processed_fpl, resolve_delay).await;
    if series_playlist.is_empty() { return; }
    // original content saved into original list
    for plg in &series_playlist {
        provider_fpl.update_playlist(plg);
    }
    // run processing pipe over new items
    let mut new_playlist = series_playlist;
    for f in pipe {
        if let Some(v) = f(&mut new_playlist, target) {
            new_playlist = v;
        }
    }
    // assign new items to the new playlist
    for plg in &new_playlist {
        processed_fpl.update_playlist(plg);
    }
}
