use crate::library::{MediaMetadata, MetadataAsyncIter, MetadataCacheEntry};
use crate::model::{AppConfig, ConfigInput, XtreamSeriesInfo, XtreamSeriesInfoEpisode, XtreamSeriesInfoEpisodeInfo, XtreamSeriesInfoInfo};
use crate::model::{XtreamMovieData, XtreamMovieInfo, XtreamMovieInfoDetails};
use serde_json::value::RawValue;
use shared::error::TuliproxError;
use shared::model::{PlaylistGroup, PlaylistItem, PlaylistItemHeader, PlaylistItemType, XtreamCluster};
use shared::utils::{generate_playlist_uuid, string_to_uuid_type};
use std::path::Path;
use std::sync::Arc;

pub async fn get_library_playlist(_client: &reqwest::Client, app_config: &Arc<AppConfig>, input: &Arc<ConfigInput>) -> (Vec<PlaylistGroup>, Vec<TuliproxError>) {
    let config = &*app_config.config.load();
    let Some(library_config) = config.library.as_ref() else { return (vec![], vec![]) };
    if !library_config.enabled { return (vec![], vec![]); }

    let storage_path = std::path::PathBuf::from(&library_config.metadata.path);
    let mut metadata_iter = MetadataAsyncIter::new(&storage_path).await;
    let mut group_movies = PlaylistGroup {
        id: 0,
        title: library_config.playlist.movie_category.clone(),
        channels: vec![],
        xtream_cluster: XtreamCluster::Video,
    };
    let mut group_series = PlaylistGroup {
        id: 0,
        title: library_config.playlist.series_category.clone(),
        channels: vec![],
        xtream_cluster: XtreamCluster::Series,
    };
    while let Some(entry) = metadata_iter.next().await {
        match entry.metadata {
            MediaMetadata::Movie(_) => {
                let pli = to_playlist_item(&entry, &input.name, &library_config.playlist.movie_category);
                group_movies.channels.extend(pli);
            }
            MediaMetadata::Series(_) => {
                let pli = to_playlist_item(&entry, &input.name, &library_config.playlist.series_category);
                group_series.channels.extend(pli);
            }
        }
    }

    let mut groups = vec![];
    if !group_movies.channels.is_empty() {
        groups.push(group_movies);
    }
    if !group_series.channels.is_empty() {
        groups.push(group_series);
    }

    (groups, vec![])
}

fn to_playlist_item(entry: &MetadataCacheEntry, input_name: &str, group_name: &str) -> Vec<PlaylistItem> {
    let metadata = &entry.metadata;

    match metadata {
        MediaMetadata::Movie(_) => {
            let additional_properties = metadata_cache_entry_to_xtream_movie_info(entry).and_then(|info| {
                let raw = serde_json::to_string(&info).ok()?;
                RawValue::from_string(raw).ok()
            });
            vec![PlaylistItem {
                header: PlaylistItemHeader {
                    uuid: string_to_uuid_type(&entry.uuid),
                    name: metadata.title().to_string(),
                    group: group_name.to_string(),
                    title: metadata.title().to_string(),
                    url: format!("file://{}", entry.file_path),
                    xtream_cluster: XtreamCluster::Video,
                    additional_properties,
                    item_type: PlaylistItemType::LocalVideo,
                    input_name: input_name.to_string(),
                    ..PlaylistItemHeader::default()
                }
            }]
        }
        MediaMetadata::Series(_series) => {
            let mut items = vec![];
            if let Some(xtream_series_info) = metadata_cache_entry_to_xtream_series_info(entry) {
                let additional_properties = serde_json::to_string(&xtream_series_info).ok().and_then(|r| RawValue::from_string(r).ok());

                let series_info = PlaylistItem {
                    header: PlaylistItemHeader {
                        uuid: string_to_uuid_type(&entry.uuid),
                        id: entry.uuid.clone(),
                        name: metadata.title().to_string(),
                        group: group_name.to_string(),
                        title: metadata.title().to_string(),
                        url: format!("file://{}", entry.file_path),
                        xtream_cluster: XtreamCluster::Series,
                        item_type: PlaylistItemType::LocalSeriesInfo,
                        input_name: input_name.to_string(),
                        additional_properties,
                        ..PlaylistItemHeader::default()
                    }
                };

                items.push(series_info);
                if let Some(episodes) = xtream_series_info.episodes.as_ref() {
                    for episode in episodes {
                        items.push(PlaylistItem {
                            header: PlaylistItemHeader {
                                id: episode.id.to_string(),
                                // we use parent_code for local series to find the parent series info and straighten the virtual_ids
                                parent_code: entry.uuid.clone(),
                                uuid: generate_playlist_uuid(input_name, &episode.id.to_string(), PlaylistItemType::LocalSeries, &episode.direct_source),
                                name: episode.title.clone(),
                                group: group_name.to_string(),
                                title: episode.title.clone(),
                                url: episode.direct_source.clone(),
                                xtream_cluster: XtreamCluster::Series,
                                item_type: PlaylistItemType::LocalSeries,
                                category_id: 0,
                                input_name: input_name.to_string(),
                                additional_properties: episode.get_additional_properties(&xtream_series_info),
                                ..Default::default()
                            }
                        });
                    }
                }
            }

            items
        }
    }
}

