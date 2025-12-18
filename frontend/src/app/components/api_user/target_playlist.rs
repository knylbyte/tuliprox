use crate::app::components::{AppIcon, Card, CollapsePanel};
use crate::html_if;
use shared::model::{PlaylistClusterBouquetDto, PlaylistClusterCategoriesDto, XtreamCluster};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::str::FromStr;
use wasm_bindgen::JsCast;
use yew::prelude::*;
use yew_i18n::use_translation;

fn normalize(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();
    cleaned.trim().to_lowercase()
}

fn sort_opt_vec(v: &mut Option<Vec<String>>) {
    if let Some(ref mut inner) = v {
        inner.sort_by_key(|a| normalize(a));
    }
}

macro_rules! create_selection {
    ($bouquet:expr, $categories:expr, $selections:expr, $field: ident) => {
        if let Some(selects) = $bouquet.$field.as_ref() {
            for b in selects {
                $selections.$field.insert(b.clone(), true);
            }
        } else {
            if let Some(cats) = $categories.$field.as_ref() {
                for c in cats {
                    $selections.$field.insert(c.clone(), true);
                }
            }
        }
    };
}

#[derive(Clone, PartialEq, Default)]
pub struct BouquetSelection {
    pub live: HashMap<String, bool>,
    pub vod: HashMap<String, bool>,
    pub series: HashMap<String, bool>,
}

#[derive(Properties, PartialEq)]
pub struct UserTargetPlaylistProps {
    pub categories: Option<PlaylistClusterCategoriesDto>,
    pub bouquet: Option<PlaylistClusterBouquetDto>,
    pub on_change: Callback<Rc<RefCell<BouquetSelection>>>,
}

#[function_component]
pub fn UserTargetPlaylist(props: &UserTargetPlaylistProps) -> Html {
    let translate = use_translation();
    let bouquet_selection = use_mut_ref(BouquetSelection::default);
    let playlist_categories = use_state(PlaylistClusterCategoriesDto::default);
    let force_update = use_state(|| 0);

    {
        let bouquet_selection = bouquet_selection.clone();
        let playlist_categories = playlist_categories.clone();
        let in_cats = props.categories.clone();
        let in_bouquet = props.bouquet.clone();
        let force_update = force_update.clone();
        use_effect_with(
            (in_cats, in_bouquet),
            move |(maybe_categories, maybe_bouquet)| {
                let mut selections = BouquetSelection::default();
                if let Some(categories) = maybe_categories.as_ref() {
                    if let Some(bouquet) = maybe_bouquet.as_ref() {
                        create_selection!(bouquet, categories, selections, live);
                        create_selection!(bouquet, categories, selections, vod);
                        create_selection!(bouquet, categories, selections, series);
                    } else {
                        if let Some(cats) = categories.live.as_ref() {
                            for c in cats {
                                selections.live.insert(c.clone(), true);
                            }
                        }
                        if let Some(cats) = categories.vod.as_ref() {
                            for c in cats {
                                selections.vod.insert(c.clone(), true);
                            }
                        }
                        if let Some(cats) = categories.series.as_ref() {
                            for c in cats {
                                selections.series.insert(c.clone(), true);
                            }
                        }
                    }
                    *bouquet_selection.borrow_mut() = selections;
                    let mut new_categories = categories.clone();
                    sort_opt_vec(&mut new_categories.live);
                    sort_opt_vec(&mut new_categories.vod);
                    sort_opt_vec(&mut new_categories.series);
                    playlist_categories.set(new_categories);
                    force_update.set(*force_update + 1);
                }
            },
        );
    }

    let handle_category_click = {
        let on_change = props.on_change.clone();
        let bouquet_selection = bouquet_selection.clone();
        let force_update = force_update.clone();
        Callback::from(move |e: MouseEvent| {
            e.prevent_default();
            if let Some(target) = e.target() {
                if let Ok(element) = target.dyn_into::<web_sys::Element>() {
                    if let Some(cluster) = element.get_attribute("data-cluster") {
                        if let Ok(cluster) = XtreamCluster::from_str(cluster.as_str()) {
                            if let Some(category) = element.get_attribute("data-category") {
                                let mut selections = bouquet_selection.borrow_mut();
                                match cluster {
                                    XtreamCluster::Live => {
                                        let selected =
                                            *selections.live.get(&category).unwrap_or(&false);
                                        selections.live.insert(category, !selected);
                                    }
                                    XtreamCluster::Video => {
                                        let selected =
                                            *selections.vod.get(&category).unwrap_or(&false);
                                        selections.vod.insert(category, !selected);
                                    }
                                    XtreamCluster::Series => {
                                        let selected =
                                            *selections.series.get(&category).unwrap_or(&false);
                                        selections.series.insert(category, !selected);
                                    }
                                }
                                on_change.emit(bouquet_selection.clone());
                                force_update.set(*force_update + 1);
                            }
                        }
                    }
                }
            }
        })
    };

    let render_category_cluster =
        |cluster: XtreamCluster, cats: Option<&Vec<String>>, selections: &HashMap<String, bool>| {
            if let Some(c) = cats {
                html_if!(!c.is_empty(), {
                   <Card>
                      <CollapsePanel title={translate.t( match cluster {
                            XtreamCluster::Live =>  "LABEL.LIVE",
                            XtreamCluster::Video =>  "LABEL.MOVIE",
                            XtreamCluster::Series =>  "LABEL.SERIES"
                      })}>
                        <div class="tp__api-user-target-playlist__categories">
                            { for c.iter().map(|cat| {
                                let selected = *selections.get(cat).unwrap_or(&false);
                                html! {
                                <div key={cat.clone()} data-cluster={cluster.to_string()} data-category={cat.clone()} class={classes!("tp__api-user-target-playlist__categories-category", if selected {"selected"} else {""})}
                                    onclick={handle_category_click.clone()}>
                                    <AppIcon name={if selected {"Checked"} else {"Unchecked"}}/> { &cat }
                                </div>
                            }})}
                        </div>
                     </CollapsePanel>
                    </Card>
                })
            } else {
                html! {}
            }
        };

    let selections = &*bouquet_selection.borrow();
    html! {
        <div class={"tp__api-user-target-playlist"}>
            <div class="tp__api-user-target-playlist__body">
                { render_category_cluster(XtreamCluster::Live, playlist_categories.live.as_ref(), &selections.live) }
                { render_category_cluster(XtreamCluster::Video, playlist_categories.vod.as_ref(), &selections.vod) }
                { render_category_cluster(XtreamCluster::Series, playlist_categories.series.as_ref(), &selections.series) }
            </div>
        </div>
    }
}
