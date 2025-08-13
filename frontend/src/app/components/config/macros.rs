#[macro_export]
macro_rules! config_field_optional {
    ($config:expr, $label:expr, $field:ident) => {
        html! {
            <div class="tp__config-field tp__config-field__text">
                <label>{$label}</label>
                <span class="tp__config-field__value">
                {
                    match $config.$field.as_ref() {
                        Some(value) => html! { &value },
                        None => Html::default(),
                    }
                 }
                </span>
            </div>
        }
    };
}

#[macro_export]
macro_rules! config_field_optional_hide {
    ($config:expr, $label:expr, $field:ident) => {
        html! {
            <div class="tp__config-field tp__config-field__text">
                <label>{$label}</label>
                <span class="tp__config-field__value">
                    <$crate::app::components::HideContent content={$config.$field.as_ref().map_or_else(String::new, |content| content.clone())}></$crate::app::components::HideContent>
                </span>
            </div>
        }
    };
}

#[macro_export]
macro_rules! config_field_hide {
    ($config:expr, $label:expr, $field:ident) => {
        html! {
            <div class="tp__config-field tp__config-field__text">
                <label>{$label}</label>
                <span class="tp__config-field__value">
                    <$crate::app::components::HideContent content={$config.$field.clone()}></$crate::app::components::HideContent>
                </span>
            </div>
        }
    };
}

#[macro_export]
macro_rules! config_field_bool {
    ($config:expr, $label:expr, $field:ident) => {
        html! {
            <div class="tp__config-field tp__config-field__bool">
                <label>{$label}</label>
                <$crate::app::components::ToggleSwitch value={$config.$field} readonly={true} />
            </div>
        }
    };
}

#[macro_export]
macro_rules! config_field_bool_empty {
    ($label:expr) => {
        html! {
            <div class="tp__config-field tp__config-field__bool">
                <label>{$label}</label>
                <$crate::app::components::ToggleSwitch value={false} readonly={true} />
            </div>
        }
    };
}

#[macro_export]
macro_rules! config_field {
    ($config:expr, $label:expr, $field:ident) => {
        html! {
            <div class="tp__config-field tp__config-field__text">
                <label>{$label}</label>
                <span class="tp__config-field__value">{$config.$field.to_string()}</span>
            </div>
        }
    };
}

#[macro_export]
macro_rules! config_field_child {
    ($config:expr, $label:expr, $body:block) => {
        html! {
            <div class="tp__config-field tp__config-field__text">
                <label>{$label}</label>
                { $body }
            </div>
        }
    };
}

#[macro_export]
macro_rules! config_field_empty {
    ($label:expr) => {
        html! {
            <div class="tp__config-field tp__config-field__text">
                <label>{$label}</label>
                <span class="tp__config-field__value"></span>
            </div>
        }
    };
}

// pub use config_field;
// pub use config_field_bool;
// pub use config_field_optional;