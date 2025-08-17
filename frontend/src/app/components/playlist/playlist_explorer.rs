use std::fmt::Display;
use std::rc::Rc;
use std::str::FromStr;
use wasm_bindgen::JsCast;
use yew::platform::spawn_local;
use crate::app::context::PlaylistExplorerContext;
use yew::prelude::*;
use yew_hooks::use_clipboard;
use yew_i18n::use_translation;
use shared::create_tuliprox_error_result;
use shared::error::{TuliproxError, TuliproxErrorKind};
use shared::model::{CommonPlaylistItem, PlaylistRequestType, SearchRequest, UiPlaylistGroup, XtreamCluster};
use crate::app::components::{AppIcon, IconButton, NoContent, Search};
use crate::app::components::menu_item::MenuItem;
use crate::app::components::popup_menu::PopupMenu;
use crate::hooks::use_service_context;
use crate::html_if;
use crate::model::{BusyStatus, EventMessage};

#[derive(Debug, Clone, Eq, PartialEq)]
enum ExplorerAction {
    CopyLinkTuliprox,
    CopyLinkProvider,
}

impl Display for ExplorerAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", match self {
            Self::CopyLinkTuliprox => "copy_link_tuliprox",
            Self::CopyLinkProvider => "copy_link_provider",
        })
    }
}

impl FromStr for ExplorerAction {
    type Err = TuliproxError;

    fn from_str(s: &str) -> Result<Self, TuliproxError> {
        if s.eq("copy_link_tuliprox") {
            Ok(Self::CopyLinkTuliprox)
        } else if s.eq("copy_link_provider") {
            Ok(Self::CopyLinkProvider)
        } else {
            create_tuliprox_error_result!(TuliproxErrorKind::Info, "Unknown InputType: {}", s)
        }
    }
}

enum ExplorerLevel {
    Categories,
    Group(Rc<UiPlaylistGroup>),
}

#[function_component]
pub fn PlaylistExplorer() -> Html {
    let context = use_context::<PlaylistExplorerContext>().expect("PlaylistExplorer context not found");
    let translate = use_translation();
    let service_ctx = use_service_context();
    let current_item = use_state(|| ExplorerLevel::Categories);
    let playlist = use_state(|| (*context.playlist).clone());
    let selected_channel = use_state(|| None::<Rc<CommonPlaylistItem>>);
    let popup_anchor_ref = use_state(|| None::<web_sys::Element>);
    let popup_is_open = use_state(|| false);
    let clipboard = use_clipboard();

    let handle_popup_close = {
        let set_is_open = popup_is_open.clone();
        Callback::from(move |()| {
            set_is_open.set(false);
        })
    };

    let handle_popup_onclick = {
        let set_selected_channel = selected_channel.clone();
        let set_anchor_ref = popup_anchor_ref.clone();
        let set_is_open = popup_is_open.clone();
        Callback::from(move |(dto, event): (Rc<CommonPlaylistItem>, MouseEvent)| {
            if let Some(target) = event.target_dyn_into::<web_sys::Element>() {
                set_selected_channel.set(Some(dto.clone()));
                set_anchor_ref.set(Some(target));
                set_is_open.set(true);
            }
        })
    };

    {
        let set_playlist = playlist.clone();
        let set_current_item = current_item.clone();
        let set_selected_channel = selected_channel.clone();
        let set_popup_is_open = popup_is_open.clone();
        let set_anchor_ref = popup_anchor_ref.clone();
        use_effect_with(context.playlist.clone(), move |new_playlist| {
            set_current_item.set(ExplorerLevel::Categories);
            set_playlist.set((**new_playlist).clone());
            // Reset popup state and selection when the underlying data changes
            set_selected_channel.set(None);
            set_popup_is_open.set(false);
            set_anchor_ref.set(None);
            || {}
        });
    }

    let copy_to_clipboard ={
        let clipboard = clipboard.clone();
        move |text: String| {
            clipboard.write_text(text);
        }
    };

    let handle_menu_click = {
        let popup_is_open_state = popup_is_open.clone();
        let selected_channel = selected_channel.clone();
        Callback::from(move |(name, _): (String, _)| {
            if let Ok(action) = ExplorerAction::from_str(&name) {
                match action {
                    ExplorerAction::CopyLinkTuliprox => {
                        if let Some(dto) = &*selected_channel {
                            copy_to_clipboard(dto.virtual_id.to_string());
                        }
                    }
                    ExplorerAction::CopyLinkProvider => {
                        if let Some(dto) = &*selected_channel {
                            copy_to_clipboard(dto.url.clone());
                        }
                    }
                }
            }
            popup_is_open_state.set(false);
        })
    };

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
                    if let Some(target)  = e.target() {
                        let img = target.dyn_into::<web_sys::HtmlMediaElement >().unwrap();
                        img.set_src("assets/missing-logo.svg");
                    }
                    })}
                />}
        }
    };

    let render_group = |group: &Rc<UiPlaylistGroup>| {
        html! {
                <div class="tp__playlist-explorer__group">
                  <div class="tp__playlist-explorer__group-list">
                  {
                      group.channels.iter().map(|chan| {
                        let chan_clone = chan.clone();
                        let popup_onclick = handle_popup_onclick.clone();
                        html! {
                            <span class="tp__playlist-explorer__item tp__playlist-explorer__channel">
                                <button class="tp__icon-button"
                                    onclick={Callback::from(move |event: MouseEvent| popup_onclick.emit((chan_clone.clone(), event)))}>
                                    <AppIcon name="Popup"></AppIcon>
                                </button>
                                {render_channel_logo(chan)}
                                {chan.title.clone()}
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

        <PopupMenu is_open={*popup_is_open} anchor_ref={(*popup_anchor_ref).clone()} on_close={handle_popup_close}>
            { html_if!((*context.playlist_request_type).as_ref() == Some(&PlaylistRequestType::Target), {
                 <MenuItem icon="Clipboard" name={ExplorerAction::CopyLinkTuliprox.to_string()} label={translate.t("LABEL.COPY_LINK_TULIPROX")} onclick={&handle_menu_click}></MenuItem>
             })
            }
            <MenuItem icon="Clipboard" name={ExplorerAction::CopyLinkProvider.to_string()} label={translate.t("LABEL.COPY_LINK_PROVIDER")} onclick={&handle_menu_click}></MenuItem>
        </PopupMenu>
      </div>
    }
}
