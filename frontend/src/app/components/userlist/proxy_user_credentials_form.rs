use std::cell::RefCell;
use std::rc::Rc;
use log::warn;
use yew::prelude::*;
use yew_i18n::use_translation;
use shared::model::{ApiProxyServerInfoDto, ConfigTargetDto, ProxyType, ProxyUserCredentialsDto, ProxyUserStatus};
use crate::app::TargetUser;
use crate::{config_field_child, edit_field_bool, edit_field_date, edit_field_number, edit_field_text, edit_field_text_option};
use crate::app::components::select::Select;
use crate::app::components::{DropDownOption, TextButton, UserStatus};
use crate::app::components::userlist::proxy_type_input::ProxyTypeInput;

#[derive(Properties, PartialEq, Clone)]
pub struct ProxyUserCredentialsFormProps {
    pub user: Option<Rc<TargetUser>>,
    pub targets: Rc<Vec<Rc<ConfigTargetDto>>>,
    pub server: Rc<Vec<ApiProxyServerInfoDto>>,
}

#[function_component]
pub fn ProxyUserCredentialsForm(props: &ProxyUserCredentialsFormProps) -> Html {
    let translate = use_translation();
    let form_state = use_memo(props.user.clone(),
                              |user| RefCell::new(user.as_ref()
                                  .map_or_else(|| ProxyUserCredentialsDto::default(), |usr| usr.credentials.as_ref().clone())));

    let targets = use_memo((props.targets.clone(), props.user.clone()), |(targets, user)|
        targets.iter().map(|t| Rc::new(DropDownOption {
            id: t.name.to_string(),
            label: html! { t.name.clone() },
            selected: user.as_ref().is_some_and(|user| user.target == t.name),
        })).collect::<Vec<Rc<DropDownOption>>>(),
    );

    let server = use_memo((props.server.clone(), props.user.clone()), |(server, user)|
        server.iter().map(|s| Rc::new(DropDownOption {
            id: s.name.to_string(),
            label: html! { s.name.clone() },
            selected: user.as_ref().is_some_and(|user| user.credentials.server.as_ref() == Some(&s.name)),
        })).collect::<Vec<Rc<DropDownOption>>>(),
    );

    let proxy_user_status = use_memo(props.user.clone(), |user|
        vec![
            ProxyUserStatus::Active,
            ProxyUserStatus::Expired,
            ProxyUserStatus::Banned,
            ProxyUserStatus::Trial,
            ProxyUserStatus::Disabled,
            ProxyUserStatus::Pending,
        ].iter().map(|s| Rc::new(DropDownOption {
            id: s.to_string(),
            label: html! { <UserStatus status={Some(s.clone())} /> },
            selected: user.as_ref().is_some_and(|user| user.credentials.status.as_ref() == Some(s)),
        })).collect::<Vec<Rc<DropDownOption>>>(),
    );

    let handle_save_user = {
        let user = form_state.clone();
        Callback::from(move |_| {
            warn!("{:?}", user.borrow());
        })
    };

    html! {
        <div class="tp__proxy-user-credentials-form tp__form-page">
          <div class="tp__proxy-user-credentials-form__body tp__form-page__body">
            { config_field_child!(translate.t("LABEL.PLAYLIST"), {
               html! { <Select name="target"
                    multi_select={false}
                    onselect={Callback::from(move |(_name, selections):(String, Vec<Rc<DropDownOption>>)| {
                         warn!("{}", selections.iter().map(|o| o.id.to_string()).collect::<Vec<String>>().join(", "));
                    })}
                    options={(*targets).clone()}
                />
            }})}
            { config_field_child!(translate.t("LABEL.STATUS"), {
               html! { <Select name="status"
                    multi_select={false}
                    onselect={Callback::from(move |(_name, selections):(String, Vec<Rc<DropDownOption>>)| {
                        warn!("{}", selections.iter().map(|o| o.id.to_string()).collect::<Vec<String>>().join(", "));
                    })}
                    options={(*proxy_user_status).clone()}
                />
            }})}
            { edit_field_text!(*form_state,  translate.t("LABEL.USERNAME"), username) }
            { edit_field_text!(*form_state,  translate.t("LABEL.PASSWORD"), password, true) }
            { edit_field_text_option!(*form_state,  translate.t("LABEL.TOKEN"), token, true) }

            { config_field_child!(translate.t("LABEL.PROXY"), {
               html! {
                     <ProxyTypeInput value={props.user.as_ref().map_or_else(|| ProxyType::Reverse(None), |u| u.credentials.proxy)}
                        on_change={Callback::from(|proxy_type: ProxyType|
                        warn!("Proxy Type: {:?}", proxy_type)
                    )}/>
            }})}

            { config_field_child!(translate.t("LABEL.SERVER"), {
               html! {
                <Select name="server"
                    multi_select={false}
                    onselect={Callback::from(move |(_name, selections):(String, Vec<Rc<DropDownOption>>)| {
                         warn!("{}", selections.iter().map(|o| o.id.to_string()).collect::<Vec<String>>().join(", "));
                    })}
                    options={(*server).clone()}
                />
            }})}

            { edit_field_number!(*form_state,  translate.t("LABEL.MAX_CONNECTIONS"), max_connections) }
            { edit_field_date!(*form_state,  translate.t("LABEL.EXP_DATE"), exp_date) }
            { edit_field_text_option!(*form_state,  translate.t("LABEL.EPG_TIMESHIFT"), epg_timeshift) }

            { edit_field_bool!(*form_state,  translate.t("LABEL.USER_UI_ENABLED"), ui_enabled) }
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
