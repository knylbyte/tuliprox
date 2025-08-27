use web_sys::{HtmlInputElement, InputEvent, KeyboardEvent, MouseEvent};
use yew::{function_component, html, use_effect_with, use_node_ref, use_state, Callback, Html, NodeRef, Properties, TargetCast};
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
    pub on_change: Option<Callback<String>>,
    #[prop_or_default]
    pub value: String,
}

#[function_component]
pub fn Input(props: &InputProps) -> Html {
    let icon_btn_ref = use_node_ref();
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
      let button_ref = icon_btn_ref.clone();
      Callback::from(move |(_, event): (String, MouseEvent)| {
          if let Some(target) = event.target_dyn_into::<web_sys::HtmlElement>() {
              if let Some(button) = button_ref.cast::<web_sys::HtmlElement>() {
                  if button == target {
                      hide_content.set(!*hide_content);
                  }
              }
          }
      })
    };

    let handle_oninput = {
        let ontext_clone = props.on_change.clone();
        Callback::from(move |event: InputEvent| {
            if let Some(input) = event.target_dyn_into::<web_sys::HtmlInputElement>() {
                let value = input.value();
                if let Some(cb) = ontext_clone.as_ref() {
                    cb.emit(value);
                }
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
                    onkeydown={props.onkeydown.clone()}
                    oninput={handle_oninput}
                    />
                { html_if!(props.hidden, {
                     <IconButton button_ref={icon_btn_ref} name="hide" icon="Visibility" class={if !*hide_content {"active"} else {""}} onclick={handle_hide_content} />
                })}
            </div>
        </div>
    }
}