use crate::app::components::menu_item::MenuItem;
use crate::app::components::popup_menu::PopupMenu;
use crate::app::components::{AppIcon, RevealContent, Table, TableDefinition, ToggleSwitch};
use crate::app::ConfigContext;
use crate::hooks::use_service_context;
use crate::utils::t_safe;
use gloo_timers::callback::Interval;
use gloo_utils::window;
use shared::error::{create_tuliprox_error_result, TuliproxError, TuliproxErrorKind};
use shared::model::{PlaylistItemType, ProtocolMessage, SortOrder, StreamChannel, StreamInfo, UserCommand};
use shared::utils::{current_time_secs, strip_port};
use std::fmt::Display;
use std::rc::Rc;
use std::str::FromStr;
use wasm_bindgen::JsCast;
use web_sys::Element;
use yew::prelude::*;
use yew_i18n::use_translation;

const LIVE: &str = "Live";
const MOVIE: &str = "Movie";
const SERIES: &str = "Series";
const CATCHUP: &str = "Archive";
const HLS: &str = "HLS";
const DASH: &str = "DASH";

const HEADERS: [&str; 12] = [
    "EMPTY",
    "USERNAME",
    "STREAM_ID",
    "CLUSTER",
    "CHANNEL",
    "GROUP",
    "CLIENT_IP",
    "COUNTRY",
    "PROVIDER",
    "SHARED",
    "USER_AGENT",
    "DURATION"
];

fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

fn update_timestamps() {
    let window = window();
    let document = window.document().unwrap();
    let spans = document.query_selector_all("span[data-ts]").unwrap();
    for i in 0..spans.length() {
        if let Some(node) = spans.item(i) {
            let el: Element = node.dyn_into().unwrap();
            if let Some(ts_str) = el.get_attribute("data-ts") {
                if let Ok(ts) = ts_str.parse::<u64>() {
                    el.set_inner_html(&format_duration(current_time_secs() - ts));
                }
            }
        }
    }
}

#[derive(Properties, PartialEq, Clone)]
pub struct StreamsTableProps {
    pub streams: Option<Vec<Rc<StreamInfo>>>,
}

