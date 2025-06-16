use crate::model::{ConfigTarget, PlaylistGroup, PlaylistItem, XtreamCluster};
use crate::model::{MatchType, TraktConfig, TraktContentType, TraktListConfig, TraktListItem, TraktMatchItem, TraktMatchResult};
use crate::tuliprox_error::TuliproxError;
use crate::utils::trakt::client::TraktClient;
use crate::utils::trakt::extract_year_from_title;
use crate::utils::trakt::normalize_title_for_matching;
use crate::utils::get_u32_from_serde_value;
use log::{debug, info, trace, warn};
use std::sync::Arc;
use strsim::jaro_winkler;

/// Utility functions for content type compatibility
fn should_include_item(item: &TraktListItem, content_type: &TraktContentType) -> bool {
    match content_type {
        TraktContentType::Vod => item.content_type == TraktContentType::Vod,
        TraktContentType::Series => item.content_type == TraktContentType::Series,
        TraktContentType::Both => true,
    }
}

fn is_compatible_content_type(cluster: XtreamCluster, content_type: &TraktContentType) -> bool {
    match content_type {
        TraktContentType::Vod => cluster == XtreamCluster::Video,
        TraktContentType::Series => cluster == XtreamCluster::Series,
        TraktContentType::Both => matches!(cluster, XtreamCluster::Video | XtreamCluster::Series),
    }
}

/// Extract TMDB ID from playlist item
fn extract_tmdb_id_from_playlist_item(item: &PlaylistItem) -> Option<u32> {
    if let Some(additional_props) = &item.header.additional_properties {
        if let Some(props_str) = additional_props.as_str() {
            if let Ok(props) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(props_str) {
                if let Some(tmdb_value) = props.get("tmdb") {
                    return get_u32_from_serde_value(tmdb_value);
                }
                if let Some(tmdb_id_value) = props.get("tmdb_id") {
                    return get_u32_from_serde_value(tmdb_id_value);
                }
            }
        }
    }
    None
}

fn calculate_year_bonus(playlist_year: Option<u32>, trakt_year: Option<u32>) -> f64 {
    match (playlist_year, trakt_year) {
        (Some(p_year), Some(t_year)) => {
            if p_year == t_year {
                // Perfect year match gets substantial bonus
                0.15
            } else {
                let year_diff = p_year.abs_diff(t_year);

                if year_diff <= 1 {
                    // 1-year difference gets small bonus (could be release date differences)
                    0.05
                } else if year_diff <= 3 {
                    // 2-3 years difference gets small penalty
                    -0.05
                } else {
                    // More than 3 years difference gets larger penalty
                    -0.15
                }
            }
        }
        (Some(_), None) | (None, Some(_)) => {
            // One has year, other doesn't - small penalty
            -0.05
        }
        (None, None) => {
            // Neither has year - no bonus/penalty
            0.0
        }
    }
}

fn find_best_fuzzy_match_for_item<'a>(channel: &'a PlaylistItem, trakt_items: &'a [TraktMatchItem], list_config: &'a TraktListConfig) -> Option<TraktMatchResult<'a>> {
    // Try fuzzy matching if no exact match found
    let normalized_playlist_title = normalize_title_for_matching(&channel.header.title);
    let playlist_year = extract_year_from_title(&channel.header.title);
    let threshold = f64::from(list_config.fuzzy_match_threshold) / 100.0;
    let mut best_match: Option<(&TraktMatchItem, f64)> = None;

    for trakt_item in trakt_items {
        let title_score = jaro_winkler(&normalized_playlist_title, &trakt_item.normalized_title);

        // Calculate year bonus
        let year_bonus = calculate_year_bonus(playlist_year, trakt_item.year);
        let mut combined_score = title_score + year_bonus;

        // Clamp score to [0.0, 1.0]
        combined_score = combined_score.clamp(0.0, 1.0);

        // Check if this is the best match so far and meets threshold
        if combined_score >= threshold {
            if let Some((_, current_best_score)) = &best_match {
                if combined_score > *current_best_score {
                    best_match = Some((trakt_item, combined_score));
                }
            } else {
                best_match = Some((trakt_item, combined_score));
            }
        }
    }

    if let Some((trakt_item, combined_score)) = best_match {
        let match_type = if playlist_year.is_some() && trakt_item.year.is_some() {
            MatchType::FuzzyTitleYear
        } else {
            MatchType::FuzzyTitle
        };

        debug!("Fuzzy match: '{}' -> '{}' (final: {combined_score:.3}, type: {match_type:?})",
                      channel.header.title, trakt_item.title);

        return Some(TraktMatchResult {
            playlist_item: channel,
            trakt_item,
            match_score: combined_score,
            match_type: match_type.clone(),
        });
    }

    None
}

fn find_best_match_for_item<'a>(
    channel: &'a PlaylistItem,
    trakt_items: &'a [TraktMatchItem<'a>],
    list_config: &'a TraktListConfig,
) -> Option<TraktMatchResult<'a>> {
    // Try TMDB exact matching first
    if let Some(playlist_tmdb_id) = extract_tmdb_id_from_playlist_item(channel) {
        for trakt_item in trakt_items {
            if Some(playlist_tmdb_id) == trakt_item.tmdb_id {
                trace!("TMDB exact match: '{}' (TMDB: {})", channel.header.title, playlist_tmdb_id);
                return Some(TraktMatchResult {
                    playlist_item: channel,
                    trakt_item,
                    match_score: 1.0,
                    match_type: MatchType::TmdbExact,
                });
            }
        }
    }

    find_best_fuzzy_match_for_item(channel, trakt_items, list_config)
}


