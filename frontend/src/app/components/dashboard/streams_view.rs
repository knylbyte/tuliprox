use std::rc::Rc;
use yew::prelude::*;
use yew_i18n::use_translation;
use crate::app::components::StreamsTable;
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
          <StreamsTable streams={ (*memo_streams).clone() } />
        </div>
      </div>
    }
}