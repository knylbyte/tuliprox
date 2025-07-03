use std::rc::Rc;
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct TableDefinition<T: PartialEq + Clone + 'static> {
    pub items: Vec<T>,
    pub num_cols: usize,
    pub render_header_cell: Callback<usize, Html>,
    pub render_data_cell: Callback<(usize, usize, T), Html>,
}

#[derive(Properties, Clone, PartialEq)]
pub struct TableProps<T: PartialEq + Clone + 'static> {
    pub definition: Rc<TableDefinition<T>>,
}

#[function_component]
pub fn Table<T: PartialEq + Clone + 'static>(props: &TableProps<T>) -> Html {
    let TableDefinition {
        items,
        num_cols,
        render_header_cell,
        render_data_cell,
    } = &*props.definition;

    html! {
        <div class={"tp__table"}>
        <div class={"tp__table__container"}>
        <table class="tp__table__table">
            <thead>
                <tr>
                    {
                        for (0..*num_cols).map(|col_index| {
                            html!{
                               <th> {render_header_cell.emit(col_index)}</th>
                            }
                        })
                    }
                </tr>
            </thead>
            <tbody>
                {
                    for items.iter().enumerate().map(|(row_index, item)| {
                        let item_clone = item.clone();
                        html! {
                            <tr>
                                {
                                    for (0..*num_cols).map(|col_index| {
                                        html!{
                                           <td>{render_data_cell.emit((row_index, col_index, item_clone.clone()))}</td>
                                        }
                                    })
                                }
                            </tr>
                        }
                    })
                }
            </tbody>
        </table>
        </div>
        </div>
    }
}
