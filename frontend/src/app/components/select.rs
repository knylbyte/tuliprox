use std::rc::Rc;
use yew::prelude::*;
use crate::app::components::{DropDownIconButton, DropDownOption};

#[derive(Properties, Clone, PartialEq, Debug)]
pub struct SelectProps {
    pub name: String,
    #[prop_or_default]
    pub icon: Option<String>,
    pub onselect: Callback<(String, Vec<Rc<DropDownOption>>)>,
    #[prop_or_default]
    pub class: String,
    pub options: Vec<Rc<DropDownOption>>,
    #[prop_or_default]
    pub multi_select: bool,
}

#[function_component]
pub fn Select(props: &SelectProps) -> Html {
    let button_ref = use_node_ref();

    let selected_options = use_state(|| props.options.iter().filter(|o| o.selected)
        .map(|o| o.label.clone()).collect::<Vec<Html>>());
    {
        let set_selected_options = selected_options.clone();
        use_effect_with(props.options.clone(), move |options: &Vec<Rc<DropDownOption>>| {
            let selections = options.iter().filter(|o| o.selected)
                .map(|o| o.label.clone()).collect::<Vec<Html>>();
            set_selected_options.set(selections);
        });
    }

    let handle_options_click = {
        let onselect = props.onselect.clone();
        let options = props.options.clone();
        let set_selections = selected_options.clone();
        Callback::from(move |(name, selections): (String, Vec<String>)| {
            let selected_options: Vec<Rc<DropDownOption>> = options.iter().filter(|&o| selections.contains(&o.id)).cloned().collect();
            set_selections.set(selected_options.iter().map(|o| o.label.clone()).collect());
            onselect.emit((name, selected_options));
        })
    };

    let handle_click_button = {
        let button_ref = button_ref.clone();
        Callback::from(move |event: MouseEvent| {
            if let Some(target) = event.target_dyn_into::<web_sys::Element>() {
                if target.class_name().contains("tp__select-wrapper") {
                    if let Some(button) = button_ref.cast::<web_sys::HtmlElement>() {
                        button.click();
                    }
                }
            }
        })
    };

    html! {
        <div class={classes!("tp__select", props.class.clone())}>
            <div class="tp__select-wrapper" onclick={handle_click_button}>
                <div class="tp__select__selected">
                    {(*selected_options).clone()}
                </div>
                <DropDownIconButton
                     button_ref={button_ref}
                     multi_select={props.multi_select}
                     options={props.options.clone()}
                     name={props.name.clone()}
                     icon={props.icon.as_ref().map_or_else(|| "Popup".to_owned(), |i|i.to_string())}
                     onselect={handle_options_click} />
            </div>
        </div>
    }
}