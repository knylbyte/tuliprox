use crate::model::xmltv::XmlTagIcon::Undefined;
use chrono::{Datelike, TimeZone, Utc};
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Error, Writer};
use shared::error::{TuliproxError, TuliproxErrorKind};
use shared::model::{parse_xmltv_time, EpgChannel, EpgProgramme, EpgTv, InputFetchMethod};
use std::cmp::{max, min};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use futures::TryFutureExt;
use tokio::io::AsyncRead;
use url::Url;
use shared::utils::sanitize_sensitive_info;
use crate::api::model::AppState;
use crate::utils::request::{get_remote_content_as_stream};

pub const EPG_TAG_TV: &str = "tv";
pub const EPG_TAG_PROGRAMME: &str = "programme";
pub const EPG_TAG_CHANNEL: &str = "channel";
pub const EPG_ATTRIB_ID: &str = "id";
pub const EPG_ATTRIB_CHANNEL: &str = "channel";
pub const EPG_TAG_DISPLAY_NAME: &str = "display-name";
pub const EPG_TAG_ICON: &str = "icon";

// https://github.com/XMLTV/xmltv/blob/master/xmltv.dtd


#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub enum XmlTagIcon {
    #[default]
    Undefined,
    Src(String),
    Exists,
}

#[derive(Debug, Clone)]
pub struct XmlTag {
    pub name: String,
    pub value: Option<String>,
    pub attributes: Option<HashMap<String, String>>,
    pub children: Option<Vec<XmlTag>>,
    pub icon: XmlTagIcon,
    pub normalized_epg_ids: Option<Vec<String>>,
}

impl XmlTag {
    pub(crate) fn new(name: String, attribs: Option<HashMap<String, String>>) -> Self {
        Self {
            name,
            value: None,
            attributes: attribs,
            children: None,
            icon: Undefined,
            normalized_epg_ids: None,
        }
    }

    pub fn get_attribute_value(&self, attr_name: &str) -> Option<&String> {
        self.attributes.as_ref().and_then(|attr| attr.get(attr_name))
    }

    fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<(), Error> {
        let mut elem = BytesStart::new(self.name.as_str());

        // empty icon not processed
        if self.icon == Undefined && self.name.eq(EPG_TAG_ICON) {
            return Ok(());
        }

        if let Some(attribs) = self.attributes.as_ref() {
            for (k, v) in attribs { elem.push_attribute((k.as_str(), v.as_str())); }
        }
        writer.write_event(Event::Start(elem))?;
        if let Some(text) = self.value.as_ref() {
            writer.write_event(Event::Text(BytesText::new(text.as_str())))?;
        }
        if let Some(children) = &self.children {
            for child in children {
                child.write_to(writer)?;
            }
        }
        Ok(writer.write_event(Event::End(BytesEnd::new(self.name.as_str())))?)
    }
}


#[derive(Debug, Clone)]
pub struct Epg {
    pub priority: i16,
    pub logo_override: bool,
    pub attributes: Option<HashMap<String, String>>,
    pub children: Vec<XmlTag>,
}

impl Epg {
    pub fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<(), quick_xml::Error> {
        let mut elem = BytesStart::new("tv");
        if let Some(attribs) = self.attributes.as_ref() {
            for (k, v) in attribs { elem.push_attribute((k.as_str(), v.as_str())); }
        }
        writer.write_event(Event::Start(elem))?;
        for child in &self.children {
            child.write_to(writer)?;
        }
        Ok(writer.write_event(Event::End(BytesEnd::new("tv")))?)
    }
}

#[derive(Debug, Clone)]
pub struct PersistedEpgSource {
    pub file_path: PathBuf,
    pub priority: i16,
    pub logo_override: bool,
}

#[derive(Debug, Clone)]
pub struct TVGuide {
    epg_sources: Vec<PersistedEpgSource>,
}

impl TVGuide {
    pub fn new(mut epg_sources: Vec<PersistedEpgSource>) -> Self {
        epg_sources.sort_by(|a, b| a.priority.cmp(&b.priority));
        Self {
            epg_sources,
        }
    }

    #[inline]
    pub fn get_epg_sources(&self) -> &Vec<PersistedEpgSource> {
        &self.epg_sources
    }
}


fn filter_channels_and_programmes(
    channels: &mut Vec<EpgChannel>,
    programmes: &mut Vec<EpgProgramme>,
) {
    for channel in channels.iter_mut() {
        let mut i = 0;
        while i < programmes.len() {
            if programmes[i].channel == channel.id {
                let prog = programmes.swap_remove(i);
                channel.programmes.push(prog);
            } else {
                i += 1;
            }
        }

        channel.programmes.sort_by_key(|p| p.start);
    }

    channels.retain(|c| !c.programmes.is_empty());
}

fn get_epg_interval(channels: &Vec<EpgChannel>) -> (i64, i64) {
    let mut epg_start = i64::MAX;
    let mut epg_stop = i64::MIN;
    for channel in channels {
        for programme in &channel.programmes {
            epg_start = min(epg_start, programme.start);
            epg_stop = max(epg_stop, programme.stop);
        }
    }
    (epg_start, epg_stop)
}

