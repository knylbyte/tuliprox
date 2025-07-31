use std::rc::Rc;
use crate::app::context::PlaylistExplorerContext;
use yew::prelude::*;
use yew_i18n::use_translation;
use shared::model::{UiPlaylistGroup, XtreamCluster};

enum ExplorerLevel {
    Categories,
    Group(Rc<UiPlaylistGroup>),
}

const HEADERS: [&str; 11] = [
    "TABLE.EMPTY",
    "TABLE.ENABLED",
    "TABLE.NAME",
    "TABLE.OUTPUT",
    "TABLE.OPTIONS",
    "TABLE.SORT",
    "TABLE.FILTER",
    "TABLE.RENAME",
    "TABLE.MAPPING",
    "TABLE.PROCESSING_ORDER",
    "TABLE.WATCH",
];

#[function_component]
pub fn PlaylistExplorer() -> Html {
    let translate = use_translation();
    let context = use_context::<PlaylistExplorerContext>().expect("PlaylistExlorer context not found");
    let current_item = use_state(|| ExplorerLevel::Categories);
    //
    // let popup_anchor_ref = use_state(|| None::<web_sys::Element>);
    // let popup_is_open = use_state(|| false);
    //
    // let handle_popup_close = {
    //     let set_is_open = popup_is_open.clone();
    //     Callback::from(move |()| {
    //         set_is_open.set(false);
    //     })
    // };
    //
    // let handle_popup_onclick = {
    //     let set_selected_dto = selected_dto.clone();
    //     let set_anchor_ref = popup_anchor_ref.clone();
    //     let set_is_open = popup_is_open.clone();
    //     Callback::from(move |(dto, event): (Rc<ConfigTargetDto>, MouseEvent)| {
    //         if let Some(target) = event.target_dyn_into::<web_sys::Element>() {
    //             set_selected_dto.set(Some(dto.clone()));
    //             set_anchor_ref.set(Some(target));
    //             set_is_open.set(true);
    //         }
    //     })
    // };
    //
    // let render_header_cell = {
    //     let translator = translate.clone();
    //     Callback::<usize, Html>::from(move |col| {
    //         html! {
    //             {
    //                 if col < HEADERS.len() {
    //                    translator.t(HEADERS[col])
    //                 } else {
    //                   String::new()
    //                 }
    //            }
    //         }
    //     })
    // };
    //
    // let render_data_cell = {
    //     let translator = translate.clone();
    //     let popup_onclick = handle_popup_onclick.clone();
    //     Callback::<(usize, usize, Rc<ConfigTargetDto>), Html>::from(
    //         move |(row, col, dto): (usize, usize, Rc<ConfigTargetDto>)| {
    //             match col {
    //                 0 => {
    //                     let popup_onclick = popup_onclick.clone();
    //                     html! {
    //                         <button class="tp__icon-button"
    //                             onclick={Callback::from(move |event: MouseEvent| popup_onclick.emit((dto.clone(), event)))}
    //                             data-row={row.to_string()}>
    //                             <AppIcon name="Popup"></AppIcon>
    //                         </button>
    //                     }
    //                 }
    //                 1 => html! { <Chip class={ convert_bool_to_chip_style(dto.enabled) }
    //                              label={if dto.enabled {translator.t("LABEL.ACTIVE")} else { translator.t("LABEL.DISABLED")} }
    //                               /> },
    //                 2 => html! { dto.name.as_str() },
    //                 3 => html! { <TargetOutput target={Rc::clone(&dto)} /> },
    //                 4 => html! { <TargetOptions target={Rc::clone(&dto)} /> },
    //                 5 => dto.sort.as_ref().map_or_else(|| html! {}, |_s| html! { <RevealContent><TargetSort target={Rc::clone(&dto)} /></RevealContent> }),
    //                 6 => dto.t_filter.as_ref().map_or_else(|| html! {}, |f| html! { <RevealContent preview={Some(html!{<FilterView inline={true} filter={f.clone()} />})}><FilterView pretty={true} filter={f.clone()} /></RevealContent> }),
    //                 7 => dto.rename.as_ref().map_or_else(|| html! {}, |_r| html! { <RevealContent><TargetRename target={Rc::clone(&dto)} /></RevealContent> }),
    //                 8 => html! { <PlaylistMappings mappings={dto.mapping.clone()} /> },
    //                 9 => html! { <PlaylistProcessing order={dto.processing_order} /> },
    //                 10 => html! { <TargetWatch  target={Rc::clone(&dto)} /> },
    //                 _ => html! {""},
    //             }
    //         })
    // };
    //
    // let table_definition = {
    //     // first register for config update
    //     let render_header_cell_cb = render_header_cell.clone();
    //     let render_data_cell_cb = render_data_cell.clone();
    //     let num_cols = crate::app::components::playlist::target_table::HEADERS.len();
    //     use_memo(props.targets.clone(), move |targets| {
    //         targets.as_ref().map(|list|
    //             Rc::new(TableDefinition::<ConfigTargetDto> {
    //                 items: Rc::new(list.clone()),
    //                 num_cols,
    //                 render_header_cell: render_header_cell_cb,
    //                 render_data_cell: render_data_cell_cb,
    //             }))
    //     })
    // };

    let handle_category_select = {
        let set_current_item = current_item.clone();
        Callback::from(move |(group, event): (Rc<UiPlaylistGroup>, MouseEvent)| {
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
                <span class="tp__playlist-explorer__category" onclick={on_click}>
                {
                 match cluster {
                    XtreamCluster::Live => html! {<span class="tp__playlist-explorer__category-live"></span>},
                    XtreamCluster::Video => html! {<span class="tp__playlist-explorer__category-video"></span>},
                    XtreamCluster::Series => html! {<span class="tp__playlist-explorer__category-series"></span>},
                    }
                }
                { group.title.clone() }</span>
            }})
            .collect::<Html>()
    };

    let render_categories = || {
        html! {
        <div class="tp__playlist-explorer__categories">
            <div class="tp__playlist-explorer__categories-list">
                { context.playlist.as_ref()
                    .and_then(|response| response.live.as_ref())
                    .map(|list| render_cluster(XtreamCluster::Live, list))
                    .unwrap_or_default()
                }
                { context.playlist.as_ref()
                    .and_then(|response| response.vod.as_ref())
                    .map(|list| render_cluster(XtreamCluster::Video, list))
                    .unwrap_or_default()
                }
                { context.playlist.as_ref()
                    .and_then(|response| response.series.as_ref())
                    .map(|list| render_cluster(XtreamCluster::Series, list))
                    .unwrap_or_default()
                }
            </div>
        </div>
    }
    };

    let render_group = |group: &Rc<UiPlaylistGroup>| {
        html! {
                <div class="tp__playlist-explorer__group">
                  {
                      group.channels.iter().map(|c| {
                            html! { c.title.clone() }
                       }).collect::<Html>()
                  }
                </div>
            }
    };

    html! {
      <div class="tp__playlist-explorer">
        <div class="tp__playlist-explorer__body">
          {
            match *current_item {
                ExplorerLevel::Categories => html!{render_categories()} ,
                ExplorerLevel::Group(ref group) => html!{ render_group(group) },
            }
          }
        </div>
      </div>
    }
}