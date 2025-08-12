use std::rc::Rc;
use wasm_bindgen::JsCast;
use yew::platform::spawn_local;
use crate::app::context::PlaylistExplorerContext;
use yew::prelude::*;
use shared::model::{CommonPlaylistItem, SearchRequest, UiPlaylistGroup, XtreamCluster};
use crate::app::components::{IconButton, NoContent, Search};
use crate::hooks::use_service_context;
use crate::model::{BusyStatus, EventMessage};

enum ExplorerLevel {
    Categories,
    Group(Rc<UiPlaylistGroup>),
}

#[function_component]
pub fn PlaylistExplorer() -> Html {
    let context = use_context::<PlaylistExplorerContext>().expect("PlaylistExplorer context not found");
    let service_ctx = use_service_context();
    let current_item = use_state(|| ExplorerLevel::Categories);
    let playlist = use_state(|| (*context.playlist).clone());

    {
        let set_playlist = playlist.clone();
        use_effect_with(context.playlist.clone(), move |new_playlist| {
            set_playlist.set((**new_playlist).clone());
            || {}
        });
    }

    let handle_back_click = {
        let current_item = current_item.clone();
        Callback::from(move |_| {
            match *current_item {
                ExplorerLevel::Categories => {}
                ExplorerLevel::Group(_) => {
                    current_item.set(ExplorerLevel::Categories);
                }
            }
        })
    };

    let handle_search = {
        let services = service_ctx.clone();
        let set_playlist = playlist.clone();
        let set_current_item = current_item.clone();
        let context = context.clone();
        Callback::from(move |search_req| {
            match search_req {
                SearchRequest::Clear => set_playlist.set((*context.playlist).clone()),
                SearchRequest::Text(ref _text, ref _search_fields)
                | SearchRequest::Regexp(ref _text, ref _search_fields) => {
                    services.event.broadcast(EventMessage::Busy(BusyStatus::Show));
                    let set_playlist = set_playlist.clone();
                    let set_current_item = set_current_item.clone();
                    let context = context.clone();
                    let services = services.clone();
                    spawn_local(async move {
                        let filtered = context
                            .playlist
                            .as_ref()
                            .and_then(|categories| categories.filter(&search_req))
                            .map(Rc::new);
                        set_playlist.set(filtered);
                        set_current_item.set(ExplorerLevel::Categories);
                        services.event.broadcast(EventMessage::Busy(BusyStatus::Hide));
                    });
                }
            }
        })
    };

    let handle_category_select = {
        let set_current_item = current_item.clone();
        Callback::from(move |(group, _event): (Rc<UiPlaylistGroup>, MouseEvent)| {
            set_current_item.set(ExplorerLevel::Group(group));
        })
    };

    let render_cluster = |cluster: XtreamCluster, list: &Vec<Rc<UiPlaylistGroup>>| {

        list.iter()
            .map(|group| {
                let group_clone = group.clone();
                let on_click = {
                    let category_select = handle_category_select.clone();
                    Callback::from(move |event: MouseEvent| {
                        category_select.emit((group_clone.clone(), event));
                    })
                };
                html! {
                <span class="tp__playlist-explorer__item" onclick={on_click}>
                {
                 match cluster {
                    XtreamCluster::Live => html! {<span class="tp__playlist-explorer__item-live"></span>},
                    XtreamCluster::Video => html! {<span class="tp__playlist-explorer__item-video"></span>},
                    XtreamCluster::Series => html! {<span class="tp__playlist-explorer__item-series"></span>},
                    }
                }
                { group.title.clone() }</span>
            }})
            .collect::<Html>()
    };

    let render_categories = || {
        if playlist.is_none() {
            html! {
                <NoContent/>
            }
        } else {
          html! {
            <div class="tp__playlist-explorer__categories">
                <div class="tp__playlist-explorer__categories-list">
                    { playlist.as_ref()
                        .and_then(|response| response.live.as_ref())
                        .map(|list| render_cluster(XtreamCluster::Live, list))
                        .unwrap_or_default()
                    }
                    { playlist.as_ref()
                        .and_then(|response| response.vod.as_ref())
                        .map(|list| render_cluster(XtreamCluster::Video, list))
                        .unwrap_or_default()
                    }
                    { playlist.as_ref()
                        .and_then(|response| response.series.as_ref())
                        .map(|list| render_cluster(XtreamCluster::Series, list))
                        .unwrap_or_default()
                    }
                </div>
            </div>
            }
        }
    };

    let render_channel_logo = |chan: &Rc<CommonPlaylistItem>| {
        let logo = if chan.logo.is_empty() { chan.logo_small.as_str() } else { chan.logo.as_str() };
        if logo.is_empty() {
           html! {}
        } else {
            html! { <img alt={"n/a"} src={logo.to_owned()}
                    onerror={Callback::from(move |e: web_sys::Event| {
                        let img = e.target().unwrap().dyn_into::<web_sys::HtmlMediaElement >().unwrap();
                        img.set_src("assets/missing-logo.svg");
                    })}
                />}
        }
    };

    let render_group = |group: &Rc<UiPlaylistGroup>| {
        html! {
                <div class="tp__playlist-explorer__group">
                  <div class="tp__playlist-explorer__group-list">
                  {
                      group.channels.iter().map(|c| {
                        html! {
                            <span class="tp__playlist-explorer__item">
                              {render_channel_logo(c)}  {c.title.clone()}
                            </span>
                          }
                       }).collect::<Html>()
                  }
                  </div>
                </div>
            }
    };

    html! {
      <div class="tp__playlist-explorer">
        <div class="tp__playlist-explorer__header">
            <div class="tp__playlist-explorer__header-toolbar">
                <IconButton class={if matches!(*current_item, ExplorerLevel::Categories) { "disabled" } else {""}} name="back" icon="Back" onclick={handle_back_click} />
                <div class="tp__playlist-explorer__header-toolbar-search">
                  <Search onsearch={handle_search}/>
                </div>
            </div>
        </div>
        <div class="tp__playlist-explorer__body">
          {
            match *current_item {
                ExplorerLevel::Categories => html!{render_categories()} ,
                ExplorerLevel::Group(ref group) => html!{ render_group(group) },
            }
          }
        </div>
      </div>
    }
}