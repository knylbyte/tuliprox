use std::rc::Rc;
use yew::prelude::*;
use yew_i18n::use_translation;
use shared::model::ConfigTargetDto;
use crate::app::components::{PlaylistContext, Table, TableDefinition, ToggleSwitch};

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
        Callback::<(usize, usize, &ConfigTargetDto), Html>::from(
            move |(_row, col, dto): (usize, usize, &ConfigTargetDto)| {
                match col {
                    0 =>  html! { &dto.name.to_string() },
                    1 => html! { <ToggleSwitch value={&dto.enabled} /> },
                    2 => html! { &dto.name.to_string() },
                    3 => html! { "options" },
                    4 => html! { "sort" },
                    5 => html! { "filter" },
                    6 => html! { "output" },
                    7 => html! { "rename" },
                    8 => html! { "mapping" },
                    9 => html! { "processing order" },
                    10 => html! { "watch" },
                    _ => html! {""},
                }
        })
    };

    let items: Vec<&ConfigTargetDto> = vec![];

    let table_definition = {
        let render_header_cell = render_header_cell.clone();
        let render_data_cell = render_data_cell.clone();

        use_state(|| Rc::new(TableDefinition::<&ConfigTargetDto> {
            items,
            num_cols: 11,
            render_header_cell,
            render_data_cell,
        }))
    };

    html! {
        <div class="tp__target-table">
            <Table::<&ConfigTargetDto> definition={(*table_definition).clone()} />
        </div>
    }
}
