#[macro_export]
macro_rules! config_field_optional {
    ($config:expr, $label:expr, $field:ident) => {
        html! {
            <div class="tp__form-field tp__form-field__text">
                <label>{$label}</label>
                <span class="tp__form-field__value">
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
            <div class="tp__form-field tp__form-field__text">
                <label>{$label}</label>
                <span class="tp__form-field__value">
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
            <div class="tp__form-field tp__form-field__text">
                <label>{$label}</label>
                <span class="tp__form-field__value">
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
            <div class="tp__form-field tp__form-field__bool">
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
            <div class="tp__form-field tp__form-field__bool">
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
            <div class="tp__form-field tp__form-field__text">
                <label>{$label}</label>
                <span class="tp__form-field__value">{$config.$field.to_string()}</span>
            </div>
        }
    };
}

#[macro_export]
macro_rules! config_field_child {
    ($label:expr, $body:block) => {
        html! {
            <div class="tp__form-field tp__form-field__text">
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
            <div class="tp__form-field tp__form-field__text">
                <label>{$label}</label>
                <span class="tp__form-field__value"></span>
            </div>
        }
    };
}

#[macro_export]
macro_rules! edit_field_text_option {
    ($instance:expr, $label:expr, $field:ident) => {
        $crate::edit_field_text_option!(@inner $instance, $label, $field, false)
    };
    ($instance:expr, $label:expr, $field:ident, $hidden:expr) => {
        $crate::edit_field_text_option!(@inner $instance, $label, $field, $hidden)
    };
    (@inner $instance:expr, $label:expr, $field:ident, $hidden:expr) => {{
        let instance = $instance.clone();
        html! {
            <div class="tp__form-field tp__form-field__text">
                <$crate::app::components::input::Input
                    label={$label}
                    hidden={$hidden}
                    name={stringify!($field)}
                    autocomplete={true}
                    value={instance.borrow().$field.as_ref().map_or_else(String::new, |v|v.to_string())}
                    ontext={Callback::from(move |value: String| {
                        instance.borrow_mut().$field = if value.is_empty() {
                            None
                        } else {
                            Some(value)
                        };
                    })}
                />
            </div>
        }
    }};
}

#[macro_export]
macro_rules! edit_field_text {
    ($instance:expr, $label:expr, $field:ident) => {
        $crate::edit_field_text!(@inner $instance, $label, $field, false)
    };
    ($instance:expr, $label:expr, $field:ident, $hidden:expr) => {
        $crate::edit_field_text!(@inner $instance, $label, $field, $hidden)
    };
    (@inner $instance:expr, $label:expr, $field:ident, $hidden:expr) => {{
        let instance = $instance.clone();
        html! {
            <div class="tp__form-field tp__form-field__text">
                <$crate::app::components::input::Input
                    label={$label}
                    hidden={$hidden}
                    name={stringify!($field)}
                    autocomplete={true}
                    value={instance.borrow().$field.clone()}
                    ontext={Callback::from(move |value: String| {
                        instance.borrow_mut().$field = value;
                    })}
                />
            </div>
        }
    }};
}

//<Input label={translate.t("LABEL.PASSWORD")} input_ref={password_ref} input_type="password" name="password"  autocomplete={false} onkeydown={handle_key_down.clone()}/>

#[macro_export]
macro_rules! edit_field_bool {
    ($instance:expr, $label:expr, $field:ident) => {{
        let instance = $instance.clone();
        html! {
            <div class="tp__form-field tp__form-field__bool">
                <label>{$label}</label>
                <$crate::app::components::ToggleSwitch
                     value={instance.borrow().$field}
                     readonly={false}
                     onchange={Callback::from(move |value| instance.borrow_mut().$field = value)} />
            </div>
        }
    }};
}