use yew::{classes, function_component, html, Html, Properties};

#[derive(Properties, Clone, PartialEq, Debug)]
pub struct LoadingIndicatorProps {
    pub loading: bool,
    #[prop_or_default]
    pub class: String,
}

#[function_component]
pub fn LoadingIndicator(props: &LoadingIndicatorProps) -> Html {
    if !props.loading {
        html! {<div class={classes!("tp__loading-bar-placeholder", props.class.clone())}></div> }
    } else {
        html! {
         <div class={classes!("tp__loading-bar-container", props.class.clone())}>
            <div class="tp__loading-bar"></div>
          </div>
        }
    }
}