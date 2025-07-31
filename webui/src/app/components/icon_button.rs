use web_sys::MouseEvent;
use yew::{classes, function_component, html, Callback, Html, Properties};
use crate::app::components::AppIcon;

#[derive(Properties, Clone, PartialEq, Debug)]
pub struct IconButtonProps {
    pub name: String,
    pub icon: String,
    pub onclick: Callback<String>,
    #[prop_or_default]
    pub style: String,
}

#[function_component]
pub fn IconButton(props: &IconButtonProps) -> Html {

    let handle_click = {
        let click = props.onclick.clone();
        let name = props.name.clone();
        Callback::from(move |e: MouseEvent| {
            e.prevent_default();
            click.emit(name.clone());
        })
    };

    html! {
        <button class={classes!("tp__icon-button", props.style.clone())} onclick={handle_click}>
            <AppIcon name={props.icon.clone()}></AppIcon>
        </button>
    }
}