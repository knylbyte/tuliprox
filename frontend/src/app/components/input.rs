use web_sys::{HtmlInputElement, KeyboardEvent};
use yew::{function_component, html, use_effect_with, use_state, Callback, Html, NodeRef, Properties, TargetCast};
use crate::app::components::IconButton;
use crate::html_if;

#[derive(Properties, Clone, PartialEq, Debug)]
pub struct InputProps {
    #[prop_or_default]
    pub name: String,
    #[prop_or_default]
    pub label: Option<String>,
    #[prop_or_default]
    pub hidden: bool,
    #[prop_or_default]
    pub input_ref: Option<NodeRef>,
    #[prop_or_default]
    pub autocomplete: bool,
    #[prop_or_default]
    pub onkeydown: Option<Callback<KeyboardEvent>>,
    #[prop_or_default]
    pub ontext: Option<Callback<String>>,
    #[prop_or_default]
    pub value: String,
}

#[function_component]
pub fn Input(props: &InputProps) -> Html {
    let hide_content = use_state(|| props.hidden);
    let local_ref = props.input_ref.clone().unwrap_or_default();

    {
        let local_ref = local_ref.clone();
        use_effect_with(props.value.clone(), move |val| {
            if let Some(input) = local_ref.cast::<HtmlInputElement>() {
                input.set_value(val);
            }
            || ()
        });
    }


    let handle_hide_content = {
      let hide_content = hide_content.clone();
      Callback::from(move |_| {
          hide_content.set(!*hide_content);
      })
    };

    let handle_keydown = {
        let onkeydown_clone = props.onkeydown.clone();
        let ontext_clone = props.ontext.clone();
        Callback::from(move |event: KeyboardEvent| {
            if event.key() == "Enter" {
                event.prevent_default();
            }

            if let Some(input) = event.target_dyn_into::<web_sys::HtmlInputElement>() {
                let value = input.value();
                if let Some(cb) = ontext_clone.as_ref() {
                        cb.emit(value);
                }
            }
            if let Some(cb) = onkeydown_clone.as_ref() {
                cb.emit(event);
            }
        })
    };

    html! {
        <div class="tp__input">
            { if props.label.is_some() {
                   html! {
                       <label>{props.label.clone().unwrap_or_default()}</label>
                   }
                } else { html!{} }
            }
            <div class="tp__input-wrapper">
                <input
                    ref={local_ref.clone()}
                    type={if *hide_content { "password".to_string() } else { "text".to_string() }}
                    name={props.name.clone()}
                    autocomplete={if props.autocomplete { "on".to_string() } else { "off".to_string() }}
                    onkeydown={handle_keydown.clone()}
                    />
                { html_if!(props.hidden, {
                     <IconButton name="hide" icon="Visibility" onclick={handle_hide_content} />
                })}
            </div>
        </div>
    }
}