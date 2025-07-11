use std::fmt::Display;
use crate::app::components::popup_menu::PopupMenu;
use crate::app::components::{convert_bool_to_chip_style, AppIcon, Chip, HideContent, Table, TableDefinition};
use crate::hooks::use_service_context;
use std::future;
use std::rc::Rc;
use std::str::FromStr;
use log::info;
use yew::platform::spawn_local;
use yew::prelude::*;
use yew::suspense::use_future;
use yew_i18n::use_translation;
use shared::error::{create_tuliprox_error_result, TuliproxError, TuliproxErrorKind};
use shared::model::ConfigInputDto;
use crate::app::components::menu_item::MenuItem;
use crate::model::DialogResult;
use crate::services::{DialogService};

const HEADERS: [&str; 15] = [
"TABLE.EMPTY",
"TABLE.ENABLED",
"TABLE.NAME",
"TABLE.INPUT_TYPE",
"TABLE.URL",
"TABLE.USERNAME",
"TABLE.PASSWORD",
"TABLE.PERSIST",
"TABLE.OPTIONS",
"TABLE.ALIASES",
"TABLE.PRIORITY",
"TABLE.MAX_CONNECTIONS",
"TABLE.METHOD",
"TABLE.EPG",
"TABLE.HEADERS",
];

