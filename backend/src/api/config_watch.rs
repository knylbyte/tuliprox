use crate::api::model::{update_app_state_config, update_app_state_sources, AppState, EventMessage};
use crate::model::{Config, SourcesConfig};
use crate::utils;
use crate::utils::{is_directory, prepare_sources_batch, read_config_file, read_sources_file};
use log::{debug, error, info};
use notify::event::{AccessKind, AccessMode};
use notify::{recommended_watcher, EventKind, RecursiveMode, Watcher};
use shared::error::{TuliproxError, TuliproxErrorKind};
use shared::model::{ConfigPaths, ConfigType};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use arc_swap::access::Access;
use arc_swap::ArcSwap;
use tokio_util::sync::CancellationToken;

enum ConfigFile {
    Config,
    ApiProxy,
    Mapping,
    Sources,
    SourceFile,
}

impl ConfigFile {
    fn load_mapping(app_state: &Arc<AppState>) -> Result<(), TuliproxError> {
        let paths = <Arc<ArcSwap<ConfigPaths>> as Access<ConfigPaths>>::load(&app_state.app_config.paths);
        if let Some(mapping_file_path) = paths.mapping_file_path.as_ref() {
            match utils::read_mappings(mapping_file_path, true) {
                Ok(Some(mappings_cfg)) => {
                    app_state.app_config.set_mappings(mapping_file_path, &mappings_cfg);
                    info!("Loaded mapping file {mapping_file_path}");
                }
                Ok(None) => {
                    info!("No mapping file loaded {mapping_file_path}");
                }
                Err(err) => {
                    error!("Failed to load mapping file {err}");
                    return Err(err);
                }
            }
        }
        Ok(())
    }

    fn load_api_proxy(app_state: &Arc<AppState>) -> Result<(), TuliproxError> {
        match utils::read_api_proxy_config(&app_state.app_config, true) {
            Ok(Some(api_proxy)) => {
                app_state.app_config.set_api_proxy(api_proxy)?;
                let paths = <Arc<ArcSwap<ConfigPaths>> as Access<ConfigPaths>>::load(&app_state.app_config.paths);
                info!("Loaded Api Proxy File: {:?}", &paths.api_proxy_file_path);
            }
            Ok(None) => {
                let paths = <Arc<ArcSwap<ConfigPaths>> as Access<ConfigPaths>>::load(&app_state.app_config.paths);
                info!("Could not load Api Proxy File: {:?}", &paths.api_proxy_file_path);
            }
            Err(err) => {
                error!("Failed to load api-proxy file {err}");
                return Err(err);
            }
        }
        Ok(())
    }

    async fn load_config(app_state: &Arc<AppState>) -> Result<(), TuliproxError> {
        let paths = <Arc<ArcSwap<ConfigPaths>> as Access<ConfigPaths>>::load(&app_state.app_config.paths);
        let config_file = paths.config_file_path.as_str();
        let config_dto = read_config_file(config_file, true, true)?;
        let mapping_changed = paths.mapping_file_path.as_ref() !=  config_dto.mapping_path.as_ref();
        let mut config: Config = Config::from(config_dto);
        config.prepare(paths.config_path.as_str())?;
        info!("Loaded config file {config_file}");
        update_app_state_config(app_state, config).await?;
        if mapping_changed {
            Self::load_mapping(app_state)?;
        }
        Ok(())
    }

    async fn load_sources(app_state: &Arc<AppState>) -> Result<(), TuliproxError> {
        let paths = <Arc<ArcSwap<ConfigPaths>> as Access<ConfigPaths>>::load(&app_state.app_config.paths);
        let sources_file = paths.sources_file_path.as_str();
        let config = <Arc<ArcSwap<Config>> as Access<Config>>::load(&app_state.app_config.config);
        let mut sources_dto = read_sources_file(sources_file, true, true, config.get_hdhr_device_overview().as_ref())?;
        prepare_sources_batch(&mut sources_dto, true)?;
        let sources: SourcesConfig = SourcesConfig::try_from(sources_dto)?;
        info!("Loaded sources file {sources_file}");
        update_app_state_sources(app_state, sources).await?;
        // mappings are not stored, so we need to reload and apply them if sources change.
        Self::load_mapping(app_state)
    }

    async fn load_source_file(app_state: &Arc<AppState>, file: &Path) -> Result<(), TuliproxError> {
        info!("Loaded sources file {}", file.display());
        // TODO selective update and not complete sources update ?
        ConfigFile::load_sources(app_state).await
    }

    pub(crate) async fn reload(&self, file_path: &Path, app_state: &Arc<AppState>) -> Result<(), TuliproxError> {
        debug!("File change detected {}", file_path.display());
        match self {
            ConfigFile::ApiProxy => {
                app_state.event_manager.send_event(EventMessage::ConfigChange(ConfigType::ApiProxy));
                ConfigFile::load_api_proxy(app_state)
            }
            ConfigFile::Mapping => {
                app_state.event_manager.send_event(EventMessage::ConfigChange(ConfigType::Mapping));
                ConfigFile::load_mapping(app_state)
            }
            ConfigFile::Config => {
                app_state.event_manager.send_event(EventMessage::ConfigChange(ConfigType::Config));
                ConfigFile::load_config(app_state).await
            }
            ConfigFile::Sources => {
                app_state.event_manager.send_event(EventMessage::ConfigChange(ConfigType::Sources));
                ConfigFile::load_sources(app_state).await
            }
            ConfigFile::SourceFile => {
                app_state.event_manager.send_event(EventMessage::ConfigChange(ConfigType::Sources));
                ConfigFile::load_source_file(app_state, file_path).await
            }
        }
    }
}

