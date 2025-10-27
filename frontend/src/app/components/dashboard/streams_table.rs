use std::borrow::Cow;
use crate::app::components::menu_item::MenuItem;
use crate::app::components::popup_menu::PopupMenu;
use crate::app::components::{AppIcon, Table, TableDefinition, ToggleSwitch};
use crate::hooks::use_service_context;
use shared::error::{create_tuliprox_error_result, TuliproxError, TuliproxErrorKind};
use shared::model::{SortOrder, StreamInfo};
use std::fmt::Display;
use std::rc::Rc;
use std::str::FromStr;
use gloo_timers::callback::Interval;
use gloo_utils::window;
use log::debug;
use wasm_bindgen::JsCast;
use web_sys::Element;
use yew::prelude::*;
use yew_i18n::use_translation;
use shared::utils::current_time_secs;

const HEADERS: [&str; 11] = [
    "LABEL.EMPTY",
    "LABEL.USERNAME",
    "LABEL.STREAM_ID",
    "LABEL.CLUSTER",
    "LABEL.CHANNEL",
    "LABEL.GROUP",
    "LABEL.CLIENT_IP",
    "LABEL.PROVIDER",
    "LABEL.SHARED",
    "LABEL.USER_AGENT",
    "LABEL.DURATION"
];

pub fn strip_port<'a>(input: &'a str) -> Cow<'a, str> {
    // IPv6 with port: [2001:db8::1]:8080
    if let Some(stripped) = input.strip_prefix('[') {
        if let Some(end) = stripped.find(']') {
            return Cow::Owned(stripped[..end].to_string());
        }
        // Invalid IPv6
        return Cow::Borrowed(input);
    }

    // IPv4 or IPv6 without bracket
    if let Some((left, right)) = input.rsplit_once(':') {
        // If `left` has a colon then its IPv6 without port.
        if left.contains(':') {
            Cow::Borrowed(input)
        } else {
            // IPv4:Port
            Cow::Owned(left.to_string())
        }
    } else {
        Cow::Borrowed(input)
    }
}

pub fn format_duration(seconds: u64) -> String {
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
    let popup_anchor_ref = use_state(|| None::<web_sys::Element>);
    let popup_is_open = use_state(|| false);
    let selected_dto = use_state(|| None::<Rc<StreamInfo>>);


    use_effect_with((), move |_| {
        Interval::new(1000, || {
            update_timestamps();
        }).forget();
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
        Callback::<usize, Html>::from(move |col| {
            html! {
                {
                    if col < HEADERS.len() {
                       translator.t(HEADERS[col])
                    } else {
                      String::new()
                    }
               }
            }
        })
    };

    let render_data_cell = {
        let popup_onclick = handle_popup_onclick.clone();
        Callback::<(usize, usize, Rc<StreamInfo>), Html>::from(
            move |(row, col, dto): (usize, usize, Rc<StreamInfo>)| {
                match col {
                    0 => {
                        let popup_onclick = popup_onclick.clone();
                        html! {
                            <button class="tp__icon-button"
                                onclick={Callback::from(move |event: MouseEvent| popup_onclick.emit((dto.clone(), event)))}
                                data-row={row.to_string()}>
                                <AppIcon name="Popup"></AppIcon>
                            </button>
                        }
                    }
                    1 => html! {dto.username.as_str()},
                    2 => html! { <>
                            { dto.channel.virtual_id.to_string() }
                            {" ("}
                            { dto.channel.provider_id.to_string() }
                            {")"}
                        </>},
                    3 => html! {dto.channel.cluster},
                    4 => html! {dto.channel.title.as_str()},
                    5 => html! {dto.channel.group.as_str()},
                    6 => html! { strip_port(&dto.addr)},
                    7 => html! {dto.provider.as_str()},
                    8 => html! { <ToggleSwitch value={dto.channel.shared} readonly={true} /> },
                    9 => html! { dto.user_agent.as_str() },
                    10 => html! { <span class="tp__stream-table__duration" data-ts={dto.ts.to_string()}>{format_duration(dto.ts)}</span> },
                    _ => html! {""},
                }
            })
    };

    let is_sortable = Callback::<usize, bool>::from(move |_col| {
        false
    });

    let on_sort = Callback::<Option<(usize, SortOrder)>, ()>::from(move |_args| {
    });

    let table_definition = {
        // first register for config update
        let render_header_cell_cb = render_header_cell.clone();
        let render_data_cell_cb = render_data_cell.clone();
        let is_sortable = is_sortable.clone();
        let on_sort = on_sort.clone();
        let num_cols = HEADERS.len();
        use_memo(props.streams.clone(), move |streams| {
            streams.as_ref().map(|list|
                Rc::new(TableDefinition::<StreamInfo> {
                    items: if list.is_empty() {None} else {Some(Rc::new(list.clone()))},
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
        //let translate = translate.clone();
        let services_ctx = services.clone();
        //let selected_dto = selected_dto.clone();
        Callback::from(move |(name, _): (String, _)| {
            if let Ok(action) = StreamsTableAction::from_str(&name) {
                match action {
                    StreamsTableAction::Kick => {
                        // TODO implement connection kick
                        services_ctx.toastr.error("Not implemented")
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