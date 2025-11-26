use crate::model::Epg;
use crate::model::{Config, ConfigTarget, TargetOutput};
use crate::repository::m3u_repository::m3u_get_epg_file_path;
use crate::repository::xtream_repository::{xtream_get_epg_file_path, xtream_get_storage_path};
use crate::utils::debug_if_enabled;
use shared::error::{notify_err, TuliproxError};
use std::path::Path;
use tokio::io::AsyncWriteExt;


// Due to an error in quick_xml we cant write doc type through event. The quotes are escaped and the xml file is invalid.
//
// // XML Header
// writer.write_event_async(quick_xml::events::Event::Decl(quick_xml::events::BytesDecl::new("1.0", Some("utf-8"), None)))
//     .await.map_err(|e| notify_err!(format!("failed to write XML header: {}", e)))?;
//
// // DOCTYPE
// writer.write_event_async(quick_xml::events::Event::DocType(quick_xml::events::BytesText::new(r#"tv SYSTEM "xmltv.dtd""#)))
//     .await.map_err(|e| notify_err!(format!("failed to write doctype: {}", e)))?;
pub async fn epg_write_file(target: &ConfigTarget, epg: &Epg, path: &Path) -> Result<(), TuliproxError> {
    let file = tokio::fs::File::create(path).await
        .map_err(|e| notify_err!(format!("failed to create epg file: {}", e)))?;
    let mut buf_writer = tokio::io::BufWriter::new(file);

    // Work-Around BytesText DocType escape, see below
    buf_writer.write_all(b"<?xml version=\"1.0\" encoding=\"utf-8\"?>\n").await
        .map_err(|e| notify_err!(format!("failed to write XML header: {}", e)))?;

    buf_writer.write_all(b"<!DOCTYPE tv SYSTEM \"xmltv.dtd\">\n").await
        .map_err(|e| notify_err!(format!("failed to write doctype: {}", e)))?;

    let mut writer = quick_xml::writer::Writer::new(buf_writer);

    // EPG Content
    epg.write_to_async(&mut writer).await.map_err(|e| notify_err!(format!("failed to write epg: {}", e)))?;

    let inner = writer.get_mut(); // Zugriff auf den BufWriter<tokio::fs::File>
    inner.flush().await.map_err(|e| notify_err!(format!("failed to flush epg: {}", e)))?;

    debug_if_enabled!("Epg for target {} written to {}", target.name, path.to_str().unwrap_or("?"));
    Ok(())
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