fn start_config_watch(app_state: &Arc<AppState>, cancel_token: &CancellationToken) -> Result<(), TuliproxError> {
    // let (tx, mut rx) = mpsc::channel::<notify::Result<Event>>(100);
    let (std_tx, std_rx) = std::sync::mpsc::channel();
    let (tx, mut rx) = tokio::sync::mpsc::channel(100);

    let paths = <Arc<ArcSwap<ConfigPaths>> as Access<ConfigPaths>>::load(&app_state.app_config.paths);
    let mapping_file_path = paths.mapping_file_path.as_ref().map_or_else(String::new, ToString::to_string);
    let files = get_watch_files(app_state, &paths, mapping_file_path.as_str());
    //
    // // Add a path to be watched. All files and directories at that path and
    // // below will be monitored for changes.
    let path = Path::new(paths.config_path.as_str());
    let recursive_mode = if !mapping_file_path.is_empty() && utils::is_directory(&mapping_file_path) { RecursiveMode::Recursive } else { RecursiveMode::NonRecursive };

    std::thread::spawn({
        let tx = tx.clone();
        move || {
            for res in std_rx {
                let _ = tx.blocking_send(res);
            }
            //debug!("Stopping config watch channel thread");
        }
    });

    // Use recommended_watcher() to automatically select the best implementation
    // for your platform. The `EventHandler` passed to this constructor can be a
    // closure, a `std::sync::mpsc::Sender`, a `crossbeam_channel::Sender`, or
    // another type the trait is implemented for.
    let mut watcher = recommended_watcher(std_tx).map_err(|err| TuliproxError::new(TuliproxErrorKind::Info, format!("Failed to init config file watcher {err}")))?;
    watcher.watch(path, recursive_mode).map_err(|err| TuliproxError::new(TuliproxErrorKind::Info, format!("Failed to start config file watcher {err}")))?;
    info!("Watching config file changes {}", path.display());

    let event_manager = Arc::clone(&app_state.event_manager);
    let cancel = cancel_token.clone();
    let watcher_app_state = Arc::clone(app_state);

    let handle_error = move |err: TuliproxError, path: &Path| {
        let msg = format!("Failed to reload config file {}: {err}", path.display());
        error!("{msg}");
        event_manager.send_event(EventMessage::ServerError(msg));
    };

    tokio::spawn(async move {
        info!("Configuration file watcher started.");

        let _keep_watcher_alive = watcher;

        loop {
            tokio::select! {
            biased;

            () = cancel.cancelled() => {
                info!("Cancellation received, shutting down watcher task.");
                break;
            }

            Some(res) = rx.recv() => {
                match res {
                    Ok(event) => {
                        if let EventKind::Access(AccessKind::Close(AccessMode::Write)) = event.kind {
                        for path in event.paths {
                            if let Some((config_file, _is_dir)) = files.get(&path) {
                                if let Err(err) = config_file.reload(&path, &watcher_app_state).await {
                                   handle_error(err, &path);
                                }
                            } else if recursive_mode == RecursiveMode::Recursive && path.extension().is_some_and(|ext| ext == "yml") {
                                for (key, (config_file, is_dir)) in &files {
                                    if *is_dir && path.starts_with(key) {
                                        if let Err(err) = config_file.reload(&path, &watcher_app_state).await {
                                            handle_error(err, &path);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    }
                    Err(e) => error!("watch error: {e:?}"),
                }
            }

            else => break, // Channel closed
            }
        }
        info!("Configuration file watcher terminated.");
    });

    Ok(())
}

fn get_watch_files(app_state: &Arc<AppState>, paths: &ConfigPaths, mapping_file_path: &str) -> HashMap<PathBuf, (ConfigFile, bool)> {
    let sources = <Arc<ArcSwap<SourcesConfig>> as Access<SourcesConfig>>::load(&app_state.app_config.sources);
    let input_files_paths = sources.get_input_files();
    let mut files = HashMap::new();
    [(paths.config_file_path.as_str(), ConfigFile::Config),
        (paths.api_proxy_file_path.as_str(), ConfigFile::ApiProxy),
        (mapping_file_path, ConfigFile::Mapping),
        (paths.sources_file_path.as_str(), ConfigFile::Sources)
    ].into_iter()
        .filter(|(path, _)| !path.is_empty())
        .for_each(|(path, config_file)| { files.insert(PathBuf::from(path), (config_file, is_directory(path))); });
    for path in input_files_paths { files.insert(path, (ConfigFile::SourceFile, false)); }
    files
}

pub fn exec_config_watch(app_state: &Arc<AppState>,
                         cancel: &CancellationToken) {
    let hot_reload = {
        let config = <Arc<ArcSwap<Config>> as Access<Config>>::load(&app_state.app_config.config);
        config.config_hot_reload
    };

    if hot_reload {
        if let Err(err) = start_config_watch(app_state, cancel) {
            error!("Failed to start config watch: {err}");
        }
    }
}
