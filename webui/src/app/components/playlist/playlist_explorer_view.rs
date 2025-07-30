use crate::app::components::{Breadcrumbs, Panel, PlaylistExplorerPage, PlaylistSourceSelector};
use crate::app::context::PlaylistExplorerContext;
use std::rc::Rc;
use yew::prelude::*;
use yew_i18n::use_translation;
use shared::model::{PlaylistCategoriesResponse, UiPlaylistCategories};
use crate::app::components::playlist::playlist_explorer::PlaylistExplorer;

#[function_component]
pub fn PlaylistExplorerView() -> Html {
    let translate = use_translation();
    let breadcrumbs = use_state(|| Rc::new(vec![translate.t("LABEL.PLAYLISTS"), translate.t("LABEL.LIST")]));
    let active_page = use_state(|| PlaylistExplorerPage::SourceSelector);
    let playlist = use_state(|| None::<Rc<UiPlaylistCategories>>);

    let handle_breadcrumb_select = {
        let view_visible = active_page.clone();
        Callback::from(move |(_name, index)| {
            if index == 0 && *view_visible != PlaylistExplorerPage::SourceSelector {
                view_visible.set(PlaylistExplorerPage::SourceSelector);
            }
        })
    };

    {
        let breadcrumbs = breadcrumbs.clone();
        let view_visible_dep = active_page.clone();
        let view_visible = active_page.clone();
        let translate = translate.clone();
        use_effect_with(view_visible_dep, move |_| {
            match *view_visible {
                PlaylistExplorerPage::SourceSelector => breadcrumbs.set(Rc::new(vec![translate.t("LABEL.PLAYLIST_EXPLORER"), translate.t("LABEL.SOURCES")])),
                PlaylistExplorerPage::Create => breadcrumbs.set(Rc::new(vec![translate.t("LABEL.PLAYLIST_EXPLORER"), translate.t("LABEL.CREATE")])),
            }
        });
    };

    let context = PlaylistExplorerContext {
        active_page: active_page.clone(),
        playlist: playlist.clone(),
    };

    html! {
        <ContextProvider<PlaylistExplorerContext> context={context}>
          <div class="tp__playlist-explorer-view tp__list-view">
            <Breadcrumbs items={&*breadcrumbs} onclick={ handle_breadcrumb_select }/>
            <div class="tp__playlist-explorer-view__body tp__list-view__body">
                <Panel value={PlaylistExplorerPage::SourceSelector.to_string()} active={active_page.to_string()}>
                    <PlaylistSourceSelector />
                    <PlaylistExplorer />
                </Panel>
                <Panel value={PlaylistExplorerPage::Create.to_string()} active={active_page.to_string()}>
                    {"Create"}
                </Panel>
            </div>
        </div>
       </ContextProvider<PlaylistExplorerContext>>
    }
}