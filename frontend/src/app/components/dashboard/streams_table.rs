use crate::app::components::menu_item::MenuItem;
use crate::app::components::popup_menu::PopupMenu;
use crate::app::components::{AppIcon, Table, TableDefinition};
use crate::hooks::use_service_context;
use shared::error::{create_tuliprox_error_result, TuliproxError, TuliproxErrorKind};
use shared::model::{SortOrder, StreamInfo};
use std::fmt::Display;
use std::rc::Rc;
use std::str::FromStr;
use yew::prelude::*;
use yew_i18n::use_translation;

const HEADERS: [&str; 7] = [
    "LABEL.EMPTY",
    "LABEL.USERNAME",
    "LABEL.STREAM_ID",
    "LABEL.CHANNEL",
    "LABEL.GROUP",
    "LABEL.CLIENT_IP",
    "LABEL.PROVIDER"
];

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
                    3 => html! {dto.channel.title.as_str()},
                    4 => html! {dto.channel.group.as_str()},
                    5 => html! {dto.addr.as_str()},
                    6 => html! {dto.provider.as_str()},
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