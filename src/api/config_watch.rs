use crate::api::model::app_state::AppState;
use crate::tuliprox_error::{TuliproxError, TuliproxErrorKind};
use crate::utils;
use crate::utils::is_directory;
use log::{debug, error, info};
use notify::event::{AccessKind, AccessMode};
use notify::{recommended_watcher, Event, EventKind, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};

enum ConfigFile {
    Config,
    ApiProxy,
    Mapping,
    Sources,
}

impl ConfigFile {
    fn load_mappping(app_state: &Arc<AppState>) -> Result<(), TuliproxError> {
        match utils::read_mappings(app_state.config.t_mapping_file_path.as_str(), true) {
            Ok(Some(mappings_cfg)) => {
                app_state.config.set_mappings(&mappings_cfg);
                info!("Loaded mapping file {}", app_state.config.t_mapping_file_path.as_str());
            }
            Ok(None) => {
                info!("No mapping file loaded {}", app_state.config.t_mapping_file_path.as_str());
            }
            Err(err) => {
                error!("Failed to load mapping file {err}");
                return Err(err);
            }
        }

        Ok(())
    }

    fn load_api_proxy(app_state: &Arc<AppState>) -> Result<(), TuliproxError> {
        match utils::read_api_proxy_config(&app_state.config) {
            Ok(()) => {
                info!("Api Proxy File: {:?}", &app_state.config.t_api_proxy_file_path);
            }
            Err(err) => {
                error!("Failed to load api-proxy file {err}");
                return Err(err);
            }
        }
        Ok(())
    }
    pub(crate) fn reload(&self, file_path: &PathBuf, app_state: &Arc<AppState>) -> Result<(), TuliproxError> {
        debug!("File changed {file_path:?}");
        match self {
            ConfigFile::ApiProxy => ConfigFile::load_api_proxy(app_state),
            ConfigFile::Mapping => ConfigFile::load_mappping(app_state),
            ConfigFile::Config | ConfigFile::Sources => { Ok(()) }
        }
    }
}

pub async fn exec_config_watch(app_state: &Arc<AppState>) -> Result<(), TuliproxError> {
    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();

    let mut files = HashMap::new();
    [(&app_state.config.t_config_file_path, ConfigFile::Config),
        (&app_state.config.t_api_proxy_file_path, ConfigFile::ApiProxy),
        (&app_state.config.t_mapping_file_path, ConfigFile::Mapping),
        (&app_state.config.t_sources_file_path, ConfigFile::Sources)
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
    let path = Path::new(app_state.config.t_config_path.as_str());
    let recursive_mode = if utils::is_directory(&app_state.config.t_mapping_file_path) { RecursiveMode::Recursive } else { RecursiveMode::NonRecursive };
    watcher.watch(path, recursive_mode).map_err(|err| TuliproxError::new(TuliproxErrorKind::Info, format!("Failed to start config file watcher {err}")))?;
    info!("Watching config file changes {path:?}");

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
                                    error!("Failed to reload config file {path:?}: {err}");
                                }
                            } else if recursive_mode == RecursiveMode::Recursive && path.extension().is_some_and(|ext| ext == "yml") {
                                for (key, (config_file, is_dir)) in &files {
                                    if *is_dir && path.starts_with(key) {
                                        if let Err(err) = config_file.reload(&path, &watcher_app_state) {
                                            error!("Failed to reload config file {path:?}: {err}");
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