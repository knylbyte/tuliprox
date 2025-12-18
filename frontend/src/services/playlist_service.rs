use crate::services::{get_base_href, request_post, ACCEPT_PREFER_BIN};
use log::error;
use shared::model::{
    EpgTv, PlaylistCategoriesResponse, PlaylistEpgRequest, PlaylistRequest, UiPlaylistCategories,
    WebplayerUrlRequest, XtreamCluster,
};
use shared::utils::concat_path_leading_slash;
use std::rc::Rc;

pub struct PlaylistService {
    target_update_api_path: String,
    playlist_api_path: String,
    playlist_api_webplayer_url_path: String,
    playlist_api_epg_path: String,
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
            playlist_api_webplayer_url_path: concat_path_leading_slash(
                &base_href,
                "api/v1/playlist/webplayer",
            ),
            playlist_api_epg_path: concat_path_leading_slash(&base_href, "api/v1/playlist/epg"),
        }
    }
    pub async fn update_targets(&self, targets: &[&str]) -> bool {
        request_post::<&[&str], ()>(&self.target_update_api_path, targets, None, None)
            .await
            .map_or_else(|_err| false, |_| true)
    }

    pub async fn get_playlist_categories(
        &self,
        playlist_request: &PlaylistRequest,
    ) -> Option<Rc<UiPlaylistCategories>> {
        request_post::<&PlaylistRequest, PlaylistCategoriesResponse>(
            &self.playlist_api_path,
            playlist_request,
            None,
            Some(ACCEPT_PREFER_BIN.to_string()),
        )
        .await
        .map_or_else(
            |err| {
                error!("{err}");
                None
            },
            |response| response.map(|resp| Rc::new(resp.into())),
        )
    }

    pub async fn get_playlist_webplayer_url(
        &self,
        target_id: u16,
        virtual_id: u32,
        cluster: XtreamCluster,
    ) -> Option<String> {
        let request = WebplayerUrlRequest {
            target_id,
            virtual_id,
            cluster,
        };
        request_post::<&WebplayerUrlRequest, String>(
            &self.playlist_api_webplayer_url_path,
            &request,
            None,
            Some("text/plain".to_string()),
        )
        .await
        .unwrap_or_else(|err| {
            error!("{err}");
            None
        })
    }

    pub async fn get_playlist_epg(&self, request: PlaylistEpgRequest) -> Option<EpgTv> {
        request_post::<&PlaylistEpgRequest, EpgTv>(
            &self.playlist_api_epg_path,
            &request,
            None,
            Some(ACCEPT_PREFER_BIN.to_string()),
        )
        .await
        .unwrap_or_else(|err| {
            error!("{err}");
            None
        })
    }
}
