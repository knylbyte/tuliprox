use crate::model::{xtream_mapping_option_from_target_options, AppConfig, ProxyUserCredentials};
use crate::model::ConfigTarget;
use crate::repository::bplustree::{BPlusTreeDiskIteratorOwned, BPlusTreeQuery};
use crate::repository::user_repository::user_get_bouquet_filter;
use crate::repository::xtream_repository::{xtream_get_file_path, xtream_get_storage_path};
use crate::utils::FileReadGuard;
use log::error;
use shared::error::info_err;
use shared::error::TuliproxError;
use shared::model::{PlaylistItemType, TargetType, XtreamCluster, XtreamMappingOptions, XtreamPlaylistItem};
use std::collections::HashSet;

pub struct XtreamPlaylistIterator {
    reader: BPlusTreeDiskIteratorOwned<u32, XtreamPlaylistItem>,
    options: XtreamMappingOptions,
    cluster: XtreamCluster,
    // Use parsed numeric filter to avoid per-item String allocations (no to_string per check)
    filter_ids: Option<HashSet<u32>>,
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
            let xtream_path = xtream_get_file_path(&storage_path, cluster);
            if !xtream_path.exists() {
                return Err(info_err!(format!("No {cluster} entries found for target {}", &target.name)));
            }
            let file_lock = app_config.file_locks.read_lock(&xtream_path).await;

            let query = BPlusTreeQuery::<u32, XtreamPlaylistItem>::try_new(&xtream_path)
                .map_err(|err| info_err!(format!("Could not open BPlusTreeQuery {xtream_path:?} - {err}")))?;
            let reader = query.disk_iter();

            let server_info = app_config.get_user_server_info(user);
            let options = xtream_mapping_option_from_target_options(target, xtream_output, app_config, user, Some(server_info.get_base_url().as_str()));

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
                lookup_item: None,
            })
        } else {
            Err(info_err!(format!("Failed to find xtream storage for target {}", &target.name)))
        }
    }

    fn matches_filters(cluster: XtreamCluster, filter_ids: Option<&HashSet<u32>>, item: &XtreamPlaylistItem) -> bool {
        // We can't serve episodes within series
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
        // reader no longer has manual error state, BPlusTreeQuery handles it via Result elsewhere

        let filter_ids = self.filter_ids.as_ref();
        let cluster = self.cluster;

        let predicate = |(_, item): &(u32, XtreamPlaylistItem)| {
            Self::matches_filters(cluster, filter_ids, item)
        };

        if self.cluster == XtreamCluster::Series || self.filter_ids.is_some() {
            if let Some((current_item, _)) = self.lookup_item.take() {
                let next_valid = self.reader.find(predicate);
                self.lookup_item = next_valid.map(|(_, item)| (item, true));
                let has_next = self.lookup_item.is_some();
                Some((current_item, has_next))
            } else {
                let current_item = self.reader.find(predicate);
                if let Some((_, item)) = current_item {
                    self.lookup_item = self.reader.find(predicate).map(|(_, item)| (item, true));
                    let has_next = self.lookup_item.is_some();
                    Some((item, has_next))
                } else {
                    None
                }
            }
        } else {
            self.reader.next().map(|(_, item)| (item, !self.reader.is_empty()))
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

impl Iterator for XtreamPlaylistJsonIterator {
    type Item = (String, bool);
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.get_next().map(|(pli, has_next)| (pli.to_document(&self.inner.options).to_string(), has_next))
    }
}

