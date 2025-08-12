use crate::services::{get_base_href, request_post};
use log::error;
use shared::model::{PlaylistCategoriesResponse, PlaylistRequest, UiPlaylistCategories};
use std::rc::Rc;
use shared::utils::concat_path_leading_slash;

pub struct PlaylistService {
    target_update_api_path: String,
    playlist_api_path: String,
}
impl Default for PlaylistService {
    fn default() -> Self {
        Self::new()
    }
}

impl PlaylistService {
    pub fn new() -> Self {
        let base_href = get_base_href();
        Self {
            target_update_api_path: concat_path_leading_slash(&base_href, "api/v1/playlist/update"),
            playlist_api_path: concat_path_leading_slash(&base_href, "api/v1/playlist"),
        }
    }
    pub async fn update_targets(&self, targets: &[&str]) -> bool {
        request_post::<&[&str], ()>(&self.target_update_api_path, targets, None).await.map_or_else(|err| {
            error!("{err}");
            false
        }, |_| true)
    }

    pub async fn get_playlist_categories(&self, playlist_request: &PlaylistRequest) -> Option<Rc<UiPlaylistCategories>> {
        request_post::<&PlaylistRequest, PlaylistCategoriesResponse>(&self.playlist_api_path, playlist_request, None).await.map_or_else(|err| {
            error!("{err}");
            None
        }, |response| Some(Rc::new(response.into())))
    }
}
