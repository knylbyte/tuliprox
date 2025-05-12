use std::collections::HashMap;
use std::path::{Path, PathBuf};
use notify::{recommended_watcher, Event, EventKind, RecursiveMode, Result, Watcher};
use std::sync::{mpsc, Arc};
use log::info;
use notify::event::{AccessKind, AccessMode};
use crate::api::model::app_state::AppState;

pub async fn exec_config_watch(app_state: &Arc<AppState>) -> notify::Result<()> {
    let (tx, rx) = mpsc::channel::<Result<Event>>();

    let mut files = HashMap::new();
    files.insert(PathBuf::from(&app_state.config.t_config_file_path), "config");
    files.insert(PathBuf::from(&app_state.config.t_api_proxy_file_path),"api-proxy");
    files.insert(PathBuf::from(&app_state.config.t_mapping_file_path),"mapping");
    files.insert(PathBuf::from(&app_state.config.t_sources_file_path),"sources");

    // Use recommended_watcher() to automatically select the best implementation
    // for your platform. The `EventHandler` passed to this constructor can be a
    // closure, a `std::sync::mpsc::Sender`, a `crossbeam_channel::Sender`, or
    // another type the trait is implemented for.
    let mut watcher = recommended_watcher(tx)?;

    // Add a path to be watched. All files and directories at that path and
    // below will be monitored for changes.
    let path = Path::new(app_state.config.t_config_path.as_str());
    info!("Watching changes {path:?}");
    watcher.watch(path, RecursiveMode::NonRecursive)?;

    tokio::spawn(async move {
        let _keep_watcher_alive = watcher;
        for res in rx {
            match res {
                Ok(event) => {
                    if let EventKind::Access(AccessKind::Close(AccessMode::Write)) = event.kind {
                        for path in event.paths {
                            if let Some(file) = files.get(&path) {
                                println!("File changed {file}: {path:?}");
                            }
                        }
                    }
                }
                Err(e) => println!("watch error: {e:?}"),
            }
        }
        info!("Watching stopped");
    });

    Ok(())
}