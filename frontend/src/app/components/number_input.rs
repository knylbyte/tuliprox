use web_sys::{HtmlInputElement, KeyboardEvent};
use yew::prelude::*;
use yew::TargetCast;

#[derive(Properties, Clone, PartialEq)]
pub struct NumberInputProps {
    #[prop_or_default]
    pub name: String,
    #[prop_or_default]
    pub label: Option<String>,
    #[prop_or_default]
    pub value: Option<i64>,
    #[prop_or_default]
    pub on_change: Callback<Option<i64>>,
    #[prop_or_default]
    pub placeholder: String,
}

#[function_component]
pub fn NumberInput(props: &NumberInputProps) -> Html {
    let input_ref = use_node_ref();

    {
        let input_ref = input_ref.clone();
        let value = props.value;
        use_effect_with(value, move |val| {
            if let Some(input) = input_ref.cast::<HtmlInputElement>() {
                match val {
                    Some(v) => input.set_value(&v.to_string()),
                    None => input.set_value(""),
                }
            }
            || ()
        });
    }

    let on_input = {
        let onchange = props.on_change.clone();
        Callback::from(move |e: InputEvent| {
            if let Some(input) = e.target_dyn_into::<HtmlInputElement>() {
                let raw = input.value();
                let parsed = raw.parse::<i64>().ok();
                onchange.emit(parsed);
            }
        })
    };

    let handle_keydown = {
        Callback::from(move |e: KeyboardEvent| {
            let key = e.key();
            let allowed = key.chars().all(|c| c.is_ascii_digit())
                || key == "Backspace"
                || key == "Delete"
                || key == "ArrowLeft"
                || key == "ArrowRight"
                || key == "Tab"
                || key == "Enter"
                || key == "."
                || key == ","
                || key == "-";

            if !allowed {
                e.prevent_default();
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
                    ref={input_ref.clone()}
                    type="text" // type is text to avoid browser validation
                    name={props.name.clone()}
                    placeholder={props.placeholder.clone()}
                    onkeydown={handle_keydown.clone()}
                    oninput={on_input.clone()}
                    />
            </div>
        </div>
    }
}
