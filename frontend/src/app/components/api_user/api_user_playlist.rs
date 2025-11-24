use yew::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::str::FromStr;
use yew_i18n::use_translation;
use shared::error::TuliproxError;
use shared::info_err;
use shared::model::{PlaylistBouquetDto, TargetBouquetDto};
use crate::app::components::{Panel, RadioButtonGroup};
use crate::hooks::use_service_context;
use crate::html_if;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum ApiUserPlaylistPage {
    Xtream,
    M3u
}

impl FromStr for ApiUserPlaylistPage {
    type Err = TuliproxError;

    fn from_str(s: &str) -> Result<Self, TuliproxError> {
        match s.to_lowercase().as_str() {
            "xtream" => Ok(ApiUserPlaylistPage::Xtream),
            "m3u" => Ok(ApiUserPlaylistPage::M3u),
            _ => Err(info_err!(format!("Unknown api user playlist type: {s}"))),
        }
    }
}

impl fmt::Display for ApiUserPlaylistPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ApiUserPlaylistPage::Xtream => "xtream",
            ApiUserPlaylistPage::M3u => "m3u",
        };
        write!(f, "{s}")
    }
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct BouquetSelection {
    pub live: HashMap<String, bool>,
    pub vod: HashMap<String, bool>,
    pub series: HashMap<String, bool>,
}

// ----- Helper -----
fn is_empty<T: Serialize>(value: &Option<T>) -> bool {
    match value {
        None => true,
        Some(v) => {
            let json = serde_json::to_value(v).unwrap_or_default();
            json.is_null() || json.as_array().map(|a| a.is_empty()).unwrap_or(false)
        }
    }
}

// ----- Component -----

#[derive(Properties, PartialEq)]
pub struct UserPlaylistProps {

}

