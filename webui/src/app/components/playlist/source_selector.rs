use std::rc::Rc;
use crate::app::components::{Card, CollapsePanel, Panel, PlaylistContext, RadioButtonGroup, TextButton};
use crate::app::context::PlaylistExplorerContext;
use crate::model::ExplorerSourceType;
use std::str::FromStr;
use yew::platform::spawn_local;
use yew::prelude::*;
use yew_i18n::use_translation;
use shared::model::{PlaylistRequest, PlaylistRequestType};
use crate::hooks::use_service_context;

#[function_component]
pub fn PlaylistSourceSelector() -> Html {
    let translate = use_translation();
    let services_ctx = use_service_context();
    let playlist_ctx = use_context::<PlaylistContext>().expect("Playlist context not found");
    let playlist_explorer_ctx = use_context::<PlaylistExplorerContext>().expect("PlaylistExplorer context not found");
    let active_source = use_state(|| ExplorerSourceType::Hosted);

    let handle_source_select = {
        let active_source_clone = active_source.clone();
        Callback::from(move |source_type_str: String| {
            if let Ok(source_type) = ExplorerSourceType::from_str(&source_type_str) {
                active_source_clone.set(source_type)
            }
        })
    };

    let handle_hosted_source = {
        let services = services_ctx.clone();
        let playlist_explorer_ctx_clone = playlist_explorer_ctx.clone();
        Callback::from(move |(target_id, target_name): (u16, String)| {
            let request = PlaylistRequest {
                rtype: PlaylistRequestType::Target,
                username: None,
                password: None,
                url: None,
                source_id: Some(target_id),
                source_name: Some(target_name),
            };
            let services = services.clone();
            let playlist_explorer_ctx_clone = playlist_explorer_ctx_clone.clone();
            spawn_local(async move {
              let playlist = services.playlist.get_playlist_categories(&request).await;
                playlist_explorer_ctx_clone.playlist.set(playlist)
            });
        })
    };

    let playlist_ctx_clone = playlist_ctx.clone();
    let render_hosted = move || {
        html! {
        <>
        {
            if let Some(data) = playlist_ctx_clone.sources.as_ref() {
                html! {
                    <div class="tp__playlist-source-selector__source-list">
                        { for data.iter().flat_map(|(_inputs, targets)| targets)
                            .map(Rc::clone)
                            .map(|target| {
                                let handle_click = handle_hosted_source.clone();
                                html! {
                                <TextButton name={target.name.clone()} title={target.name.clone()} icon={"Download"}
                                onclick={move |_| handle_click.emit((target.id, target.name.clone()))}/>
                                }
                        })}
                    </div>
                }
            } else {
                html! {}
            }
        }
        </>
    }
    };


    html! {
      <div class="tp__playlist-source-selector tp__list-list">
        <div class="tp__playlist-source-selector__header tp__list-list__header">
          <h1>{ translate.t("LABEL.SOURCES")}</h1>
        </div>
        <div class="tp__playlist-source-selector__body tp__list-list__body">
            <CollapsePanel class="tp__playlist-source-selector__source-picker" expanded={true}
               title={translate.t("LABEL.SOURCE_PICKER")}>
               <Card>
                <div class="tp__playlist-source-selector__source-picker__header">
                    <RadioButtonGroup options={vec![
                                    ExplorerSourceType::Hosted.to_string(),
                                    ExplorerSourceType::Provider.to_string(),
                                    ExplorerSourceType::Custom.to_string()]}
                                  selected={(*active_source).to_string()}
                                  on_change={handle_source_select} />
                </div>
                <div class="tp__playlist-source-selector__source-picker__body">
                    <Panel value={ExplorerSourceType::Hosted.to_string()} active={active_source.to_string()}>
                        { render_hosted() }
                    </Panel>
                    <Panel value={ExplorerSourceType::Provider.to_string()} active={active_source.to_string()}>
                        {"provider"}
                    </Panel>
                    <Panel value={ExplorerSourceType::Custom.to_string()} active={active_source.to_string()}>
                        {"custom"}
                    </Panel>
                </div>
              </Card>
            </CollapsePanel>
            <div class="tp__playlist-source-selector__body tp__list-list__selector">

            </div>
        </div>
      </div>
    }
}