use crate::app::components::{convert_bool_to_chip_style, CollapsePanel, Tag, TagList};
use shared::model::{ClusterFlags, ConfigTargetDto};
use std::rc::Rc;
use yew::prelude::*;
use yew_i18n::use_translation;

#[derive(Properties, Clone, PartialEq, Debug)]
pub struct TargetWatchProps {
    pub target: Rc<ConfigTargetDto>,
}

#[function_component]
pub fn TargetWatch(props: &TargetWatchProps) -> Html {
    let translate = use_translation();
    match props.target.watch.as_ref() {
        None => html! {},
        Some(watch) => html! {
            <div class="tp__target-watch">
                <ul>
                    { watch.iter().map(|item| html! { <li>{ item }</li> }).collect::<Html>() }
                </ul>
            </div>
        }
    }
}