pub fn metadata_cache_entry_to_xtream_movie_info(
    entry: &MetadataCacheEntry,
) -> Option<XtreamMovieInfo> {
    let movie = match &entry.metadata {
        MediaMetadata::Movie(m) => m,
        MediaMetadata::Series(_) => return None,
    };

    let container_extension = Path::new(&entry.file_path)
        .extension()
        .and_then(|s| s.to_str())
        .map(ToString::to_string);

    let actor_names = movie.actors.as_ref().map(|a| a.iter().map(|a| a.name.clone()).collect::<Vec<_>>().join(", "));

    let info = XtreamMovieInfoDetails {
        kinopoisk_url: movie.tmdb_id.map(|id| format!("https://www.themoviedb.org/movie/{id}")),
        tmdb_id: movie.tmdb_id.map(|id| id.to_string()),
        name: Some(movie.title.clone()),
        o_name: movie.original_title.clone(),
        cover_big: movie.poster.clone(),
        movie_image: movie.poster.clone(),
        releasedate: movie.year.map(|y| format!("{y}-01-01")),
        episode_run_time: movie.runtime,
        director: movie.directors.as_ref().map(|d| d.join(", ")),
        youtube_trailer: movie.videos.as_ref().and_then(|v| v.iter().find(|video| video.site.eq_ignore_ascii_case("youtube")).map(|video| video.key.clone())),
        actors: actor_names.clone(),
        cast: actor_names,
        genre: movie.genres.as_ref().map(|g| g.join(", ")),
        description: movie.plot.clone(),
        plot: movie.plot.clone(),
        age: None,
        mpaa_rating: movie.mpaa.clone(),
        rating_count_kinopoisk: 0,
        country: None,
        backdrop_path: movie
            .fanart
            .as_ref()
            .filter(|s| !s.is_empty())
            .map(|f| vec![f.clone()])
            .or_else(|| {
                movie.poster
                    .as_ref()
                    .filter(|s| !s.is_empty())
                    .map(|p| vec![p.clone()])
            })
            .unwrap_or_default(),
        duration_secs: movie.runtime.map(|r| (r * 60).to_string()),
        duration: movie.runtime.map(|r| {
            let h = r / 60;
            let m = r % 60;
            format!("{h:02}:{m:02}:00")
        }),

        video: Vec::new(),
        audio: Vec::new(),
        bitrate: 0,
        rating: movie.rating.map(|r| format!("{r:.2}")),
        runtime: movie.runtime.map(|r| (r * 60).to_string()),
        status: Some("Released".to_string()),
    };

    let movie_data = XtreamMovieData {
        stream_id: 0,
        name: movie.title.clone(),
        added: Some(entry.file_modified.to_string()),
        category_id: 0,
        category_ids: vec![0],
        container_extension,
        custom_sid: None,
        direct_source: String::new(),
    };

    Some(XtreamMovieInfo { info, movie_data })
}

