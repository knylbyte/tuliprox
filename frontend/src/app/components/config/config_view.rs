use crate::app::components::config::MainConfigView;
use yew::prelude::*;
use yew_i18n::use_translation;
use crate::app::components::Card;

#[function_component]
pub fn ConfigView() -> Html {
    let translate = use_translation();

    html! {
        <div class="tp__config-view">
            <div class="tp__config-view__header">
                <h1>{ translate.t("LABEL.CONFIG")}</h1>
            </div>
            <div class="tp__config-view__body">
            <Card>
                <MainConfigView />
            </Card>
            </div>
        </div>
    }
}