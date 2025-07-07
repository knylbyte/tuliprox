use shared::model::{ConfigTargetDto};
use std::rc::Rc;
use yew::prelude::*;
use yew_i18n::use_translation;

#[derive(Properties, Clone, PartialEq, Debug)]
pub struct TargetSortProps {
    pub target: Rc<ConfigTargetDto>,
}

#[function_component]
pub fn TargetSort(props: &TargetSortProps) -> Html {
    let translate = use_translation();
    match props.target.sort.as_ref() {
        None => html! {},
        Some(watch) => html! {
            <div class="tp__target-sort">
                {"Sort"}
            </div>
        }
    }
}