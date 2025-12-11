use crate::api::model::AppState;
use crate::model::{AppConfig, ProxyUserCredentials};
use crate::model::{Config, ConfigTarget, M3uTargetOutput};
use crate::repository::indexed_document::{IndexedDocumentDirectAccess, IndexedDocumentIterator, IndexedDocumentWriter};
use crate::repository::m3u_playlist_iterator::M3uPlaylistM3uTextIterator;
use crate::repository::storage::get_target_storage_path;
use crate::repository::storage_const;
use crate::utils;
use shared::error::{create_tuliprox_error, info_err};
use shared::error::{str_to_io_error, TuliproxError, TuliproxErrorKind};
use shared::model::PlaylistItemType;
use shared::model::{M3uPlaylistItem, PlaylistGroup, PlaylistItem};
use std::io::Error;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use log::error;
use tokio::fs;
use tokio::io::{AsyncWriteExt};
use tokio::task;
use crate::utils::{async_file_writer, IO_BUFFER_SIZE};

macro_rules! cant_write_result {
    ($path:expr, $err:expr) => {
        create_tuliprox_error!(TuliproxErrorKind::Notify, "failed to write m3u playlist: {} - {}", $path.display() ,$err)
    }
}

pub fn m3u_get_file_paths(target_path: &Path) -> (PathBuf, PathBuf) {
    let m3u_path = target_path.join(PathBuf::from(format!("{}.{}", storage_const::FILE_M3U, storage_const::FILE_SUFFIX_DB)));
    let index_path = target_path.join(PathBuf::from(format!("{}.{}", storage_const::FILE_M3U, storage_const::FILE_SUFFIX_INDEX)));
    (m3u_path, index_path)
}

pub fn m3u_get_epg_file_path(target_path: &Path) -> PathBuf {
    let path = target_path.join(PathBuf::from(format!("{}.{}", storage_const::FILE_M3U, storage_const::FILE_SUFFIX_DB)));
    utils::add_prefix_to_filename(&path, "epg_", Some("xml"))
}

macro_rules! await_playlist_write {
    ($expr:expr, $fmt:literal $(, $args:expr)* ) => {{
        $expr.await.map_err(|err| {
            create_tuliprox_error!(TuliproxErrorKind::Notify, $fmt $(, $args)*, err)
        })?
    }};
}

async fn persist_m3u_playlist_as_text(
    cfg: &Config,
    target: &ConfigTarget,
    target_output: &M3uTargetOutput,
    m3u_playlist: Arc<Vec<M3uPlaylistItem>>,
) -> Result<(), TuliproxError> {
    let Some(filename) = target_output.filename.as_ref() else { return Ok(()); };
    let Some(m3u_filename) = utils::get_file_path(&cfg.working_dir, Some(PathBuf::from(filename))) else { return Ok(()); };

    let file = await_playlist_write!(fs::File::create(&m3u_filename), "Can't write m3u plain playlist {} - {}", m3u_filename.display());
    // Larger buffer for sequential writes to reduce syscalls
    let mut writer = async_file_writer(file);
    await_playlist_write!(writer.write_all(b"#EXTM3U\n"), "Failed to write header to {} - {}", m3u_filename.display());

    let mut write_counter = 0usize;

    for m3u in m3u_playlist.iter() {
        let line = m3u.to_m3u(target.options.as_ref(), false);
        let bytes = line.as_bytes();
        await_playlist_write!(writer.write_all(bytes), "Failed to write entry to {} - {}", m3u_filename.display());
        await_playlist_write!(writer.write_all(b"\n"), "Failed to write newline to {} - {}", m3u_filename.display());
        write_counter += bytes.len() + 1;
        if write_counter >= IO_BUFFER_SIZE {
            await_playlist_write!(writer.flush(), "Failed to flush {} - {}", m3u_filename.display());
            write_counter = 0;
        }
    }

    await_playlist_write!(writer.flush(), "Failed to flush {} - {}", m3u_filename.display());

    Ok(())
}