pub async fn parse_xmltv_for_web_ui_from_file(path: &Path) -> Result<EpgTv, TuliproxError> {
    let file = tokio::fs::File::open(path).map_err(|err| TuliproxError::new(TuliproxErrorKind::Info, err.to_string())).await?;
    parse_xmltv_for_web_ui(file).await
}

pub async fn parse_xmltv_for_web_ui_from_url(app_state: &Arc<AppState>, url: &str) -> Result<EpgTv, TuliproxError> {
    if let Ok(request_url) = Url::parse(url) {
       match get_remote_content_as_stream(
            Arc::clone(&app_state.http_client.load()),
            &request_url,
            InputFetchMethod::GET,
            None,
        ).await {
           Ok((stream, _url)) => {
               parse_xmltv_for_web_ui(stream).await
           }
           Err(err) => Err(TuliproxError::new(TuliproxErrorKind::Info, format!("Failed to download: {} {err}", sanitize_sensitive_info(url))))
       }

    } else {
        Err(TuliproxError::new(TuliproxErrorKind::Info, format!("Invalid url: {}", sanitize_sensitive_info(url))))
    }
}

#[allow(clippy::too_many_lines)]
async fn parse_xmltv_for_web_ui<R: AsyncRead + Send + Unpin>(reader: R) -> Result<EpgTv, TuliproxError> {

    let mut reader = quick_xml::reader::Reader::from_reader(tokio::io::BufReader::new(reader));
    let mut buf = Vec::new();

    let mut channels = Vec::new();
    let mut programmes = Vec::new();

    let mut current_channel: Option<EpgChannel> = None;
    let mut current_programme: Option<EpgProgramme> = None;

    let mut current_tag = String::new();

    // only 1 day old epg
    let now = Utc::now();
    let yesterday_start = Utc.with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0).unwrap()
        - chrono::Duration::days(1);
    let threshold_ts = yesterday_start.timestamp();

    let get_attr_value = |attr: &quick_xml::events::attributes::Attribute| {
        if let Ok(value) = attr.unescape_value() {
            return Some(value.to_string());
        }
        None
    };

    loop {
        match reader.read_event_into_async(&mut buf).await {
            Ok(Event::Start(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                current_tag.clone_from(&tag);

                match tag.as_str() {
                    EPG_TAG_CHANNEL => {
                        let mut id = None;
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"id" {
                                if let Some(value) = get_attr_value(&attr) {
                                    id = Some(value);
                                    break;
                                }
                            }
                        }
                        if let Some(cid) = id {
                            current_channel = Some(EpgChannel::new(cid));
                        } else {
                            current_channel = None;
                        }
                    }
                    EPG_TAG_PROGRAMME => {
                        let mut start = None;
                        let mut stop = None;
                        let mut channel = None;
                        current_programme = None;
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"start" => start = get_attr_value(&attr),
                                b"stop" => stop = get_attr_value(&attr),
                                b"channel" => channel = get_attr_value(&attr),
                                _ => {}
                            }
                        }
                        if let (Some(pstart), Some(pstop), Some(pchannel)) = (start, stop, channel) {
                            if let (Some(start_time), Some(stop_time)) = (parse_xmltv_time(&pstart), parse_xmltv_time(&pstop)) {
                                if stop_time >= threshold_ts {
                                    let epg_programme = EpgProgramme::new(start_time, stop_time, pchannel);
                                    current_programme = Some(epg_programme);
                                }
                            }
                        }
                    }
                    EPG_TAG_ICON => {
                        if let Some(channel) = &mut current_channel {
                            if channel.icon.is_none() {
                                for attr in e.attributes().flatten() {
                                    if attr.key.as_ref() == b"src" {
                                      if let Some(icon) = get_attr_value(&attr) {
                                          if !icon.is_empty() {
                                              channel.icon = Some(icon);
                                          }
                                      }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(e)) => {
                if let Ok(decoded) = e.decode() {
                    let text = decoded.trim();
                    if !text.is_empty() {
                        if let Some(channel) = &mut current_channel {
                            if current_tag == EPG_TAG_DISPLAY_NAME && channel.title.is_empty() {
                                channel.title = text.to_string();
                            }
                        }

                        if let Some(program) = &mut current_programme {
                            if current_tag == "title" && program.title.is_empty() {
                                program.title = text.to_string();
                            }
                        }
                    }
                }
            }
            Ok(Event::End(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                match tag.as_str() {
                    EPG_TAG_CHANNEL => {
                        if let Some(channel) = current_channel.take() {
                            channels.push(channel);
                        }
                    }
                    EPG_TAG_PROGRAMME => {
                        if let Some(program) = current_programme.take() {
                            programmes.push(program);
                        }
                    }
                    _ => {}
                }
                current_tag.clear();
            }
            Ok(Event::Eof) => break,
            Err(err) => return Err(TuliproxError::new(TuliproxErrorKind::Info, err.to_string())),
            _ => {}
        }

        buf.clear();
    }

    filter_channels_and_programmes(&mut channels, &mut programmes);

    if channels.is_empty() {
        return Ok(EpgTv {
            start: 0,
            stop: 0,
            channels,
        })
    }

    let (epg_start, epg_stop) = get_epg_interval(&channels);

    Ok(EpgTv {
        start: epg_start,
        stop: epg_stop,
        channels,
    })
}
