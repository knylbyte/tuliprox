use crate::api::model::app_state::AppState;
use crate::utils;
use crate::utils::{is_directory, read_config_file, read_sources_file};
use log::{debug, error, info};
use notify::event::{AccessKind, AccessMode};
use notify::{recommended_watcher, Event, EventKind, RecursiveMode, Watcher};
use shared::error::{TuliproxError, TuliproxErrorKind};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use crate::model::{Config, SourcesConfig};

enum ConfigFile {
    Config,
    ApiProxy,
    Mapping,
    Sources,
}

impl ConfigFile {
    fn load_mappping(app_state: &Arc<AppState>) -> Result<(), TuliproxError> {
        let paths =  app_state.app_config.paths.load();
        if let Some(mapping_file_path) = paths.mapping_file_path.as_ref() {
            match utils::read_mappings(mapping_file_path, true) {
                Ok(Some(mappings_cfg)) => {
                    app_state.app_config.set_mappings(&mappings_cfg);
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
                let paths =  app_state.app_config.paths.load();
                info!("Loaded Api Proxy File: {:?}", &paths.api_proxy_file_path);
            }
            Ok(None) => {
                let paths =  app_state.app_config.paths.load();
                info!("Coul dnot load Api Proxy File: {:?}", &paths.api_proxy_file_path);
            }
            Err(err) => {
                error!("Failed to load api-proxy file {err}");
                return Err(err);
            }
        }
        Ok(())
    }

    fn load_config(app_state: &Arc<AppState>) -> Result<(), TuliproxError> {
        let paths =  app_state.app_config.paths.load();
        let config_file = paths.config_file_path.as_str();
        let config_dto = read_config_file(config_file, true)?;
        let mut config: Config = Config::from(config_dto);
        config.prepare(paths.config_path.as_str())?;
        info!("Loaded config file {config_file}");
        app_state.set_config(config)?;
        Ok(())
    }

    fn load_sources(app_state: &Arc<AppState>) -> Result<(), TuliproxError> {
        let paths =  app_state.app_config.paths.load();
        let sources_file = paths.sources_file_path.as_str();
        let sources_dto = read_sources_file(sources_file, true, true)?;
        let sources: SourcesConfig = sources_dto.into();
        info!("Loaded sources file {sources_file}");
        app_state.app_config.set_sources(sources)?;
        Ok(())
    }

    pub(crate) fn reload(&self, file_path: &Path, app_state: &Arc<AppState>) -> Result<(), TuliproxError> {
        debug!("File change detected {}", file_path.display());
        match self {
            ConfigFile::ApiProxy => ConfigFile::load_api_proxy(app_state),
            ConfigFile::Mapping => ConfigFile::load_mappping(app_state),
            ConfigFile::Config => ConfigFile::load_config(app_state),
            ConfigFile::Sources => ConfigFile::load_sources(app_state),
        }
    }
}

pub async fn exec_config_watch(app_state: &Arc<AppState>) -> Result<(), TuliproxError> {
    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();

    let paths = &app_state.app_config.paths.load();
    let mapping_file_path = paths.mapping_file_path.as_ref().map_or_else(String::new, ToString::to_string);
    let mut files = HashMap::new();
    [(&paths.config_file_path, ConfigFile::Config),
        (&paths.api_proxy_file_path, ConfigFile::ApiProxy),
        (&mapping_file_path, ConfigFile::Mapping),
        (&paths.sources_file_path, ConfigFile::Sources)
    ].into_iter()
        .filter(|(path, _)| !path.is_empty())
        .for_each(|(path, config_file)| { files.insert(PathBuf::from(path), (config_file, is_directory(path))); });

    // Use recommended_watcher() to automatically select the best implementation
    // for your platform. The `EventHandler` passed to this constructor can be a
    // closure, a `std::sync::mpsc::Sender`, a `crossbeam_channel::Sender`, or
    // another type the trait is implemented for.
    let mut watcher = recommended_watcher(tx).map_err(|err| TuliproxError::new(TuliproxErrorKind::Info, format!("Failed to init config file watcher {err}")))?;

    // Add a path to be watched. All files and directories at that path and
    // below will be monitored for changes.
    let path = Path::new(paths.config_path.as_str());
    let recursive_mode = if !mapping_file_path.is_empty() && utils::is_directory(&mapping_file_path) { RecursiveMode::Recursive } else { RecursiveMode::NonRecursive };
    watcher.watch(path, recursive_mode).map_err(|err| TuliproxError::new(TuliproxErrorKind::Info, format!("Failed to start config file watcher {err}")))?;
    info!("Watching config file changes {}", path.display());

    let watcher_app_state = Arc::clone(app_state);
    tokio::spawn(async move {
        let _keep_watcher_alive = watcher;
        for res in rx {
            match res {
                Ok(event) => {
                    if let EventKind::Access(AccessKind::Close(AccessMode::Write)) = event.kind {
                        for path in event.paths {
                            if let Some((config_file, _is_dir)) = files.get(&path) {
                                if let Err(err) = config_file.reload(&path, &watcher_app_state) {
                                    error!("Failed to reload config file {}: {err}", path.display());
                                }
                            } else if recursive_mode == RecursiveMode::Recursive && path.extension().is_some_and(|ext| ext == "yml") {
                                for (key, (config_file, is_dir)) in &files {
                                    if *is_dir && path.starts_with(key) {
                                        if let Err(err) = config_file.reload(&path, &watcher_app_state) {
                                            error!("Failed to reload config file {}: {err}", path.display());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("watch error: {e:?}");
                }
            }
        }
        info!("Watching stopped");
    });

    Ok(())
}