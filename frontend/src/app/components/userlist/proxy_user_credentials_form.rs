use std::cell::RefCell;
use std::rc::Rc;
use log::warn;
use yew::prelude::*;
use yew_i18n::use_translation;
use shared::model::ProxyUserCredentialsDto;
use crate::app::TargetUser;
use crate::{edit_field_bool, edit_field_text, edit_field_text_option};
use crate::app::components::TextButton;

#[derive(Properties, PartialEq, Clone)]
pub struct ProxyUserCredentialsFormProps {
    pub user: Option<Rc<TargetUser>>,
}

#[function_component]
pub fn ProxyUserCredentialsForm(props: &ProxyUserCredentialsFormProps) -> Html {
    let translate = use_translation();
    let form_state = use_memo(props.user.clone(), |user| Rc::new(RefCell::new(user.as_ref().map_or_else(|| ProxyUserCredentialsDto::default(), |usr| usr.credentials.as_ref().clone()))));

    let handle_save_user = {
        let user = form_state.clone();
      Callback::from(move |_| {
          warn!("{:?}", user.borrow());
      })
    };

    html! {
        <div class="tp__proxy-user-credentials-form tp__form-page">
          <div class="tp__proxy-user-credentials-form__body tp__form-page__body">
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
    // pub status: Option<ProxyUserStatus>,

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
