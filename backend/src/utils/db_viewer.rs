use std::io::Write;
use std::path::PathBuf;
use env_logger::{Builder, Target};
use log::{error, warn, LevelFilter};
use serde::{Deserialize, Serialize};
use shared::model::{M3uPlaylistItem, XtreamPlaylistItem};
use crate::repository::bplustree::{BPlusTreeDiskIterator, BPlusTreeQuery};

pub fn db_viewer(filename: &str, content_type: &str) {
    let mut log_builder = Builder::from_default_env();
    log_builder.target(Target::Stderr);
    log_builder.filter_level(LevelFilter::Info);
    let _ = log_builder.try_init();

    let path = match PathBuf::from(filename).canonicalize() {
        Ok(p) => p,
        Err(err) => {
            error!("Invalid file path! {err}");
            let _ = std::io::stderr().flush();
            std::process::exit(1);
        }
    };

    match content_type {
        "xtream" => {
            if let Ok(mut query) = BPlusTreeQuery::<u32, XtreamPlaylistItem>::try_new(&path) {
                let iterator = query.iter();
                print_json_from_iter(iterator);
            }
        },
        "m3u" => {
            if let Ok(mut query) = BPlusTreeQuery::<u32, M3uPlaylistItem>::try_new(&path) {
                let iterator = query.iter();
                print_json_from_iter(iterator);
            }
        }
        _ => warn!("Allowed content types are: [m3u, xtream]"),
    }
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    std::process::exit(1);
}

fn print_json_from_iter<P>(iterator: BPlusTreeDiskIterator<u32, P>)
where P: Serialize + for<'de> Deserialize<'de> + Clone
{
    println!("[");
    let mut first = true;
    for (_, entry) in iterator {
        match serde_json::to_string(&entry) {
            Ok(text) => {
                if !first {
                    println!(",");
                }
                println!("{text}");
                first = false;
            },
            Err(err) => error!("Failed: {err}"),
        }
    }
    println!("]");
}
