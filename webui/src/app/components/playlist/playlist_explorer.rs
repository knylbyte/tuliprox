use std::rc::Rc;
use crate::app::context::PlaylistExplorerContext;
use yew::prelude::*;
use yew_i18n::use_translation;
use shared::model::{PlaylistResponseGroup, UiPlaylistGroup, XtreamCluster};

enum ExplorerLevel {
    Categories,
    Groups(XtreamCluster),
    Group(Rc<PlaylistResponseGroup>),
}

#[function_component]
pub fn PlaylistExplorer() -> Html {
    let translate = use_translation();
    let context = use_context::<PlaylistExplorerContext>().expect("PlaylistExlorer context not found");
    let current_item = use_state(|| ExplorerLevel::Categories);

    let render_cluster = |cluster: &Vec<Rc<UiPlaylistGroup>>| {
        cluster.iter()
            .map(|group| html! {
                <div class="tp__playlist-explorer__category">{ group.title.clone() }</div>
            })
            .collect::<Html>()
    };

    let render_categories = || {
        html! {
        <div class="tp__playlist-explorer__categories">
            <div class="tp__playlist-explorer__categories-list">
                { context.playlist.as_ref()
                    .and_then(|response| response.live.as_ref())
                    .map(render_cluster)
                    .unwrap_or_default()
                }
            </div>
            <div class="tp__playlist-explorer__categories-list">
                { context.playlist.as_ref()
                    .and_then(|response| response.vod.as_ref())
                    .map(render_cluster)
                    .unwrap_or_default()
                }
            </div>
            <div class="tp__playlist-explorer__categories-list">
                { context.playlist.as_ref()
                    .and_then(|response| response.series.as_ref())
                    .map(render_cluster)
                    .unwrap_or_default()
                }
            </div>
        </div>
    }
    };

    html! {
      <div class="tp__playlist-explorer">
        <div class="tp__playlist-explorer__body">
          {
            match *current_item {
                ExplorerLevel::Categories => html!{render_categories()} ,
                ExplorerLevel::Groups(ref cluster) => html!{},
                ExplorerLevel::Group(ref groups) => html!{},
            }
          }
        </div>
      </div>
    }
}