use yew::prelude::*;

#[derive(Properties, Clone, PartialEq, Debug)]
pub struct ToggleSwitchProps {
    #[prop_or_default]
    pub value: bool,
}

#[function_component]
pub fn ToggleSwitch(props: &ToggleSwitchProps) -> Html {
    let toggled = use_state(|| props.value);

    let onclick = {
        let toggled = toggled.clone();
        Callback::from(move |_| toggled.set(!*toggled))
    };

    html! {
        <label class="tp__toggle-switch">
            <input type="checkbox"
                   checked={*toggled}
                   onclick={onclick}/>
              <span class={classes!("tp__toggle-switch__track", if *toggled { "tp__toggle-switch__active" } else { "" })}>
               <span class={classes!("tp__toggle-switch__toggle", if *toggled { "tp__toggle-switch__on" } else { "" })}>
              </span>
            </span>
        </label>
    }
}
