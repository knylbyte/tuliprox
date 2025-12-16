use crate::library::{MediaMetadata, MetadataAsyncIter, MetadataCacheEntry};
use crate::model::{XtreamMovieData, XtreamMovieInfo, XtreamMovieInfoDetails};
use crate::model::{AppConfig, ConfigInput};
use serde_json::value::RawValue;
use shared::error::TuliproxError;
use shared::model::{PlaylistGroup, PlaylistItem, PlaylistItemHeader, PlaylistItemType, XtreamCluster};
use shared::utils::string_to_uuid_type;
use std::sync::Arc;
use std::path::Path;

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
                group_movies.channels.push(pli);
            },
            MediaMetadata::Series(_) => {
                let pli = to_playlist_item(&entry, &input.name, &library_config.playlist.series_category);
                group_series.channels.push(pli);
            },
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

fn to_playlist_item(entry: &MetadataCacheEntry, input_name: &str, group_name: &str) -> PlaylistItem {
    let metadata = &entry.metadata;

    let (xtream_cluster, item_type, additional_props): (XtreamCluster, PlaylistItemType, Option<Box<RawValue>>) = {
        match metadata {
            MediaMetadata::Movie(_) => {
                let add_props = metadata_cache_entry_to_xtream_movie_info(entry).and_then(|info| {
                    let raw = serde_json::to_string(&info).ok()?;
                    RawValue::from_string(raw).ok()
                });
                (XtreamCluster::Video, PlaylistItemType::LocalVideo, add_props)
            }
            MediaMetadata::Series(_s) => {
                (XtreamCluster::Series, PlaylistItemType::LocalSeriesInfo, None)
            }
        }
    };

    PlaylistItem {
        header: PlaylistItemHeader {
            uuid: string_to_uuid_type(&entry.uuid),
            id: String::new(),
            virtual_id: 0,
            name: metadata.title().to_string(),
            chno: String::new(),
            logo: String::new(),
            logo_small: String::new(),
            group: group_name.to_string(),
            title: metadata.title().to_string(),
            parent_code: String::new(),
            audio_track: String::new(),
            time_shift: String::new(),
            rec: String::new(),
            url: format!("file://{}", entry.file_path),
            epg_channel_id: None,
            xtream_cluster,
            additional_properties: additional_props,
            item_type,
            category_id: 0,
            input_name: input_name.to_string(),
        },
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
        tmdb_id: movie.tmdb_id.map(|id|id.to_string()),

        name: Some(movie.title.clone()),
        o_name: movie.original_title.clone(),

        cover_big: movie.poster.clone(),
        movie_image: movie.poster.clone(),

        releasedate: movie.year.map(|y| format!("{y}-01-01")),
        episode_run_time: movie.runtime,

        youtube_trailer: None,
        director: movie.directors.as_ref().map(|d| d.join(", ")),
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
