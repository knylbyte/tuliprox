use web_sys::MouseEvent;
use yew::{function_component, html, Callback, Html, Properties};
use crate::app::components::AppIcon;

#[derive(Properties, Clone, PartialEq, Debug)]
pub struct MenuItemProps {
    pub name: String,
    pub label: String,
    pub icon: String,
    pub onclick: Callback<String>,
}

#[function_component]
pub fn MenuItem(props: &MenuItemProps) -> Html {

    let handle_click = {
        let click = props.onclick.clone();
        let name = props.name.clone();
        Callback::from(move |e: MouseEvent| {
            e.prevent_default();
            click.emit(name.clone());
        })
    };

    html! {
        <div class="menu-item" onclick={ handle_click }>
            <AppIcon name={props.icon.clone()}></AppIcon>
            <label>{props.label.clone()}</label>
        </div>
    }
}