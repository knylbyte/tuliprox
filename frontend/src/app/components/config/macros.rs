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
macro_rules! config_field_custom {
    ($label:expr, $value:expr) => {
        html! {
            <div class="tp__form-field tp__form-field__text">
                <label>{$label}</label>
                <span class="tp__form-field__value">{$value}</span>
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

pub trait HasFormData {
    type Data;
    fn data(&self) -> &Self::Data;
    //fn data_mut(&mut self) -> &mut Self::Data;
}

#[macro_export]
macro_rules! edit_field_text_option {
    ($instance:expr, $label:expr, $field:ident, $action:path) => {
        $crate::edit_field_text_option!(@inner $instance, $label, $field, $action, false)
    };
    ($instance:expr, $label:expr, $field:ident, $action:path, $hidden:expr) => {
        $crate::edit_field_text_option!(@inner $instance, $label, $field, $action, $hidden)
    };
    (@inner $instance:expr, $label:expr, $field:ident, $action:path, $hidden:expr) => {{
        let instance = $instance.clone();
        html! {
            <div class="tp__form-field tp__form-field__text">
                <$crate::app::components::input::Input
                    label={$label}
                    hidden={$hidden}
                    name={stringify!($field)}
                    autocomplete={true}
                    value={(*instance).data().$field.as_ref().map_or_else(String::new, |v|v.to_string())}
                    on_change={Callback::from(move |value: String| {
                        instance.dispatch($action(if value.is_empty() {
                            None
                        } else {
                            Some(value)
                        }));
                    })}
                />
            </div>
        }
    }};
}

#[macro_export]
macro_rules! edit_field_text {
    ($instance:expr, $label:expr, $field:ident, $action:path) => {
        $crate::edit_field_text!(@inner $instance, $label, $field, $action, false)
    };
    ($instance:expr, $label:expr, $field:ident, $action:path,  $hidden:expr) => {
        $crate::edit_field_text!(@inner $instance, $label, $field, $action, $hidden)
    };
    (@inner $instance:expr, $label:expr, $field:ident, $action:path, $hidden:expr) => {{
        let instance = $instance.clone();
        html! {
            <div class="tp__form-field tp__form-field__text">
                <$crate::app::components::input::Input
                    label={$label}
                    hidden={$hidden}
                    name={stringify!($field)}
                    autocomplete={true}
                    value={(*instance).data().$field.clone()}
                    on_change={Callback::from(move |value: String| {
                        instance.dispatch($action(value));
                    })}
                />
            </div>
        }
    }};
}

#[macro_export]
macro_rules! edit_field_bool {
    ($instance:expr, $label:expr, $field:ident, $action:path) => {{
        let instance = $instance.clone();
        html! {
            <div class="tp__form-field tp__form-field__bool">
                <label>{$label}</label>
                <$crate::app::components::ToggleSwitch
                     value={(*instance).data().$field}
                     readonly={false}
                     on_change={Callback::from(move |value| instance.dispatch($action(value)))} />
            </div>
        }
    }};
}

#[macro_export]
macro_rules! edit_field_number {
    ($instance:expr, $label:expr, $field:ident, $action:path) => {{
        let instance = $instance.clone();
        html! {
            <div class="tp__form-field tp__form-field__number">
                <$crate::app::components::number_input::NumberInput
                    label={$label}
                    name={stringify!($field)}
                    value={(*instance).data().$field.clone()}
                    on_change={Callback::from(move |value: Option<u32>| {
                        match value {
                            Some(value) => instance.dispatch($action(value)),
                            None => instance.dispatch($action(0)),
                        }
                    })}
                />
            </div>
        }
    }};
}

#[macro_export]
macro_rules! edit_field_date {
    ($instance:expr, $label:expr, $field:ident, $action:path) => {{
        let instance = $instance.clone();
        html! {
            <div class="tp__form-field tp__form-field__date">
                <$crate::app::components::date_input::DateInput
                    label={$label}
                    name={stringify!($field)}
                    value={(*instance).data().$field.clone()}
                    on_change={Callback::from(move |value: Option<i64>| {
                        instance.dispatch($action(value));
                    })}
                />
            </div>
        }
    }};
}

#[macro_export]
macro_rules! generate_form_reducer {
    (
        state: $state_name:ident { $data_field:ident: $data_type:ty },
        action_name: $action_name:ident,
        fields {
            $($set_name:ident => $field_name:ident : $field_type:ty),* $(,)?
        }
    ) => {
        #[derive(Debug, Clone, PartialEq)]
        pub struct $state_name {
            pub $data_field: $data_type,
            modified: bool,
        }

        impl $state_name {
            pub fn modified(&self) -> bool {
                self.modified
            }
        }

        #[derive(Clone)]
        pub enum $action_name {
            $(
                $set_name($field_type),
            )*
            SetAll($data_type),
        }

        impl yew::prelude::Reducible for $state_name {
            type Action = $action_name;

            fn reduce(self: std::rc::Rc<Self>, action: Self::Action) -> std::rc::Rc<Self> {
                let mut new_data = self.$data_field.clone();
                let mut modified = self.modified;
                match action {
                    $(
                        $action_name::$set_name(v) => {
                            new_data.$field_name = v;
                            if !modified { modified = true; }
                        },
                    )*
                    $action_name::SetAll(v) => {
                        new_data = v;
                        modified = false;
                    },
                }
                $state_name { $data_field: new_data, modified }.into()
            }
        }

        impl $crate::app::components::config::HasFormData for $state_name {
            type Data = $data_type;

            fn data(&self) -> &Self::Data {
                &self.$data_field
            }

            // fn data_mut(&mut self) -> &mut Self::Data {
            //     &mut self.$data_field
            // }
        }
    };
}
