use crate::repository::bplustree::{BPlusTreeDiskIterator, BPlusTreeQuery};
use env_logger::{Builder, Target};
use log::{error, warn, LevelFilter};
use serde::{Deserialize, Serialize};
use shared::model::{M3uPlaylistItem, XtreamPlaylistItem};
use std::io::Write;
use std::path::{Path, PathBuf};

enum DbType {
    Xtream,
    M3u,
}

pub fn db_viewer(xtream_filename: Option<&str>, m3u_filename: Option<&str>) {
    if let Some(filename) = xtream_filename {
        dump_db(filename, DbType::Xtream);
    }
    if let Some(filename) = m3u_filename {
        dump_db(filename, DbType::M3u);
    }
}

fn dump_db(filename: &str, db_type: DbType) {
    let mut log_builder = Builder::from_default_env();
    log_builder.target(Target::Stderr);
    log_builder.filter_level(LevelFilter::Info);
    let _ = log_builder.try_init();

    match PathBuf::from(filename).canonicalize() {
        Ok(path) => {
            match db_type {
                DbType::Xtream => {
                    if let Ok(mut query) = BPlusTreeQuery::<u32, XtreamPlaylistItem>::try_new(&path) {
                        let iterator = query.iter();
                        print_json_from_iter(iterator);
                    }
                }
                DbType::M3u => {
                    if let Ok(mut query) = BPlusTreeQuery::<u32, M3uPlaylistItem>::try_new(&path) {
                        let iterator = query.iter();
                        print_json_from_iter(iterator);
                    }
                }
            }
        }
        Err(err) => {
            error!("Invalid file path! {err}");
        }
    };

    exit_app(1);
}

fn print_json_from_iter<P>(iterator: BPlusTreeDiskIterator<u32, P>)
where
    P: Serialize + for<'de> Deserialize<'de> + Clone,
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
            }
            Err(err) => error!("Failed: {err}"),
        }
    }
    println!("]");

    exit_app(0);
}

fn exit_app(code: i32) {
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    std::process::exit(code);
}
