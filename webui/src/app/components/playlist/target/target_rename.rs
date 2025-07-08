use shared::model::{ConfigTargetDto};
use std::rc::Rc;
use yew::prelude::*;
use yew_i18n::use_translation;

#[derive(Properties, Clone, PartialEq, Debug)]
pub struct TargetRenameProps {
    pub target: Rc<ConfigTargetDto>,
}

#[function_component]
pub fn TargetRename(props: &TargetRenameProps) -> Html {
    let translate = use_translation();
    match props.target.sort.as_ref() {
        None => html! {},
        Some(sort) => html! {
            <div class="tp__target-sort">
                {"Sort"}
            </div>
        }
    }
}