use std::collections::HashSet;
use std::rc::Rc;
use web_sys::MouseEvent;
use yew::{classes, function_component, html, use_node_ref, use_state, Callback, Html, Properties, TargetCast};
use yew_hooks::use_set;
use yew_i18n::use_translation;
use crate::app::components::{AppIcon, IconButton};
use crate::app::components::popup_menu::PopupMenu;
use crate::html_if;

#[derive(Clone, PartialEq, Debug)]
pub struct DropDownOption {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) selected: bool,
}

impl DropDownOption {
    pub fn new(id: &str, label: &str, selected: bool) -> Self {
        Self { id: id.to_owned(), label: label.to_owned(), selected }
    }
}

#[derive(Properties, Clone, PartialEq, Debug)]
pub struct DropDownIconButtonProps {
    pub name: String,
    pub icon: String,
    pub onselect: Callback<(String, Vec<String>)>,
    #[prop_or_default]
    pub class: String,
    pub options: Rc<Vec<DropDownOption>>,
    #[prop_or_default]
    pub multi_select: bool,
}

#[function_component]
pub fn DropDownIconButton(props: &DropDownIconButtonProps) -> Html {
    let translate = use_translation();
    let button_ref = use_node_ref();
    let popup_anchor_ref = use_state(|| None::<web_sys::Element>);
    let popup_is_open = use_state(|| false);
    let selections = use_set(props.options.as_ref().iter().filter(|x| x.selected).map(|x|x.id.clone()).collect::<HashSet<String>>());

    let handle_popup_close = {
        let set_is_open = popup_is_open.clone();
        Callback::from(move |()| {
            set_is_open.set(false);
        })
    };

    let handle_click = {
        let set_anchor_ref = popup_anchor_ref.clone();
        let set_is_open = popup_is_open.clone();
        Callback::from(move |(_name, event): (String, MouseEvent)| {
            if let Some(target) = event.target_dyn_into::<web_sys::Element>() {
                set_anchor_ref.set(Some(target));
                set_is_open.set(true);
            }
        })
    };

    let handle_option_click = {
        let multi_select = props.multi_select;
        let name = props.name.clone();
        let selections = selections.clone();
        let onselect = props.onselect.clone();
        let close_popup = handle_popup_close.clone();
        Callback::from(move |(id, e): (String, MouseEvent)| {
            e.prevent_default();
            if selections.current().contains(&id) {
                selections.remove(&id);
            } else {
                selections.insert(id.clone());
            }
            let selected_options = if multi_select {
                selections.current().iter().map(Clone::clone).collect::<Vec<_>>()
            } else {
                vec![id.clone()]
            };
            onselect.emit((name.clone(), selected_options));
            if !multi_select {
                close_popup.emit(());
            }
        })
    };

    html! {
         <>
         <IconButton button_ref={button_ref} class={props.class.clone()} name={props.name.clone()} icon={props.icon.clone()} onclick={handle_click} />
         <PopupMenu is_open={*popup_is_open} anchor_ref={(*popup_anchor_ref).clone()} on_close={handle_popup_close}>
            {
                for props.options.iter().map(|o| {
                    let checkbox_id = o.id.clone();
                    let checkbox_handler = handle_option_click.clone();
                    let option_click = Callback::from({
                            let id = checkbox_id.clone();
                            move |e| checkbox_handler.emit((id.clone(), e))
                        });
                    html! {
                        <div class={classes!("tp__dropdown-icon-button__option", "tp__menu-item", if selections.current().contains(&o.id) {"checked"} else {"unchecked"})} onclick={option_click}>
                            {
                                html_if!(
                                    props.multi_select,
                                    {
                                        if selections.current().contains(&o.id) {
                                            <AppIcon name={"CheckMark"} />
                                        } else {
                                            <span class={"placeholder"} />
                                        }
                                })
                            }
                            <label>{ translate.t(&o.label) }</label>
                        </div>
                }})
            }
        </PopupMenu>
         </>
    }
}