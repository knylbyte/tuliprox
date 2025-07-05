use yew::prelude::*;
use crate::app::components::AppIcon;

#[derive(Properties, Clone, PartialEq, Debug)]
pub struct RevealContentProps {
    #[prop_or_default]
    pub icon: String,
    pub children: Children,
}

#[function_component]
pub fn RevealContent(props: &RevealContentProps) -> Html {
    html! {
        <div class={"tp__reveal_content"}>
            <AppIcon name={if props.icon.is_empty() {"Ellipsis".to_string()} else {props.icon.to_string()} } />
            // { for props.children.iter() }
        </div>
    }
}