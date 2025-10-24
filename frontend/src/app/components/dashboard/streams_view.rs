use std::rc::Rc;
use yew::prelude::*;
use yew_i18n::use_translation;
use crate::app::components::StreamsTable;
use crate::app::StatusContext;

#[function_component]
pub fn StreamsView() -> Html {
    let translate = use_translation();
    let status_ctx = use_context::<StatusContext>().expect("Status context not found");

    html! {
      <div class="tp__streams">
        <div class="tp__streams__header">
         <h1>{ translate.t("LABEL.STREAMS")}</h1>
        </div>
        <div class="tp__streams__body">
          <StreamsTable streams={ status_ctx.status.as_ref().map(|s| s.active_user_streams.iter().map(|si|Rc::new(si.clone())).collect() ) } />
        </div>
      </div>
    }
}