#[function_component]
pub fn UserPlaylist(props: &UserPlaylistProps) -> Html {
    let translate = use_translation();
    let service_ctx = use_service_context();
    let loading = use_state(|| false);
    let categories = use_state(|| None as Option<PlaylistBouquetDto>);
    let bouquets = use_state(|| None as Option<PlaylistBouquetDto>);
    let active_tab = use_state(|| ApiUserPlaylistPage::Xtream);
    let playlist_types = use_memo((), |st| {
        let st = st.as_ref().map(|v| v.as_slice()).unwrap_or(&[ApiUserPlaylistPage::Xtream, ApiUserPlaylistPage::M3u]);
        st.iter().map(ToString::to_string).collect::<Vec<String>>()
    });

    let handle_tab_select = {
        let active_tab_clone = active_tab.clone();
        Callback::from(move |page_selection: Rc<Vec<String>>| {
            if let Some(page_selection_str) = page_selection.first() {
                if let Ok(page) = ApiUserPlaylistPage::from_str(page_selection_str) {
                    active_tab_clone.set(page)
                }
            }
        })
    };

    // Selection reference
    let selections = use_mut_ref(|| {
        HashMap::<String, BouquetSelection>::new()
    });

    // ----- Load data on mount -----
    {
        let loading = loading.clone();
        let categories = categories.clone();
        let bouquets = bouquets.clone();
        let services = service_ctx.clone();

        use_effect_with((), move |_| {
            wasm_bindgen_futures::spawn_local(async move {
                loading.set(true);

                let bq = services.user_config.get_playlist_bouquet().await;
                let ct = services.user_config.get_playlist_categories().await;

                let mut bouquet = if is_empty(&bq) { None } else { bq };
                let mut cats = if is_empty(&ct) { None } else { ct };

                bouquets.set(bouquet.clone());
                categories.set(cats.clone());

                // Convert loaded user bouquet → rust maps
                if let Some(b) = bouquet {
                    if let Some(xt) = &b.xtream {
                        selections.borrow_mut().insert(
                            "xtream".into(),
                            target_to_selection(xt)
                        );
                    }
                    if let Some(m3u) = &b.m3u {
                        selections.borrow_mut().insert(
                            "m3u".into(),
                            target_to_selection(m3u)
                        );
                    }
                }

                loading.set(false);
            });
            || {}
        });
    }

    // ----- Save handler -----
    let on_save = {
        let selections = selections.clone();
        let categories = categories.clone();
        let services = service_ctx.clone();
        let loading = loading.clone();

        Callback::from(move |_| {
            let selections = selections.clone();
            let categories = categories.clone();
            let services = services.clone();
            let loading = loading.clone();

            wasm_bindgen_futures::spawn_local(async move {
                loading.set(true);

                // Convert selection maps back into service payload
                let result = PlaylistBouquetDto {
                    xtream: categories.as_ref().and_then(|cats| {
                        cats.xtream.as_ref().map(|target| {
                            selection_to_target(
                                selections.borrow().get("xtream").cloned(),
                                target,
                            )
                        })
                    }),
                    m3u: categories.as_ref().and_then(|cats| {
                        cats.m3u.as_ref().map(|target| {
                            selection_to_target(
                                selections.borrow().get("m3u").cloned(),
                                target,
                            )
                        })
                    }),
                };

                // Save bouquet
                // let r = services
                //     .user_config
                //     .save_playlist_bouquet(result)
                //     .await;

                // match r {
                //     Ok(_) =>  services.toastr.success("Playlist saved successfully".into()),
                //     Err(_) => services.toastr.error("Failed to save playlist".into()),
                // }

                loading.set(false);
            });
        })
    };

    // ----- Tab switch -----
    let on_tab_click = {
        let active_tab = active_tab.clone();
        Callback::from(move |tab: String| active_tab.set(tab))
    };

    html! {
        <div class="tp__api-user-playlist">
            { if *loading { html!{ <div class="loading">{"Loading..."}</div> } } else { html!{} }}

            <div class="tp__api-user-playlist__toolbar">
                <label>{"User Bouquet Editor"}</label>
                <button onclick={on_save}>{"Save"}</button>
            </div>

            <div class="tp__api-user-playlist__content">
                <div class="user-playlist__content-toolbar">
                 <RadioButtonGroup options={playlist_types.clone()}
                                          selected={Rc::new(vec![(*active_tab).to_string()])}
                                          on_select={handle_tab_select} />
                </div>

                <div class="tp__api-user-playlist__content-panels">
                    <Panel value={ApiUserPlaylistPage::Xtream.to_string()} active={active_tab.to_string()}>
                       {"xtream"}
                    // <UserTargetPlaylist
                    //     visible={*active_tab == "xtream"}
                    //     categories={categories.as_ref().and_then(|c| c.xtream.clone())}
                    //     bouquet={bouquets.as_ref().and_then(|c| c.xtream.clone())}
                    //     on_change={Callback::from(move |sel: BouquetSelection| {
                    //         selections.borrow_mut().insert("xtream".into(), sel);
                    //     })}
                    // />
                    </Panel>
                    <Panel value={ApiUserPlaylistPage::M3u.to_string()} active={active_tab.to_string()}>
                       {"xtream"}
                    // <UserTargetPlaylist
                    //     visible={*active_tab == "m3u"}
                    //     categories={categories.as_ref().and_then(|c| c.m3u.clone())}
                    //     bouquet={bouquets.as_ref().and_then(|c| c.m3u.clone())}
                    //     on_change={Callback::from(move |sel: BouquetSelection| {
                    //         selections.borrow_mut().insert("m3u".into(), sel);
                    //     })}
                    // />
                    </Panel>
                </div>
            </div>
        </div>
    }
}

/// Convert user target categories → selection maps
fn target_to_selection(t: &TargetBouquetDto) -> BouquetSelection {
    fn map(list: &Option<Vec<String>>) -> HashMap<String, bool> {
        match list {
            Some(vec) => vec.iter().map(|v| (v.clone(), true)).collect(),
            None => HashMap::new(),
        }
    }

    BouquetSelection {
        live: map(&t.live),
        vod: map(&t.vod),
        series: map(&t.series),
    }
}

/// Convert selection back into a service payload
fn selection_to_target(
    sel: Option<BouquetSelection>,
    target: &TargetBouquetDto,
) -> TargetBouquetDto {
    let s = sel.unwrap_or(BouquetSelection::default());

    fn to_vec(map: &HashMap<String, bool>) -> Option<Vec<String>> {
        let v: Vec<String> = map.iter().filter(|(_, v)| **v).map(|(k, _)| k.clone()).collect();
        if v.is_empty() { None } else { Some(v) }
    }

    TargetBouquetDto {
        live: to_vec(&s.live),
        vod: to_vec(&s.vod),
        series: to_vec(&s.series),
    }
}