pub fn metadata_cache_entry_to_xtream_series_info(
    entry: &MetadataCacheEntry,
) -> Option<XtreamSeriesInfo> {
    let series = match &entry.metadata {
        MediaMetadata::Movie(_) => return None,
        MediaMetadata::Series(m) => m,
    };

    let actor_names = series.actors.as_ref().map(|a| a.iter().map(|a| a.name.clone()).collect::<Vec<_>>().join(", ")).unwrap_or_default();
    let release_date = series.year.map(|y| format!("{y}-01-01")).unwrap_or_default();
    let youtube_trailer = series.videos.as_ref().and_then(|v| v.iter().find(|video| video.site.eq_ignore_ascii_case("youtube")).map(|video| video.key.clone())).unwrap_or_default();

    let info = XtreamSeriesInfoInfo {
        name: series.title.clone(),
        cover: series.poster.clone().unwrap_or_default(),
        plot: series.plot.clone().unwrap_or_default(),
        cast: actor_names,
        director: series.directors.as_ref().map(|d| d.join(", ")).unwrap_or_default(),
        genre: series.genres.as_ref().map(|d| d.join(", ")).unwrap_or_default(),
        release_date: release_date.clone(),
        releaseDate: release_date.clone(),
        releasedate: release_date.clone(),
        last_modified: series.last_updated.to_string(),
        rating: series.rating.unwrap_or(0f64),
        rating_5based: 0.0,
        backdrop_path: series
            .fanart
            .as_ref()
            .filter(|s| !s.is_empty())
            .map(|f| vec![f.clone()])
            .or_else(|| {
                series.poster.as_ref()
                    .filter(|s| !s.is_empty())
                    .map(|p| vec![p.clone()])
            }),
        trailer: youtube_trailer.clone(),
        youtube_trailer,
        episode_run_time: String::from("0"),
        category_id: 0,
        tmdb_id: series.tmdb_id,
        tmdb: series.tmdb_id,
        year: series.year,
    };

    // seasons are not delivered through xtream get_series_info.
    let seasons = Some(vec![]); /*series.seasons.as_ref()
        .map(|seasons| seasons.iter().map(|season| XtreamSeriesInfoSeason {
            air_date: season.air_date.clone().unwrap_or_default(),
            episode_count: season.episode_count,
            id: season.id,
            name: season.name.clone(),
            overview: season.overview.clone().unwrap_or_default(),
            season_number: season.season_number,
            vote_average: season.vote_average,
            cover: season.poster_path.as_ref().cloned().unwrap_or_default(),
            cover_big: season.poster_path.as_ref().cloned().unwrap_or_default(),
        }).collect());
        */

    let episodes = series.episodes.as_ref().map(|episodes| {
        episodes.iter().filter(|episode| !episode.file_path.is_empty()).map(|episode| {
            let container_extension = Path::new(&episode.file_path)
                .extension()
                .and_then(|s| s.to_str())
                .map(ToString::to_string)
                .unwrap_or_default();

            let episode_release_date = episode.aired.as_ref().map(ToString::to_string).unwrap_or_default();

            let tmdb_id = (episode.tmdb_id > 0).then_some(episode.tmdb_id);

            XtreamSeriesInfoEpisode {
                id: episode.id,
                episode_num: episode.episode,
                season: episode.season,
                title: episode.title.clone(),
                container_extension,
                info: Some(XtreamSeriesInfoEpisodeInfo {
                    id: tmdb_id,
                    tmdb_id,
                    tmdb: tmdb_id,
                    season: episode.season,
                    release_date: episode_release_date.clone(),
                    releaseDate: episode_release_date.clone(),
                    releasedate: episode_release_date,
                    plot: episode.plot.clone().unwrap_or_default(),
                    duration_secs: episode.runtime.map_or(0, |r| r * 60),
                    duration: episode.runtime
                        .map(|r| format!("{:02}:{:02}:00", r / 60, r % 60))
                        .unwrap_or_default(),
                    movie_image: episode.thumb.clone().unwrap_or_default(),
                    video: None,
                    audio: None,
                    bitrate: 0,
                    rating: episode.rating.unwrap_or(0.0),
                }),
                custom_sid: String::new(),
                added: episode.file_modified.to_string(),
                direct_source: episode.file_path.clone(),
            }
        }).collect::<Vec<_>>()
    });

    Some(XtreamSeriesInfo { seasons, info: Some(info), episodes })
}