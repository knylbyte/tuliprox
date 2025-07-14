use std::future;
use std::rc::Rc;
use yew::prelude::*;
use yew::suspense::use_future;
use yew_i18n::use_translation;
use shared::model::ConfigTargetDto;
use crate::app::components::{AppIcon, CollapsePanel, InputRow, InputTable, PlaylistContext, PlaylistPage, TargetTable, TextButton};
use crate::hooks::use_service_context;

#[function_component]
pub fn PlaylistList() -> Html {
    let services = use_service_context();
    let translate = use_translation();
    let playlist_ctx = use_context::<PlaylistContext>();
    let sources = use_state(||None::<Rc<Vec<(Vec<Rc<InputRow>>, Vec<Rc<ConfigTargetDto>>)>>>);

    let handle_create = {
        let playlist_ctx = playlist_ctx.clone();
        Callback::from(move |_| {
            if let Some(ctx) = playlist_ctx.as_ref() {
                ctx.active_page.set(PlaylistPage::NewPlaylist);
            }
        })
    };


    {
        // first register for config update
        let services_ctx = services.clone();
        let sources_state = sources.clone();
        let _ = use_future(|| async move {
            services_ctx.config.config_subscribe(
                &mut |cfg| {
                    if let Some(app_cfg) = cfg.clone() {
                        let mut sources = vec![];
                        for source in &app_cfg.sources.sources {
                            let mut inputs = vec![];
                            for input_cfg in &source.inputs {
                                let input = Rc::new(input_cfg.clone());
                                inputs.push(Rc::new(InputRow::Input(Rc::clone(&input))));
                                if let Some(aliases) = input_cfg.aliases.as_ref() {
                                    for alias in aliases {
                                        inputs.push(Rc::new(InputRow::Alias(Rc::new(alias.clone()), Rc::clone(&input))));
                                    }
                                }
                            }
                            let mut targets = vec![];
                            for target in &source.targets {
                                targets.push(Rc::new(target.clone()));
                            }
                            sources.push((inputs, targets));
                        }
                        sources_state.set(Some(Rc::new(sources)));
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

    let playlist_body = if let Some(data) = &*sources {
        html! {
        <>
            { for data.iter().map(|(inputs, targets)| html! {
                <div class="tp__playlist-list__source">
                    <CollapsePanel class="tp__playlist-list__source-inputs" expanded={false} title_content={Some(html!{<><AppIcon name="Input"/>{translate.t("LABEL.INPUTS")}</>})}>
                        <InputTable inputs={Some(inputs.clone())} />
                    </CollapsePanel>
                    <span class="tp__playlist-list__source-label"><AppIcon name="Target" />{translate.t("LABEL.TARGETS")}</span>
                    <TargetTable targets={Some(targets.clone())} />
                </div>
            }) }
        </>
    }
    } else {
        html! {  }
    };

    html! {
      <div class="tp__playlist-list">
        <div class="tp__playlist-list__header">
          <h1>{ translate.t("LABEL.PLAYLISTS")}</h1>
          <TextButton style="primary" name="new_playlist"
                icon="PlaylistAdd"
                title={ translate.t("LABEL.NEW_PLAYLIST")}
                onclick={handle_create}></TextButton>
        </div>
        <div class="tp__playlist-list__body">
           { playlist_body }
        </div>
      </div>
    }
}