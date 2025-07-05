use std::future;
use std::rc::Rc;
use log::info;
use yew::prelude::*;
use yew::suspense::use_future;
use yew_i18n::use_translation;
use shared::model::{ConfigTargetDto};
use crate::app::components::{PlaylistContext, Table, TableDefinition, TargetOutput, ToggleSwitch};
use crate::app::components::reveal_content::RevealContent;
use crate::hooks::use_service_context;

const HEADERS: [&str; 11] = [
"TABLE.ID",
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
    let playlist_ctx = use_context::<PlaylistContext>();

    let render_header_cell = {
        Callback::<usize, Html>::from(move |col| {
            html! {
                {
                    if col < HEADERS.len() {
                       translate.t(HEADERS[col])
                    } else {
                      String::new()
                    }
               }
            }
        })
    };

    let render_data_cell = {
        Callback::<(usize, usize, Rc<ConfigTargetDto>), Html>::from(
            move |(_row, col, dto): (usize, usize, Rc<ConfigTargetDto>)| {
                match col {
                    0 =>  html! { &dto.id.to_string() },
                    1 => html! { <ToggleSwitch readonly={false} value={&dto.enabled} /> },
                    2 => html! { &dto.name.to_string() },
                    3 => html! { <RevealContent>{"Hello"}</RevealContent> },
                    4 => html! { <RevealContent>{dto.sort.as_ref().map_or_else(String::new, |s| format!("{s:?}"))}</RevealContent> },
                    5 => html! { &dto.filter.clone() },
                    6 => html! { <TargetOutput target={Rc::clone(&dto)} /> },
                    7 => html! { "rename" },
                    8 => html! { dto.mapping.as_ref().map_or_else(String::new, |m| format!("{m:?}")) },
                    9 => html! { &dto.processing_order.to_string() },
                    10 => html! { dto.watch.as_ref().map_or_else(String::new, |w| format!("{w:?}"))},
                    _ => html! {""},
                }
        })
    };

    let table_definition = {
        let render_header_cell_cb = render_header_cell.clone();
        let render_data_cell_cb = render_data_cell.clone();
        use_state(|| Rc::new(TableDefinition::<ConfigTargetDto> {
            items: Rc::new(Vec::new()),
            num_cols: 11,
            render_header_cell: render_header_cell_cb,
            render_data_cell: render_data_cell_cb,
        }))
    };

    {
        // first register for config update
        let services_ctx = services.clone();
        let table_definition_state  = table_definition.clone();
        let render_header_cell_cb = render_header_cell.clone();
        let render_data_cell_cb = render_data_cell.clone();
        let _ = use_future(|| async move {
            services_ctx.config.config_subscribe(
                &mut |cfg| {
                    let render_header_cell_cb = render_header_cell_cb.clone();
                    let render_data_cell_cb = render_data_cell_cb.clone();
                    if let Some(app_cfg) = cfg.clone() {
                        let mut targets = vec![];
                        info!("{:?}", &app_cfg.sources.sources);
                        for source in &app_cfg.sources.sources {
                            for target in &source.targets {
                                targets.push(Rc::new(target.clone()));
                            }
                        }
                        table_definition_state.set(Rc::new(TableDefinition::<ConfigTargetDto> {
                            items: Rc::new(targets),
                            num_cols: 11,
                            render_header_cell: render_header_cell_cb,
                            render_data_cell: render_data_cell_cb,
                        }));
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

    html! {
        <div class="tp__target-table">
            <Table::<ConfigTargetDto> definition={(*table_definition).clone()} />
        </div>
    }
}
