use std::rc::Rc;
use log::warn;
use yew::prelude::*;
use crate::app::components::{DropDownIconButton, DropDownOption, Tag, TagList};

#[derive(Properties, Clone, PartialEq, Debug)]
pub struct SelectProps {
    pub name: String,
    #[prop_or_default]
    pub icon: Option<String>,
    pub onselect: Callback<(String, Vec<String>)>,
    #[prop_or_default]
    pub class: String,
    pub options: Rc<Vec<DropDownOption>>,
    #[prop_or_default]
    pub multi_select: bool,
}

fn create_tag_from_str(s: &str) -> Rc<Tag> {
    Rc::new(Tag { label: s.to_owned(), class: Some("active".to_string()) })
}

fn create_tag_from_option(o: &DropDownOption) -> Rc<Tag> {
  create_tag_from_str(o.label.as_str())
}

#[function_component]
pub fn Select(props: &SelectProps) -> Html {

    // TODO render for selections !!!

    let selected_options = use_state(|| props.options.as_ref().iter().filter(|o| o.selected)
        .map(create_tag_from_option)
        .collect::<Vec<Rc<Tag>>>());
    {
        let set_selected_options = selected_options.clone();
        use_effect_with(props.options.clone(), move |options| {
            let selections = options.as_ref().iter().filter(|o| o.selected)
                .map(create_tag_from_option)
                .collect::<Vec<Rc<Tag>>>();
            set_selected_options.set(selections);
        });
    }

    let handle_options_click = {
        let onselect = props.onselect.clone();
        let set_selections = selected_options.clone();
        Callback::from(move |(name, selections): (String, Vec<String>)| {
            set_selections.set(selections.iter().map(|s|s.as_str()).map(create_tag_from_str).collect::<Vec<Rc<Tag>>>());
            onselect.emit((name, selections));
        })
    };

    html! {
        <div class={classes!("tp__select", props.class.clone())}>
            <div class="tp__select-wrapper">
                <div class="tp__select__selected">
                    <TagList tags={(*selected_options).clone()} />
                </div>
                <DropDownIconButton
                     multi_select={props.multi_select}
                     options={props.options.clone()}
                     name={props.name.clone()}
                     icon={props.icon.as_ref().map_or_else(|| "Popup".to_owned(), |i|i.to_string())}
                     onselect={handle_options_click} />
            </div>
        </div>
    }
}