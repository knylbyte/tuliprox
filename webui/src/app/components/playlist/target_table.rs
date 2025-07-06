use crate::app::components::popup_menu::PopupMenu;
use crate::app::components::reveal_content::RevealContent;
use crate::app::components::{AppIcon, Table, TableDefinition, TargetOutput, ToggleSwitch};
use crate::hooks::use_service_context;
use shared::model::ConfigTargetDto;
use std::future;
use std::rc::Rc;
use log::info;
use yew::prelude::*;
use yew::suspense::use_future;
use yew_i18n::use_translation;
use crate::app::components::menu_item::MenuItem;

const HEADERS: [&str; 11] = [
    "TABLE.EMPTY",
    "TABLE.ENABLED",
    "TABLE.NAME",
    "TABLE.OPTIONS",
    "TABLE.SORT",
    "TABLE.FILTER",
    "TABLE.OUTPUT",
    "TABLE.RENAME",
    "TABLE.MAPPING",
    "TABLE.PROCESSING_ORDER",
    "TABLE.WATCH",
];

#[function_component]
pub fn TargetTable() -> Html {
    let translate = use_translation();
    let services = use_service_context();
    let popup_anchor_ref = use_state(|| None::<web_sys::Element>);
    let popup_is_open = use_state(|| false);
    let selected_dto = use_state(|| None::<Rc<ConfigTargetDto>>);
    let table_definition = use_state(|| None::<Rc<TableDefinition<ConfigTargetDto>>>);

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
        Callback::from(move |(dto, event): (Rc<ConfigTargetDto>, MouseEvent)| {
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
        let popup_onclick = handle_popup_onclick.clone();
        Callback::<(usize, usize, Rc<ConfigTargetDto>), Html>::from(
            move |(row, col, dto): (usize, usize, Rc<ConfigTargetDto>)| {
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
                    1 => html! { <ToggleSwitch readonly={true} value={&dto.enabled} /> },
                    2 => html! { &dto.name.to_string() },
                    3 => html! { <RevealContent>{"Hello"}</RevealContent> },
                    4 => html! { <RevealContent>{dto.sort.as_ref().map_or_else(String::new, |s| format!("{s:?}"))}</RevealContent> },
                    5 => html! { &dto.filter.clone() },
                    6 => html! { <TargetOutput target={Rc::clone(&dto)} /> },
                    7 => html! { <RevealContent>{"Hello"}</RevealContent> },
                    8 => html! { dto.mapping.as_ref().map_or_else(String::new, |m| format!("{m:?}")) },
                    9 => html! { &dto.processing_order.to_string() },
                    10 => html! { dto.watch.as_ref().map_or_else(String::new, |w| format!("{w:?}"))},
                    _ => html! {""},
                }
            })
    };

    {
        // first register for config update
        let services_ctx = services.clone();
        let table_definition_state = table_definition.clone();
        let render_header_cell_cb = render_header_cell.clone();
        let render_data_cell_cb = render_data_cell.clone();
        let _ = use_future(|| async move {
            services_ctx.config.config_subscribe(
                &mut |cfg| {
                    let render_header_cell_cb = render_header_cell_cb.clone();
                    let render_data_cell_cb = render_data_cell_cb.clone();
                    if let Some(app_cfg) = cfg.clone() {
                        let mut targets = vec![];
                        for source in &app_cfg.sources.sources {
                            for target in &source.targets {
                                targets.push(Rc::new(target.clone()));
                            }
                        }
                        table_definition_state.set(Some(Rc::new(TableDefinition::<ConfigTargetDto> {
                            items: Rc::new(targets),
                            num_cols: 11,
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


    let handle_menu_edit = {
        let popup_is_open_state = popup_is_open.clone();
        Callback::from(move |name:String| {
            info!("Menu selected {name}");
            popup_is_open_state.set(false);
        })
    };

    html! {
        <div class="tp__target-table">
          {
              if table_definition.is_some() {
                html! {
                    <>
                       <Table::<ConfigTargetDto> definition={(*table_definition).as_ref().unwrap().clone()} />
                        <PopupMenu is_open={*popup_is_open} anchor_ref={(*popup_anchor_ref).clone()} on_close={handle_popup_close}>
                            <MenuItem icon="Edit" name="edit" label={translate.t("LABEL.EDIT")} onclick={&handle_menu_edit}></MenuItem>
                            <MenuItem style="tp__delete_action" icon="Delete" name="delete" label={translate.t("LABEL.DELETE")} onclick={&handle_menu_edit}></MenuItem>
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
