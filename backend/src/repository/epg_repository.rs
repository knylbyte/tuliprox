use std::io::Write;
use shared::error::{notify_err, TuliproxError};
use crate::model::{Config, ConfigTarget, TargetOutput};
use crate::model::Epg;
use crate::repository::m3u_repository::m3u_get_epg_file_path;
use crate::repository::xtream_repository::{xtream_get_epg_file_path, xtream_get_storage_path};
use crate::utils::debug_if_enabled;
use std::path::Path;

async fn epg_write_file(target: &ConfigTarget, epg: &Epg, path: &Path) -> Result<(), TuliproxError> {
    let file = tokio::fs::File::create(path).await.map_err(|e| notify_err!(format!("failed to create epg file: {}", e)))?;
    // problem quickxml is not async
    let sync_writer = tokio_util::io::SyncIoBridge::new(file);
    let mut writer = quick_xml::Writer::new(std::io::BufWriter::new(sync_writer));

    writer.write_event(quick_xml::events::Event::Decl(
        quick_xml::events::BytesDecl::new("1.0", Some("utf-8"), None)
    )).map_err(|e| notify_err!(format!("failed to write XML header: {}", e)))?;

    writer.write_event(quick_xml::events::Event::DocType(quick_xml::events::BytesText::new("tv SYSTEM \"xmltv.dtd\"")))
        .map_err(|e| notify_err!(format!("failed to write doctype: {}", e)))?;


    epg.write_to(&mut writer).map_err(|e| notify_err!(format!("failed to write epg: {}", e)))?;

    writer.into_inner().flush().map_err(|e| notify_err!(format!("failed to flush epg: {}", e)))?;

    debug_if_enabled!("Epg for target {} written to {}", target.name, path.to_str().unwrap_or("?"));
    Ok(())
    //
    //
    // let mut writer = Writer::new(Cursor::new(vec![]));
    // match epg.write_to(&mut writer) {
    //     Ok(()) => {
    //         let result = writer.into_inner().into_inner();
    //         match File::create(path).await {
    //             Ok(mut epg_file) => {
    //                 match epg_file.write_all("<?xml version=\"1.0\" encoding=\"utf-8\" ?><!DOCTYPE tv SYSTEM \"xmltv.dtd\">".as_bytes()).await {
    //                     Ok(()) => {}
    //                     Err(err) => return Err(notify_err!(format!("failed to write epg: {} - {}", path.to_str().unwrap_or("?"), err))),
    //                 }
    //                 match epg_file.write_all(&result).await {
    //                     Ok(()) => {
    //                         debug_if_enabled!("Epg for target {} written to {}", target.name, path.to_str().unwrap_or("?"));
    //                     }
    //                     Err(err) => return Err(notify_err!(format!("failed to write epg: {} - {}", path.to_str().unwrap_or("?"), err))),
    //                 }
    //             }
    //             Err(err) => return Err(notify_err!(format!("failed to write epg: {} - {}", path.to_str().unwrap_or("?"), err))),
    //         }
    //     }
    //     Err(err) => return Err(notify_err!(format!("failed to write epg: {} - {}", path.to_str().unwrap_or("?"), err))),
    // }
    // Ok(())
}

pub async fn epg_write(cfg: &Config, target: &ConfigTarget, target_path: &Path, epg: Option<&Epg>, output: &TargetOutput) -> Result<(), TuliproxError> {
    if let Some(epg_data) = epg {
        match output {
            TargetOutput::Xtream(_) => {
                match xtream_get_storage_path(cfg, &target.name) {
                    Some(path) => {
                        let epg_path = xtream_get_epg_file_path(&path);
                        debug_if_enabled!("writing xtream epg to {}", epg_path.to_str().unwrap_or("?"));
                        epg_write_file(target, epg_data, &epg_path).await?;
                    }
                    None => return Err(notify_err!(format!("failed to serialize epg for target: {}, storage path not found", target.name))),
                }
            }
            TargetOutput::M3u(_) => {
                let path = m3u_get_epg_file_path(target_path);
                debug_if_enabled!("writing m3u epg to {}", path.to_str().unwrap_or("?"));
                epg_write_file(target, epg_data, &path).await?;
            }
            TargetOutput::Strm(_) | TargetOutput::HdHomeRun(_) => {}
        }
    }
    Ok(())
}
