use std::rc::Rc;
use yew::prelude::*;
use yew_i18n::use_translation;
use crate::app::components::{Card, StatusCard, StreamsTable};
use crate::app::StatusContext;

#[function_component]
pub fn StreamsView() -> Html {
    let translate = use_translation();
    let status_ctx = use_context::<StatusContext>().expect("Status context not found");

    let memo_streams = {
        let status = status_ctx.status.clone();
        use_memo(status, |s| {
            s.as_ref().map(|st| st.active_user_streams.iter().cloned().map(Rc::new).collect::<Vec<_>>())
        })
    };

    html! {
      <div class="tp__streams">
        <div class="tp__streams__header">
         <h1>{ translate.t("LABEL.STREAMS")}</h1>
        </div>
        <div class="tp__streams__body">
           <div class="tp__stats__body-group">
                <Card><StatusCard title={translate.t("LABEL.ACTIVE_USERS")} data={status_ctx.status.as_ref().map_or_else(|| "n/a".to_string(), |status| status.active_users.to_string())} /></Card>
                <Card><StatusCard title={translate.t("LABEL.ACTIVE_USER_CONNECTIONS")} data={status_ctx.status.as_ref().map_or_else(|| "n/a".to_string(), |status| status.active_user_connections.to_string())} /></Card>
                <Card>
                    <StatusCard
                        title={translate.t("LABEL.ACTIVE_PROVIDER_CONNECTIONS")}
                        data={status_ctx.status.as_ref().map_or_else(|| "n/a".to_string(), |status|
                                status. active_provider_connections.as_ref().map(|map| map.values().sum::<usize>()).unwrap_or(0).to_string())}
                    />
                 </Card>
            </div>
            <StreamsTable streams={ (*memo_streams).clone() } />
        </div>
      </div>
    }
}