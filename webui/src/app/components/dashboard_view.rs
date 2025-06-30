use gloo_timers::callback::Interval;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use yew_i18n::use_translation;
use crate::app::components::{Card, UserActionCard, VersionActionCard};
use crate::hooks::use_service_context;

#[function_component]
pub fn DashboardView() -> Html {
    let translate = use_translation();
    let services = use_service_context();
    let status =  use_state(|| None);

    {
        let services_ctx = services.clone();
        let status_clone = status.clone();

        use_effect(move || {
            let fetch_status = {
                let status_clone = status_clone.clone();
                let services_ctx = services_ctx.clone();
                move || {
                    let status_clone = status_clone.clone();
                    let services_ctx = services_ctx.clone();
                    spawn_local(async move {
                        status_clone.set(services_ctx.status.get_server_status().await.ok());
                    });
                }
            };

            fetch_status();
            // all 5 seconds
            let interval = Interval::new(5000, move || {
                fetch_status();
            });

            // Cleanup
            || drop(interval)
        });
    }

    html! {
      <div class="dashboard">
        <div class="dashboard__header">
         <h1>{ translate.t("LABEL.DASHBOARD")}</h1>
        </div>
        <div class="dashboard__body">
          <Card><VersionActionCard version={(*status).as_ref().map_or_else(String::new,  |s| s.version.clone())}
                 build_time={(*status).as_ref().map_or_else(String::new,  |s| s.build_time.as_ref().map_or_else(String::new, |v| v.clone()))}/></Card>
          <Card><UserActionCard /></Card>
        </div>
      </div>
    }
}