use yew::prelude::*;
use crate::app::components::{AppIcon};


#[derive(Properties, Clone, PartialEq, Debug)]
pub struct ActionProps {
    #[prop_or_default]
    pub icon: String,
    #[prop_or_default]
    pub title: String,
    #[prop_or_default]
    pub subtitle: String,
    #[prop_or_default]
    pub onaction: Callback<()>,
    pub children: Children,
}

#[function_component]
pub fn ActionCard(props: &ActionProps) -> Html {

    html! {
        <div class="action-card">
            <div class="action-card__icon">
                <AppIcon name={props.icon.clone()} />
            </div>
            <div class="action-card__body">
                <span class="action-card__title">
                    {props.title.clone()}
                </span>
                <span class="action-card__content">
                    {props.subtitle.clone()}
                </span>
            </div>
            {for props.children.iter() }
        </div>
    }
}