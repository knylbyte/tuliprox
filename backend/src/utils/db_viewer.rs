use std::io::Write;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use shared::model::{M3uPlaylistItem, XtreamPlaylistItem};
use crate::repository::bplustree::{BPlusTreeDiskIterator, BPlusTreeQuery};

pub fn db_viewer(filename: &str, content_type: &str) {
    let path = match PathBuf::from(filename).canonicalize() {
        Ok(p) => p,
        Err(err) => {
            println!("File does not exist! {err}");
            let _ = std::io::stdout().flush();
            std::process::exit(1);
        }
    };

    if !path.exists() {
        println!("File does not exist! {}", path.display());
        let _ = std::io::stdout().flush();
        std::process::exit(1);
    }
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
        _ => println!("Allowed content types are: [m3u, xtream]"),
    }
    let _ = std::io::stdout().flush();
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
            Err(err) => eprintln!("Failed: {err}"),
        }
    }
    println!("]");
}
