use crate::services::request_post;
use log::error;
use shared::model::{PlaylistCategoriesResponse, PlaylistRequest, UiPlaylistCategories};
use std::rc::Rc;

const TARGET_UPDATE_API_PATH: &str = "/api/v1/playlist/update";
const PLAYLIST_API_PATH: &str = "/api/v1/playlist";

pub struct PlaylistService {}
impl Default for PlaylistService {
    fn default() -> Self {
        Self::new()
    }
}

impl PlaylistService {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn update_targets(&self, targets: &[&str]) -> bool {
        request_post::<&[&str], ()>(TARGET_UPDATE_API_PATH, targets).await.map_or_else(|err| {
            error!("{err}");
            false
        }, |_| true)
    }

    pub async fn get_playlist_categories(&self, playlist_request: &PlaylistRequest) -> Option<Rc<UiPlaylistCategories>> {
        request_post::<&PlaylistRequest, PlaylistCategoriesResponse>(PLAYLIST_API_PATH, playlist_request).await.map_or_else(|err| {
            error!("{err}");
            None
        }, |response| Some(Rc::new(response.into())))
    }
}
