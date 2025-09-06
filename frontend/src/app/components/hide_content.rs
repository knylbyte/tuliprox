use yew::prelude::*;
use crate::app::components::AppIcon;

#[derive(Properties, Clone, PartialEq, Debug)]
pub struct HideContentProps {
    #[prop_or_default]
    pub icon: String,
    pub content: Html,
}

#[function_component]
pub fn HideContent(props: &HideContentProps) -> Html {

    let hidden = use_state(|| true);

    let handle_click = {
        let hidden = hidden.clone();
        Callback::from(move |e: MouseEvent| {
            e.prevent_default();
            hidden.set(!*hidden);
        })
    };

    html! {
        <div class={classes!("tp__hide-content", if !*hidden {"active"} else {""})} onclick={handle_click}>
          <span class={"tp__hide-content__text"}>
          {
            if *hidden {
                html! { "******" }
            } else {
              props.content.clone()
            }
          }
          </span>
          <AppIcon name={if props.icon.is_empty() { "Visibility".to_string() } else {props.icon.to_string()} } />
        </div>
    }
}