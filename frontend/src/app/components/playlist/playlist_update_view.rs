use crate::app::components::{Breadcrumbs, Card, PlaylistContext, TextButton};
use std::rc::Rc;
use yew::platform::spawn_local;
use yew::prelude::*;
use yew_hooks::use_list;
use yew_i18n::use_translation;
use shared::model::ConfigTargetDto;
use crate::hooks::use_service_context;

#[function_component]
pub fn PlaylistUpdateView() -> Html {
    let translate = use_translation();
    let playlist_ctx = use_context::<PlaylistContext>().expect("Playlist context not found");
    let services_ctx = use_service_context();
    let breadcrumbs = use_state(|| Rc::new(vec![translate.t("LABEL.PLAYLISTS"), translate.t("LABEL.UPDATE")]));
    let selected_targets = use_list::<Rc<ConfigTargetDto>>(vec![]);

    let handle_all_select = {
        let selected_targets = selected_targets.clone();
        Callback::from(move |_| {
           selected_targets.clear();
        })
    };


    let handle_target_select = {
        let selected_targets = selected_targets.clone();
        Callback::from(move |target: Rc<ConfigTargetDto>| {
            let exists = selected_targets.current().iter().any(|t| t.id == target.id);
            if !exists {
                selected_targets.push(target);
            }
        })
    };


    let handle_update = {
        let translate = translate.clone();
        let services = services_ctx.clone();
        let selected_targets = selected_targets.clone();
        Callback::from(move |_| {
            let selected_targets = selected_targets.clone();
            let services = services.clone();
            let translate = translate.clone();
            spawn_local(async move {
                let target_names = {
                    let targets = selected_targets.current();
                    targets.iter().map(|t| t.name.clone()).collect::<Vec<String>>()
                };
                let update_target_names = target_names.iter().map(|t|t.as_str()).collect::<Vec<&str>>();
                match services.playlist.update_targets(&update_target_names).await {
                    true => { services.toastr.success(translate.t("MESSAGES.PLAYLIST_UPDATE.SUCCESS")); }
                    false => { services.toastr.error(translate.t("MESSAGES.PLAYLIST_UPDATE.FAIL")); }
                }
            });
        })
    };

    html! {
      <div class="tp__playlist-update-view">
         <Breadcrumbs items={&*breadcrumbs}/>
         <div class="tp__playlist-update-view__header">
          <h1>{ translate.t("LABEL.UPDATE")}</h1>
          <TextButton class="primary" name="playlist_update"
               icon="Refresh"
               title={ translate.t("LABEL.UPDATE")}
               onclick={handle_update}></TextButton>
        </div>
        <Card>
         <div class="tp__playlist-update-view__body">
            <TextButton class={if selected_targets.current().is_empty() { "active" } else {""}}
                name={translate.t("LABEL.ALL")} title={translate.t("LABEL.ALL")} icon={"SelectAll"}
                onclick={handle_all_select}/>

         {
            if let Some(data) = playlist_ctx.sources.as_ref() {
              data.iter().flat_map(|(_inputs, targets)| targets)
                    .map(Rc::clone)
                    .map(|target| {
                        let handle_click = handle_target_select.clone();
                        let target_name = target.name.clone();
                        html! {
                          <TextButton class={if selected_targets.current().iter().any(|t| t.id == target.id) { "active" } else {""}}
                            name={target_name.clone()} title={target_name} icon={"UpdateChecked"}
                             onclick={move |_| handle_click.emit(target.clone())}/>
                        }
              }).collect::<Html>()
            } else {
              html! {<></>}
            }
         }
         </div>
         </Card>
      </div>
    }
}