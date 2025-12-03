use crate::app::components::{AppIcon, FilterView};
use crate::services::DialogService;
use yew::platform::spawn_local;
use yew::prelude::*;
use shared::foundation::filter::get_filter;
use crate::app::{ConfigContext};

#[derive(Properties, Clone, PartialEq, Debug)]
pub struct FilterInputProps {
    #[prop_or_default]
    pub icon: String,
    #[prop_or_default]
    pub filter: Option<String>,
    #[prop_or_default]
    pub on_change: Callback<Option<String>>,
}

#[function_component]
pub fn FilterInput(props: &FilterInputProps) -> Html {
    let config_ctx = use_context::<ConfigContext>().expect("Config context not found");
    let dialog = use_context::<DialogService>().expect("Dialog service not found");

    let filter_state = use_state(|| None);
    let parsed_filter_state = use_state(|| None);
    let templates_state = use_state(|| None);

    {
        let templates = templates_state.clone();
        let cfg_templates = config_ctx.config.as_ref().and_then(|c| c.sources.templates.clone());
        use_effect_with(cfg_templates,  move |templ| {
            templates.set(templ.clone());
        });
    }

    {
        let filter = filter_state.clone();
        let parsed_filter = parsed_filter_state.clone();
        let templates = templates_state.clone();
        use_effect_with(props.filter.clone(), move |flt| {
            filter.set(flt.clone());
            let parsed = if let Some(new_fltr) = flt.as_ref() {
                get_filter(new_fltr, (*templates).as_ref()).ok()
            } else {
                None
            };
            parsed_filter.set(parsed);
        });
    }

    let handle_click = {
        let dialog = dialog.clone();
        let current_filter = filter_state.clone();
        Callback::from(move |e: MouseEvent| {
            e.prevent_default();
            let current_filter = (*current_filter).clone();
            let dlg = dialog.clone();
            spawn_local(async move {
                let filter_view = html!{<div>{current_filter}</div>};
                let _result = dlg.content(filter_view, None).await;
            });
        })
    };

    html! {
        <div class={"tp__filter-input tp__input"} onclick={handle_click}>
        <div class={"tp__input-wrapper"}>
        <span class="tp__filter-input__preview">
        {
            match (*parsed_filter_state).as_ref() {
              None => html! {},
              Some(preview) => html! {
                    <FilterView inline={true} filter={preview.clone()} />
              }
            }
        }
        </span>
         <AppIcon name={if props.icon.is_empty() { "Edit".to_owned() } else {  props.icon.clone()} } />
        </div>
        </div>
    }
}