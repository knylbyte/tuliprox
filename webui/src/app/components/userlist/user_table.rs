use crate::app::components::menu_item::MenuItem;
use crate::app::components::popup_menu::PopupMenu;
use crate::app::components::{convert_bool_to_chip_style, AppIcon, Chip, HideContent, ProxyTypeView, Table, TableDefinition};
use crate::hooks::use_service_context;
use crate::model::DialogResult;
use shared::error::{create_tuliprox_error_result, TuliproxError, TuliproxErrorKind};
use std::fmt::Display;
use std::rc::Rc;
use std::str::FromStr;
use yew::platform::spawn_local;
use yew::prelude::*;
use yew_i18n::use_translation;
use crate::app::context::TargetUser;
use crate::services::DialogService;

const HEADERS: [&str; 15] = [
    "TABLE.EMPTY",
    "TABLE.ENABLED",
    "TABLE.PLAYLIST",
    "TABLE.USERNAME",
    "TABLE.PASSWORD",
    "TABLE.TOKEN",
    "TABLE.PROXY",
    "TABLE.SERVER",
    "TABLE.EPG_TIMESHIFT",
    "TABLE.CREATED_AT",
    "TABLE.EXP_DATE",
    "TABLE.MAX_CONNECTIONS",
    "TABLE.STATUS",
    "TABLE.UI_ENABLED",
    "TABLE.COMMENT",
];

#[derive(Properties, PartialEq, Clone)]
pub struct UserTableProps {
    pub targets: Option<Rc<Vec<Rc<TargetUser>>>>,
}

#[function_component]
pub fn UserTable(props: &UserTableProps) -> Html {
    let translate = use_translation();
    let services = use_service_context();
    let dialog = use_context::<DialogService>().expect("Dialog service not found");
    let popup_anchor_ref = use_state(|| None::<web_sys::Element>);
    let popup_is_open = use_state(|| false);
    let selected_dto = use_state(|| None::<Rc<TargetUser>>);

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
        Callback::from(move |(dto, event): (Rc<TargetUser>, MouseEvent)| {
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
        Callback::<(usize, usize, Rc<TargetUser>), Html>::from(
            move |(row, col, dto): (usize, usize, Rc<TargetUser>)| {
                let user_active = dto.credentials.is_active();
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
                    1 => html! { <Chip class={ convert_bool_to_chip_style(user_active ) }
                                  label={if user_active {translator.t("LABEL.ACTIVE")} else { translator.t("LABEL.DISABLED")} }
                                   /> },
                    2 => html! { dto.target.as_str() },
                    3 => html! { dto.credentials.username.as_str() },
                    4 => html! { <HideContent content={&dto.credentials.password.to_string()}></HideContent> },
                    5 => html! { dto.credentials.token.as_ref().map_or_else(|| html!{}, |token| html! { <HideContent content={token.to_string()}></HideContent>}) },
                    6 => html! {<ProxyTypeView value={dto.credentials.proxy} /> },
                    7 => dto.credentials.server.as_ref().map_or_else(|| html! {}, |s| html! { s } ),
                    // 6 => dto.t_filter.as_ref().map_or_else(|| html! {}, |f| html! { <RevealContent preview={Some(html!{<FilterView inline={true} filter={f.clone()} />})}><FilterView pretty={true} filter={f.clone()} /></RevealContent> }),
                    // 7 => dto.rename.as_ref().map_or_else(|| html! {}, |_r| html! { <RevealContent><UserRename target={Rc::clone(&dto)} /></RevealContent> }),
                    // 8 => html! { <PlaylistMappings mappings={dto.mapping.clone()} /> },
                    // 9 => html! { <PlaylistProcessing order={dto.processing_order} /> },
                    // 10 => html! { <UserWatch  target={Rc::clone(&dto)} /> },
                    _ => html! {""},
                }
            })
    };

    let table_definition = {
        // first register for config update
        let render_header_cell_cb = render_header_cell.clone();
        let render_data_cell_cb = render_data_cell.clone();
        let num_cols = HEADERS.len();
        use_memo(props.targets.clone(), move |targets| {
            targets.as_ref().map(|list|
                Rc::new(TableDefinition::<TargetUser> {
                    items: list.clone(),
                    num_cols,
                    render_header_cell: render_header_cell_cb,
                    render_data_cell: render_data_cell_cb,
                }))
        })
    };


    let handle_menu_click = {
        let popup_is_open_state = popup_is_open.clone();
        let confirm = dialog.clone();
        let translate = translate.clone();
        let selected_dto = selected_dto.clone();
        Callback::from(move |name: String| {
            if let Ok(action) = TableAction::from_str(&name) {
                match action {
                    TableAction::Edit => {}
                    TableAction::Refresh => {
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
            if let Some(definition) = table_definition.as_ref() {
                html! {
                  <>
                   <Table::<TargetUser> definition={definition.clone()} />
                    <PopupMenu is_open={*popup_is_open} anchor_ref={(*popup_anchor_ref).clone()} on_close={handle_popup_close}>
                        <MenuItem icon="Edit" name={TableAction::Edit.to_string()} label={translate.t("LABEL.EDIT")} onclick={&handle_menu_click}></MenuItem>
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