fn create_category_from_matches<'a>(
    matches: Vec<TraktMatchResult<'a>>,
    list_config: &'a TraktListConfig,
) -> Option<PlaylistGroup> {
    if matches.is_empty() { return None; }

    let mut matched_items = Vec::new();

    let mut sorted_matches = matches;
    // Simple sort by rank only
    sorted_matches.sort_by(|a, b| {
        a.trakt_item.rank.unwrap_or(9999).cmp(&b.trakt_item.rank.unwrap_or(9999))
    });

    for match_result in sorted_matches {
        let mut modified_item = match_result.playlist_item.clone();
        // Use the (possibly numbered) title from the match result (which now contains the original playlist title)
        modified_item.header.title.clone_from(&match_result.trakt_item.title.to_string());
        // Synchronize name with title so both fields show the same value
        modified_item.header.name.clone_from(&match_result.trakt_item.title.to_string());
        matched_items.push(modified_item);
    }

    let cluster = match list_config.content_type {
        TraktContentType::Vod => XtreamCluster::Video,
        TraktContentType::Series => XtreamCluster::Series,
        TraktContentType::Both => {
            matched_items.first()
                .map_or(XtreamCluster::Video, |item| item.header.xtream_cluster)
        }
    };

    if matched_items.is_empty() { return None; }

    Some(PlaylistGroup {
        id: 0,
        title: list_config.category_name.clone(),
        channels: matched_items,
        xtream_cluster: cluster,
    })
}

fn match_trakt_items_with_playlist<'a>(
    trakt_items: &'a [TraktListItem],
    playlist: &'a [PlaylistGroup],
    list_config: &'a TraktListConfig,
) -> Option<PlaylistGroup> {
    let trakt_match_items: Vec<TraktMatchItem<'a>> = trakt_items
        .iter()
        .filter(|item| should_include_item(item, &list_config.content_type))
        .filter_map(TraktMatchItem::from_trakt_list_item)
        .collect();

    debug!("Matching {} Trakt items against playlist for content type {:?}", trakt_match_items.len(), list_config.content_type);

    let mut matches = Vec::new();
    for playlist_group in playlist {
        for channel in &playlist_group.channels {
            if is_compatible_content_type(channel.header.xtream_cluster, &list_config.content_type) {
                matches.extend(find_best_match_for_item(channel, &trakt_match_items, list_config));
            }
        }
    }

    create_category_from_matches(matches, list_config)
}

pub struct TraktCategoriesProcessor {
    client: TraktClient,
}

impl TraktCategoriesProcessor {
    pub fn new(http_client: Arc<reqwest::Client>, trakt_config: &TraktConfig) -> Self {
        let client = TraktClient::new(http_client, trakt_config.api.clone());
        Self { client }
    }

    pub async fn process_trakt_categories(
        &self,
        playlist: &mut [PlaylistGroup],
        target: &ConfigTarget,
        trakt_config: &TraktConfig,
    ) -> Result<Vec<PlaylistGroup>, Vec<TuliproxError>> {
        if trakt_config.lists.is_empty() {
            debug!("No Trakt lists configured for target {}", target.name);
            return Ok(vec![]);
        }

        info!("Processing {} Trakt lists for target {}", trakt_config.lists.len(), target.name);

        let trakt_lists = match self.client.get_all_lists(&trakt_config.lists).await {
            Ok(lists) => lists,
            Err(errors) => {
                warn!("Failed to fetch some Trakt lists: {} errors", errors.len());
                return Err(errors);
            }
        };

        let mut new_categories = Vec::new();
        let mut total_matches = 0;

        for list_config in &trakt_config.lists {
            let cache_key = format!("{}:{}", list_config.user, list_config.list_slug);

            if let Some(trakt_items) = trakt_lists.get(&cache_key) {
                info!("Processing Trakt list {} with {} items", cache_key, trakt_items.len());

                if let Some(category) = match_trakt_items_with_playlist(trakt_items, playlist, list_config) {
                    if !category.channels.is_empty() {
                        total_matches += category.channels.len();
                        let category_len = category.channels.len();
                        new_categories.push(category);
                        info!("Created Trakt category '{}' with {} items",
                             list_config.category_name, category_len);
                    }
                }
            } else {
                warn!("No items found for Trakt list {cache_key}");
            }
        }

        info!("Trakt processing complete: created {} categories with {} total matches",
             new_categories.len(), total_matches);

        Ok(new_categories)
    }

}
pub async fn process_trakt_categories_for_target(
    http_client: Arc<reqwest::Client>,
    playlist: &mut [PlaylistGroup],
    target: &ConfigTarget,
) -> Result<Vec<PlaylistGroup>, Vec<TuliproxError>> {

    let Some(trakt_config) = target.get_xtream_output().and_then(|output| output.trakt_lists.as_ref()) else {
        debug!("No Trakt configuration found for target {}", target.name);
        return Ok(vec![]);
    };

    let processor = TraktCategoriesProcessor::new(http_client, trakt_config);
    processor.process_trakt_categories(playlist, target, trakt_config).await
}