#[function_component]
pub fn InputTable() -> Html {
    let translate = use_translation();
    let services = use_service_context();
    let dialog = use_context::<DialogService>().expect("Dialog service not found");
    let popup_anchor_ref = use_state(|| None::<web_sys::Element>);
    let popup_is_open = use_state(|| false);
    let selected_dto = use_state(|| None::<Rc<ConfigInputDto>>);
    let table_definition = use_state(|| None::<Rc<TableDefinition<ConfigInputDto>>>);

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
        Callback::from(move |(dto, event): (Rc<ConfigInputDto>, MouseEvent)| {
            if let Some(target) = event.target_dyn_into::<web_sys::Element>() {
                set_selected_dto.set(Some(dto.clone()));
                set_anchor_ref.set(Some(target));
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
        let translator = translate.clone();
        let popup_onclick = handle_popup_onclick.clone();
        Callback::<(usize, usize, Rc<ConfigInputDto>), Html>::from(
            move |(row, col, dto): (usize, usize, Rc<ConfigInputDto>)| {
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
                    1 => html! { <Chip class={ convert_bool_to_chip_style(dto.enabled) }
                                 label={if dto.enabled {translator.t("LABEL.ACTIVE")} else { translator.t("LABEL.DISABLED")} }
                                  /> },
                    2 => html! { dto.name.as_str() },
                    3 => html! { dto.input_type.to_string() },
                    4 => html! { dto.url.as_str() },
                    5 => html! { dto.username.as_ref().map_or_else(String::new, ToString::to_string) },
                    6 => dto.password.as_ref().map_or_else(|| html!{}, |pwd| html! { <HideContent content={pwd.to_string()}></HideContent>}),
                    7 => html! { dto.persist.as_ref().map_or_else(String::new, ToString::to_string) },
                    8 => html! { "" },
                    9 => html! { "" },
                    10 => html! { "" },
                    11 => html! { "" },
                    12 => html! { "" },
                    13 => html! { "" },
                    14 => html! { "" },
                    _ => html! {""},
                }
            })
    };

    // pub xtream_skip_live: bool,
    // pub xtream_skip_vod: bool,
    // pub xtream_skip_series: bool,
    // pub xtream_live_stream_use_prefix: bool,
    // pub xtream_live_stream_without_extension: bool,

    // pub persist: Option<String>,
    // pub options: Option<ConfigInputOptionsDto>,
    // pub aliases: Option<Vec<ConfigInputAliasDto>>,
    // pub priority: i16,
    // pub max_connections: u16,
    // pub method: InputFetchMethod,
    // pub epg: Option<EpgConfigDto>,
    // pub headers: HashMap<String, String>,
    {
        // first register for config update
        let services_ctx = services.clone();
        let table_definition_state = table_definition.clone();
        let render_header_cell_cb = render_header_cell.clone();
        let render_data_cell_cb = render_data_cell.clone();
        let num_cols = HEADERS.len();
        let _ = use_future(|| async move {
            services_ctx.config.config_subscribe(
                &mut |cfg| {
                    let render_header_cell_cb = render_header_cell_cb.clone();
                    let render_data_cell_cb = render_data_cell_cb.clone();
                    if let Some(app_cfg) = cfg.clone() {
                        let mut inputs = vec![];
                        for source in &app_cfg.sources.sources {
                            for input in &source.inputs {
                                inputs.push(Rc::new(input.clone()));
                            }
                        }
                        table_definition_state.set(Some(Rc::new(TableDefinition::<ConfigInputDto> {
                            items: Rc::new(inputs),
                            num_cols,
                            render_header_cell: render_header_cell_cb,
                            render_data_cell: render_data_cell_cb,
                        })));
                    };
                    future::ready(())
                }
            ).await
        });
    }

    {
        let services_ctx = services.clone();
        let _ = use_future(|| async move {
            let _cfg = services_ctx.config.get_server_config().await;
        });
    }

    let handle_menu_click = {
        let popup_is_open_state = popup_is_open.clone();
        let confirm = dialog.clone();
        let translate = translate.clone();
        let services_ctx = services.clone();
        let selected_dto = selected_dto.clone();
        Callback::from(move |name:String| {
            if let Ok(action) = TableAction::from_str(&name) {
                match action {
                    TableAction::Edit => {}
                    TableAction::Refresh => {
                        let services_ctx = services_ctx.clone();
                        let dto_name = selected_dto.as_ref().map_or_else(String::new, |d| d.name.to_string());
                        spawn_local(async move {
                            let targets = vec![dto_name.as_str()];
                            match services_ctx.playlist.update_targets(&targets).await {
                                true => { info!("Ok"); }
                                false => { info!("not ok");  }
                            }
                        });
                    }
                    TableAction::Delete => {
                        let confirm = confirm.clone();
                        let translator = translate.clone();
                        spawn_local(async move {
                            let result = confirm.confirm(&translator.t("MESSAGES.CONFIRM_DELETE")).await;
                            if result == DialogResult::Ok {
                                // TODO edit
                            }
                        });
                    }
                }
            }
            popup_is_open_state.set(false);
        })
    };

    html! {
        <div class="tp__target-table">
          {
              if table_definition.is_some() {
                html! {
                    <>
                       <Table::<ConfigInputDto> definition={(*table_definition).as_ref().unwrap().clone()} />
                        <PopupMenu is_open={*popup_is_open} anchor_ref={(*popup_anchor_ref).clone()} on_close={handle_popup_close}>
                            <MenuItem icon="Edit" name={TableAction::Edit.to_string()} label={translate.t("LABEL.EDIT")} onclick={&handle_menu_click}></MenuItem>
                            <MenuItem icon="Refresh" name={TableAction::Refresh.to_string()} label={translate.t("LABEL.REFRESH")} onclick={&handle_menu_click} style="tp__update_action"></MenuItem>
                            <hr/>
                            <MenuItem icon="Delete" name={TableAction::Delete.to_string()} label={translate.t("LABEL.DELETE")} onclick={&handle_menu_click} style="tp__delete_action"></MenuItem>
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
enum TableAction {
    Edit,
    Refresh,
    Delete,
}

impl Display for TableAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", match self {
            Self::Edit => "edit",
            Self::Refresh => "refresh",
            Self::Delete => "delete",
        })
    }
}

impl FromStr for TableAction {
    type Err = TuliproxError;

    fn from_str(s: &str) -> Result<Self, TuliproxError> {
        if s.eq("edit") {
            Ok(Self::Edit)
        } else if s.eq("refresh") {
                Ok(Self::Refresh)
        } else if s.eq("delete") {
            Ok(Self::Delete)
        } else {
            create_tuliprox_error_result!(TuliproxErrorKind::Info, "Unknown InputType: {}", s)
        }
    }
}