pub async fn m3u_write_playlist(
    cfg: &AppConfig,
    target: &ConfigTarget,
    target_output: &M3uTargetOutput,
    target_path: &Path,
    new_playlist: &[PlaylistGroup],
) -> Result<(), TuliproxError> {
    if new_playlist.is_empty() {
        return Ok(());
    }

    let (m3u_path, idx_path) = m3u_get_file_paths(target_path);
    let m3u_playlist = Arc::new(
        new_playlist
            .iter()
            .flat_map(|pg| &pg.channels)
            .filter(|&pli| pli.header.item_type != PlaylistItemType::SeriesInfo)
            .map(PlaylistItem::to_m3u)
            .collect::<Vec<M3uPlaylistItem>>(),
    );

    let file_lock = cfg.file_locks.write_lock(&m3u_path).await;

    if let Err(err) = persist_m3u_playlist_as_text(&cfg.config.load(), target, target_output, Arc::clone(&m3u_playlist)).await {
        error!("Persisting m3u playlist failed: {err}");
    }

    let playlist = Arc::clone(&m3u_playlist);
    let m3u_path_clone = m3u_path.clone();
    let idx_path_clone = idx_path.clone();

    task::spawn_blocking(move || -> Result<(), TuliproxError> {
        let _guard = file_lock;
        match IndexedDocumentWriter::new(m3u_path_clone.clone(), idx_path_clone) {
            Ok(mut writer) => {
                for m3u in playlist.iter() {
                    writer
                        .write_doc(m3u.virtual_id, m3u)
                        .map_err(|err| cant_write_result!(&m3u_path_clone, err))?;
                }
                writer
                    .store()
                    .map_err(|err| cant_write_result!(&m3u_path_clone, err))
            }
            Err(err) => Err(cant_write_result!(&m3u_path_clone, err)),
        }
    })
        .await
        .map_err(|err| create_tuliprox_error!(TuliproxErrorKind::Notify, "failed to write m3u playlist: {} - {err}", m3u_path.display()))??;

    Ok(())
}

pub async fn m3u_load_rewrite_playlist(
    cfg: &AppConfig,
    target: &ConfigTarget,
    user: &ProxyUserCredentials,
) -> Result<M3uPlaylistM3uTextIterator, TuliproxError> {
    M3uPlaylistM3uTextIterator::new(cfg, target, user).await
}

pub async fn m3u_get_item_for_stream_id(stream_id: u32, app_state: &AppState, target: &ConfigTarget) -> Result<M3uPlaylistItem, Error> {
    if stream_id < 1 {
        return Err(str_to_io_error("id should start with 1"));
    }
    {
        if let Some(playlist) = app_state.playlists.data.read().await.get(target.name.as_str()) {
            if let Some(m3u_playlist) = playlist.m3u.as_ref() {
                if let Some(item) = m3u_playlist.query(&stream_id) {
                    return Ok(item.clone());
                }
                // fall through to disk lookup on cache miss
            }
        }

        let cfg: &AppConfig = &app_state.app_config;
        let target_path = get_target_storage_path(&cfg.config.load(), target.name.as_str()).ok_or_else(|| str_to_io_error(&format!("Could not find path for target {}", &target.name)))?;
        let (m3u_path, idx_path) = m3u_get_file_paths(&target_path);
        let _file_lock = cfg.file_locks.read_lock(&m3u_path).await;
        IndexedDocumentDirectAccess::read_indexed_item::<u32, M3uPlaylistItem>(&m3u_path, &idx_path, &stream_id)
    }
}

pub async fn iter_raw_m3u_playlist(config: &AppConfig, target: &ConfigTarget) -> Option<(utils::FileReadGuard, impl Iterator<Item=(M3uPlaylistItem, bool)>)> {
    let target_path = get_target_storage_path(&config.config.load(), target.name.as_str())?;
    let (m3u_path, idx_path) = m3u_get_file_paths(&target_path);
    if !m3u_path.exists() || !idx_path.exists() {
        return None;
    }
    let file_lock = config.file_locks.read_lock(&m3u_path).await;
    match IndexedDocumentIterator::<u32, M3uPlaylistItem>::new(&m3u_path, &idx_path)
        .map_err(|err| info_err!(format!("Could not deserialize file {m3u_path:?} - {err}"))) {
        Ok(reader) => Some((file_lock, reader)),
        Err(_) => None
    }
}