#[function_component]
pub fn StreamsTable(props: &StreamsTableProps) -> Html {
    let translate = use_translation();
    let services = use_service_context();
    let config_ctx = use_context::<ConfigContext>().expect("Config context not found");
    let popup_anchor_ref = use_state(|| None::<web_sys::Element>);
    let popup_is_open = use_state(|| false);
    let selected_dto = use_state(|| None::<Rc<StreamInfo>>);

    let headers = use_memo(config_ctx, |cfg| {
        let include_country = if let Some(app_cfg) = &cfg.config {
            app_cfg.config.is_geoip_enabled()
        } else {
            false
        };

        let visible_headers: Vec<&str> = if include_country {
            HEADERS.to_vec() // all headers
        } else {
            HEADERS.iter()
                .filter(|h| **h != "COUNTRY")
                .copied()
                .collect()
        };
        visible_headers
    });


    use_effect_with((), move |_| {
        let interval = Interval::new(1000, update_timestamps);
        move || drop(interval)
    });


    let handle_popup_close = {
        let set_is_open = popup_is_open.clone();
        Callback::from(move |()| {
            set_is_open.set(false);
        })
    };

    let handle_popup_onclick = {
        let set_selected_dto = selected_dto.clone();
        let set_anchor_ref = popup_anchor_ref.clone();
        let set_is_open = popup_is_open.clone();
        Callback::from(move |(dto, event): (Rc<StreamInfo>, MouseEvent)| {
            if let Some(streams) = event.target_dyn_into::<web_sys::Element>() {
                set_selected_dto.set(Some(dto.clone()));
                set_anchor_ref.set(Some(streams));
                set_is_open.set(true);
            }
        })
    };

    let render_header_cell = {
        let translator = translate.clone();
        let headers = headers.clone();
        Callback::<usize, Html>::from(move |col| {
            html! {
                {
                    if col < headers.len() {
                       translator.t(&format!("LABEL.{}", headers[col]))
                    } else {
                      String::new()
                    }
               }
            }
        })
    };

    let render_cluster = |channel: &StreamChannel| -> &str {
        match channel.item_type {
            PlaylistItemType::LiveUnknown
            | PlaylistItemType::Live => LIVE,
            PlaylistItemType::Video => MOVIE,
            PlaylistItemType::Series
            | PlaylistItemType::SeriesInfo => SERIES,
            PlaylistItemType::Catchup => CATCHUP,
            PlaylistItemType::LiveHls => HLS,
            PlaylistItemType::LiveDash => DASH
        }
    };

    let render_data_cell = {
        let popup_onclick = handle_popup_onclick.clone();
        let headers = headers.clone();
        let translate = translate.clone();
        Callback::<(usize, usize, Rc<StreamInfo>), Html>::from(
            move |(row, col, dto): (usize, usize, Rc<StreamInfo>)| {
                match headers[col] {
                    "EMPTY" => {
                        let popup_onclick = popup_onclick.clone();
                        html! {
                            <button class="tp__icon-button"
                                onclick={Callback::from(move |event: MouseEvent| popup_onclick.emit((dto.clone(), event)))}
                                data-row={row.to_string()}>
                                <AppIcon name="Popup"></AppIcon>
                            </button>
                        }
                    }
                    "USERNAME" => html! {dto.username.as_str()},
                    "STREAM_ID" => html! { <>
                            { dto.channel.virtual_id.to_string() }
                            {" ("}
                            { dto.channel.provider_id.to_string() }
                            {")"}
                        </>},
                    "CLUSTER" => html! { render_cluster(&dto.channel) },
                    "CHANNEL" => html! {dto.channel.title.as_str()},
                    "GROUP" => html! {dto.channel.group.as_str()},
                    "CLIENT_IP" => html! { strip_port(&dto.client_ip)},
                    "COUNTRY" => html! { dto.country.as_ref().map_or_else(String::new, |c| t_safe(&translate, &format!("COUNTRY.{c}")).unwrap_or_else(||c.to_string())) },
                    "PROVIDER" => html! {dto.provider.as_str()},
                    "SHARED" => html! { <ToggleSwitch value={dto.channel.shared} readonly={true} /> },
                    "USER_AGENT" => html! { <RevealContent preview={Some(html! { dto.user_agent.as_str() })}>{dto.user_agent.as_str()}</RevealContent> },
                    "DURATION" => html! { <span class="tp__stream-table__duration" data-ts={dto.ts.to_string()}>{format_duration(current_time_secs() - dto.ts)}</span> },
                    _ => html! {""},
                }
            })
    };

    let is_sortable = Callback::<usize, bool>::from(move |_col| {
        false
    });

    let on_sort = Callback::<Option<(usize, SortOrder)>, ()>::from(move |_args| {});

    let table_definition = {
        // first register for config update
        let render_header_cell_cb = render_header_cell.clone();
        let render_data_cell_cb = render_data_cell.clone();
        let is_sortable = is_sortable.clone();
        let on_sort = on_sort.clone();
        let num_cols = headers.len();
        use_memo((props.streams.clone(), (*headers).clone()), move |(streams, _)| {
            streams.as_ref().map(|list|
                Rc::new(TableDefinition::<StreamInfo> {
                    items: if list.is_empty() { None } else { Some(Rc::new(list.clone())) },
                    num_cols,
                    is_sortable,
                    on_sort,
                    render_header_cell: render_header_cell_cb,
                    render_data_cell: render_data_cell_cb,
                }))
        })
    };


    let handle_menu_click = {
        let popup_is_open_state = popup_is_open.clone();
        let translate = translate.clone();
        let services_ctx = services.clone();
        let selected_dto = selected_dto.clone();
        Callback::from(move |(name, _): (String, _)| {
            if let Ok(action) = StreamsTableAction::from_str(&name) {
                match action {
                    StreamsTableAction::Kick => {
                        if let Some(dto) = (*selected_dto).as_ref() {
                            if !services_ctx.websocket.send_message(ProtocolMessage::UserAction(UserCommand::Kick(dto.addr))) {
                                services_ctx.toastr.error(translate.t("MESSAGES.FAILED_TO_KICK_USER_STREAM"));
                            }
                        }
                    }
                }
            }
            popup_is_open_state.set(false);
        })
    };

    html! {
        <div class="tp__streams-table">
          {
            if let Some(definition) = table_definition.as_ref() {
                html! {
                  <>
                   <Table::<StreamInfo> definition={definition.clone()} />
                    <PopupMenu is_open={*popup_is_open} anchor_ref={(*popup_anchor_ref).clone()} on_close={handle_popup_close}>
                        <MenuItem icon="Disconnect" name={StreamsTableAction::Kick.to_string()} label={translate.t("LABEL.KICK")} onclick={&handle_menu_click} class="tp__delete_action"></MenuItem>
                    </PopupMenu>
                </>
                  }
            } else {
              html! {}
            }
          }
        </div>
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum StreamsTableAction {
    Kick,
}

impl Display for StreamsTableAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", match self {
            Self::Kick => "kick",
        })
    }
}

impl FromStr for StreamsTableAction {
    type Err = TuliproxError;

    fn from_str(s: &str) -> Result<Self, TuliproxError> {
        if s.eq("kick") {
            Ok(Self::Kick)
        } else {
            create_tuliprox_error_result!(TuliproxErrorKind::Info, "Unknown Stream Action: {}", s)
        }
    }
}