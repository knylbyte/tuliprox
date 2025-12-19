use crate::model::XtreamMappingOptions;
use crate::model::{xtream_playlistitem_to_document, AppConfig, ProxyUserCredentials};
use crate::model::ConfigTarget;
use crate::repository::indexed_document::IndexedDocumentIterator;
use crate::repository::user_repository::user_get_bouquet_filter;
use crate::repository::xtream_repository::{xtream_get_file_paths, xtream_get_storage_path};
use crate::utils::FileReadGuard;
use log::error;
use serde_json::Value;
use shared::error::info_err;
use shared::error::TuliproxError;
use shared::model::{PlaylistItemType, TargetType, XtreamCluster, XtreamPlaylistItem};
use std::collections::HashSet;

pub struct XtreamPlaylistIterator {
    reader: IndexedDocumentIterator<u32, XtreamPlaylistItem>,
    options: XtreamMappingOptions,
    cluster: XtreamCluster,
    // Use parsed numeric filter to avoid per-item String allocations (no to_string per check)
    filter_ids: Option<HashSet<u32>>,
    base_url: String,
    user: ProxyUserCredentials,
    lookup_item: Option<(XtreamPlaylistItem, bool)>,  // this is for filtered iteration
    _file_lock: FileReadGuard,
}

impl XtreamPlaylistIterator {
    pub async fn new(
        cluster: XtreamCluster,
        app_config: &AppConfig,
        target: &ConfigTarget,
        category_id: Option<u32>,
        user: &ProxyUserCredentials,
    ) -> Result<Self, TuliproxError> {

        // TODO use playlist memory cache and keep sorted

        let xtream_output = target.get_xtream_output().ok_or_else(|| info_err!(format!("Unexpected: xtream output required for target {}", target.name)))?;
        let config = app_config.config.load();
        if let Some(storage_path) = xtream_get_storage_path(&config, target.name.as_str()) {
            let (xtream_path, idx_path) = xtream_get_file_paths(&storage_path, cluster);
            if !xtream_path.exists() || !idx_path.exists() {
                return Err(info_err!(format!("No {cluster} entries found for target {}", &target.name)));
            }
            let file_lock = app_config.file_locks.read_lock(&xtream_path).await;

            let reader = IndexedDocumentIterator::<u32, XtreamPlaylistItem>::new(&xtream_path, &idx_path)
                .map_err(|err| info_err!(format!("Could not deserialize file {xtream_path:?} - {err}")))?;

            let options = XtreamMappingOptions::from_target_options(target, xtream_output, app_config);
            let server_info = app_config.get_user_server_info(user);

            let filter = user_get_bouquet_filter(&config, &user.username, category_id, TargetType::Xtream, cluster).await;
            // Parse bouquet filter (strings) once into u32 set to minimize per-item allocations
            let filter_ids: Option<HashSet<u32>> = filter.as_ref().map(|set| {
                set.iter().filter_map(|s| {
                    s.parse::<u32>().map_err(|e| {
                        error!("Failed to parse bouquet filter id '{s}': {e}");
                        e
                    }).ok()
                }).collect()
            });

            Ok(Self {
                reader,
                options,
                cluster,
                filter_ids,
                _file_lock: file_lock,
                base_url: server_info.get_base_url(),
                user: user.clone(),
                lookup_item: None,
            })
        } else {
            Err(info_err!(format!("Failed to find xtream storage for target {}", &target.name)))
        }
    }

    fn matches_filters(cluster: XtreamCluster, filter_ids: Option<&HashSet<u32>>, item: &XtreamPlaylistItem) -> bool {

        if cluster == XtreamCluster::Series
            && !matches!(item.item_type, PlaylistItemType::SeriesInfo | PlaylistItemType::LocalSeriesInfo) {
                return false;
            }

        // category_id-Filter
        if let Some(set) = filter_ids {
            if !set.contains(&item.category_id) {
                return false;
            }
        }

        true
    }

    fn get_next(&mut self) -> Option<(XtreamPlaylistItem, bool)> {
        if self.reader.has_error() {
            error!("Could not deserialize xtream item: {}", self.reader.get_path().display());
            return None;
        }

        let filter_ids = self.filter_ids.as_ref();
        let cluster = self.cluster;

        let predicate = |(item, _): &(XtreamPlaylistItem, bool)| {
            Self::matches_filters(cluster, filter_ids, item)
        };

        if self.cluster == XtreamCluster::Series || self.filter_ids.is_some() {
            if let Some((current_item, _)) = self.lookup_item.take() {
                let next_valid = self.reader.find(predicate);
                self.lookup_item = next_valid;
                let has_next = self.lookup_item.is_some();
                Some((current_item, has_next))
            } else {
                let current_item = self.reader.find(predicate);
                if let Some((item, _)) = current_item {
                    self.lookup_item = self.reader.find(predicate);
                    let has_next = self.lookup_item.is_some();
                    Some((item, has_next))
                } else {
                    None
                }
            }
        } else {
            self.reader.next()
        }
    }
}

impl Iterator for XtreamPlaylistIterator {
    type Item = (XtreamPlaylistItem, bool);
    fn next(&mut self) -> Option<Self::Item> {
        self.get_next()
    }
}


pub struct XtreamPlaylistJsonIterator {
    inner: XtreamPlaylistIterator,
}

impl XtreamPlaylistJsonIterator {
    pub async fn new(
        cluster: XtreamCluster,
        config: &AppConfig,
        target: &ConfigTarget,
        category_id: Option<u32>,
        user: &ProxyUserCredentials,
    ) -> Result<Self, TuliproxError> {
        Ok(Self {
            inner: XtreamPlaylistIterator::new(cluster, config, target, category_id, user).await?
        })
    }
}

pub fn to_doc(pli: &XtreamPlaylistItem, url: &str, options: &XtreamMappingOptions, user: &ProxyUserCredentials) -> Value {
    xtream_playlistitem_to_document(pli, url, options, user)
}

impl Iterator for XtreamPlaylistJsonIterator {
    type Item = (String, bool);
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.get_next().map(|(pli, has_next)| (to_doc(&pli, &self.inner.base_url, &self.inner.options, &self.inner.user).to_string(), has_next))
    }
}

