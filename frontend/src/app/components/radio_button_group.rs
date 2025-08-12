use yew::prelude::*;
use crate::app::components::TextButton;

#[derive(Properties, PartialEq, Clone)]
pub struct RadioButtonGroupProps {
    pub options: Vec<String>,
    pub selected: String,
    pub on_change: Callback<String>,
}

#[function_component]
pub fn RadioButtonGroup(props: &RadioButtonGroupProps) -> Html {
    let on_click = {
        let on_change = props.on_change.clone();
        move |value: String| {
            on_change.emit(value);
        }
    };

    html! {
        <div class="tp__radio-button-group">
            { for props.options.iter().map(|option| {
                let is_selected = *option == props.selected;
                let class = if is_selected {
                    "primary"
                } else {
                    ""
                };

                let label = option.clone();
                let onclick = {
                    let label = label.clone();
                    let on_click = on_click.clone();
                    Callback::from(move |_| on_click(label.clone()))
                };

                html! {
                    <TextButton {onclick} class={class} name={ option.clone() } title={ option.clone() }></TextButton>
                }
            }) }
        </div>
    }
}
