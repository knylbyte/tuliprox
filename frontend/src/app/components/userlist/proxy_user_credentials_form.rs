use std::cell::RefCell;
use std::rc::Rc;
use log::warn;
use yew::prelude::*;
use yew_i18n::use_translation;
use shared::model::{ProxyUserCredentialsDto, ProxyUserStatus};
use crate::app::TargetUser;
use crate::{edit_field_bool, edit_field_text, edit_field_text_option};
use crate::app::components::select::Select;
use crate::app::components::{convert_bool_to_chip_style, Chip, DropDownOption, TextButton};

#[derive(Properties, PartialEq, Clone)]
pub struct ProxyUserCredentialsFormProps {
    pub user: Option<Rc<TargetUser>>,
}

#[function_component]
pub fn ProxyUserCredentialsForm(props: &ProxyUserCredentialsFormProps) -> Html {
    let translate = use_translation();
    let form_state = use_memo(props.user.clone(),
                              |user| RefCell::new(user.as_ref()
                                  .map_or_else(|| ProxyUserCredentialsDto::default(), |usr| usr.credentials.as_ref().clone())));

    let proxy_user_status = use_memo(props.user.clone(), |user|
         vec![
              ProxyUserStatus::Active,
              ProxyUserStatus::Expired,
              ProxyUserStatus::Banned,
              ProxyUserStatus::Trial,
              ProxyUserStatus::Disabled,
              ProxyUserStatus::Pending,
        ].iter().map(|s| DropDownOption {
              id: s.to_string(),
              label: format!("LABEL.USER_STATUS_{}", s.to_string().to_uppercase()),
              selected: user.as_ref().is_some_and(|user| user.credentials.status.as_ref() == Some(s)),
          }).collect::<Vec<DropDownOption>>()
    );

    let handle_save_user = {
      let user = form_state.clone();
      Callback::from(move |_| {
          warn!("{:?}", user.borrow());
      })
    };

    let user_active = props.user.as_ref().is_some_and(|u| u.credentials.is_active());

    html! {
        <div class="tp__proxy-user-credentials-form tp__form-page">
          <div class="tp__proxy-user-credentials-form__body tp__form-page__body">
            <div class="tp__config-field tp__config-field__bool">
                <Chip class={ convert_bool_to_chip_style(user_active) }
                    label={if user_active {translate.t("LABEL.ENABLED")} else { translate.t("LABEL.DISABLED")} }
                />
            </div>
            <div class="tp__config-field tp__config-field__text">
                <label>{translate.t("LABEL.STATUS")}</label>
                <Select name="status"
                    multi_select={false}
                    onselect={Callback::from(move |(_name, selections):(String, Vec<String>)| {
                        warn!("{}", selections.join(", "));
                    })}
                    options={proxy_user_status.clone()}
                />
            </div>
            { edit_field_text!(*form_state,  translate.t("LABEL.USERNAME"), username) }
            { edit_field_text!(*form_state,  translate.t("LABEL.PASSWORD"), password, true) }
            { edit_field_bool!(*form_state,  translate.t("LABEL.USER_UI_ENABLED"), ui_enabled) }
            { edit_field_text_option!(*form_state,  translate.t("LABEL.TOKEN"), token, true) }
            <label>{ translate.t("LABEL.PROXY") }</label>
            <span>{"TODO"}</span>

    // pub server: Option<String>,
    // pub epg_timeshift: Option<String>,
    // pub created_at: Option<i64>,
    // pub exp_date: Option<i64>,
    // pub max_connections: u32,

            { edit_field_text_option!(*form_state,  translate.t("LABEL.COMMENT"), comment) }

          </div>
          <div class="tp__proxy-user-credentials-form__toolbar tp__form-page__toolbar">
             <TextButton class="primary" name="save_user"
                icon="Save"
                title={ translate.t("LABEL.SAVE")}
                onclick={handle_save_user}></TextButton>
          </div>
        </div>
    }
}
