use std::rc::Rc;
use chrono::{Duration, Utc};
use yew::prelude::*;
use yew_i18n::use_translation;
use shared::model::{ApiProxyServerInfoDto, ConfigTargetDto, ProxyType, ProxyUserCredentialsDto, ProxyUserStatus};
use shared::utils::generate_random_string;
use crate::app::TargetUser;
use crate::{config_field_child, config_field_custom, edit_field_bool, edit_field_date, edit_field_number, edit_field_text, edit_field_text_option, generate_form_reducer};
use crate::app::components::select::Select;
use crate::app::components::{DropDownOption, TextButton, UserStatus};
use crate::app::components::config::HasFormData;
use crate::app::components::userlist::proxy_type_input::ProxyTypeInput;
use crate::hooks::use_service_context;

const DEFAULT_MAX_CONNECTIONS: u32 = 1;
const DEFAULT_EXPIRATION_DAYS: i64 = 365;

generate_form_reducer!(
    state: UserFormState { form: ProxyUserCredentialsDto },
    action_name: UserFormAction,
    fields {
        Username => username: String,
        Password => password: String,
        Token => token: Option<String>,
        Proxy => proxy: ProxyType,
        Server => server: Option<String>,
        Status => status: Option<ProxyUserStatus>,
        MaxConnections => max_connections: u32,
        ExpDate => exp_date: Option<i64>,
        UiEnabled => ui_enabled: bool,
        EpgTimeshift => epg_timeshift: Option<String>,
        Comment => comment: Option<String>,
    }
);

#[derive(Properties, PartialEq, Clone)]
pub struct ProxyUserCredentialsFormProps {
    pub user: Option<Rc<TargetUser>>,
    pub targets: Rc<Vec<Rc<ConfigTargetDto>>>,
    pub server: Rc<Vec<ApiProxyServerInfoDto>>,
    pub on_save: Callback<(bool, String, ProxyUserCredentialsDto)>,
}

#[function_component]
pub fn ProxyUserCredentialsForm(props: &ProxyUserCredentialsFormProps) -> Html {
    let translate = use_translation();
    let service_ctx = use_service_context();
    let selected_target = use_state(|| None);
    let update = use_state(|| false);

    let form_state: UseReducerHandle<UserFormState> = use_reducer(|| {
        UserFormState { form: ProxyUserCredentialsDto::default(), modified: false }
    });

    let proxy_user_status = use_memo(form_state.data().status, |status|
        [ProxyUserStatus::Active,
            ProxyUserStatus::Expired,
            ProxyUserStatus::Banned,
            ProxyUserStatus::Trial,
            ProxyUserStatus::Disabled,
            ProxyUserStatus::Pending].iter().map(|s| Rc::new(DropDownOption {
            id: s.to_string(),
            label: html! { <UserStatus status={Some(*s)} /> },
            selected: status.as_ref() == Some(s),
        })).collect::<Vec<Rc<DropDownOption>>>(),
    );

    let targets = use_memo((props.targets.clone(), (*selected_target).clone()),
                           |(targets, selected)|
        targets.iter().map(|t| Rc::new(DropDownOption {
            id: t.name.clone(),
            label: html! { t.name.clone() },
            selected: selected.as_ref().is_some_and(|ut: &String| ut == &t.name),
        })).collect::<Vec<Rc<DropDownOption>>>(),
    );

    let server = use_memo((props.server.clone(), form_state.data().server.clone()),
                          |(server_list, user_server)|
        server_list.iter().map(|s| Rc::new(DropDownOption {
            id: s.name.to_string(),
            label: html! { s.name.clone() },
            selected: user_server.as_ref() == Some(&s.name),
        })).collect::<Vec<Rc<DropDownOption>>>(),
    );

    {
        let form_state = form_state.clone();
        let set_selected_target = selected_target.clone();
        let set_update = update.clone();
        use_effect_with((props.user.clone(), props.server.clone()), move |(user, server)| {
            if let Some(u) = user.clone() {
                set_update.set(true);
                set_selected_target.set(Some(u.target.clone()));
                form_state.dispatch(UserFormAction::SetAll((*u.credentials).clone()));
            } else {
                set_update.set(false);
                set_selected_target.set(None);
                let mut user = ProxyUserCredentialsDto::default();
                if let Some(api_server) = (*server).first() {
                    user.server = Some(api_server.name.clone());
                }
                user.max_connections = DEFAULT_MAX_CONNECTIONS;
                user.proxy = ProxyType::Redirect;
                user.status = Some(ProxyUserStatus::Active);
                user.ui_enabled = true;
                let now = Utc::now();
                user.created_at = Some(now.timestamp());
                let in_one_year = now + Duration::days(DEFAULT_EXPIRATION_DAYS);
                user.exp_date = Some(in_one_year.timestamp());
                user.token = Some(generate_random_string(6));

                form_state.dispatch(UserFormAction::SetAll(user));
            }
            || ()
        },
        );
    }

    let handle_save_user = {
        let user = form_state.clone();
        let original = props.user.clone();
        let services = service_ctx.clone();
        let translate_clone = translate.clone();
        let target = selected_target.clone();
        let onsave = props.on_save.clone();
        let is_update = update.clone();
        Callback::from(move |_| {
            if let Some(target_name) = (*target).as_ref().cloned() {
                if user.modified() {
                    let user = user.data();
                    if let Err(err) = user.validate() {
                        services.toastr.error(err.to_string());
                    } else {
                        match original.as_ref().map(|t| t.credentials.clone()) {
                            None => onsave.emit((*is_update, target_name, user.clone())),
                            Some(original_user) => {
                                if &(*original_user) != user {
                                    onsave.emit((*is_update, target_name, user.clone()));
                                }
                            }
                        };
                    }
                } else {
                    services.toastr.warning(translate_clone.t("MESSAGES.SAVE.USER.NOTHING_TO_SAVE"));
                }
            } else {
                services.toastr.error(translate_clone.t("MESSAGES.SAVE.USER.TARGET_NOT_SELECTED"));
            }
        })
    };

    let set_selected_target = selected_target.clone();
    let server_list = server.clone();
    let instance_status = form_state.clone();
    let instance_proxy = form_state.clone();
    let instance_server = form_state.clone();
    html! {
        <div class="tp__proxy-user-credentials-form tp__form-page">
          <div class="tp__proxy-user-credentials-form__body tp__form-page__body">
            { if *update {
                 config_field_custom!(translate.t("LABEL.PLAYLIST"), (*set_selected_target).as_ref().map_or_else(String::new, |t| t.clone()))
               } else { config_field_child!(translate.t("LABEL.PLAYLIST"), {
               html! { <Select name="target"
                    multi_select={false}
                    onselect={Callback::from(move |(_name, selections):(String, Vec<Rc<DropDownOption>>)| {
                        if let Some(target_option) =  selections.first() {
                            set_selected_target.set(Some(target_option.id.clone()));
                        } else {
                            set_selected_target.set(None);
                        }
                    })}
                    options={(*targets).clone()}
                />
            }})}}
            { config_field_child!(translate.t("LABEL.STATUS"), {
               html! { <Select name="status"
                    multi_select={false}
                    onselect={Callback::from(move |(_name, selections):(String, Vec<Rc<DropDownOption>>)| {
                        if let Some(status_option) =  selections.first() {
                            if let Ok(status) = status_option.id.parse::<ProxyUserStatus>() {
                                instance_status.dispatch(UserFormAction::Status(Some(status)));
                            }
                        }
                    })}
                    options={(*proxy_user_status).clone()}
                />
            }})}
            { if *update {
                  config_field_custom!(translate.t("LABEL.USERNAME"), form_state.data().username.clone())
                } else {
                  edit_field_text!(form_state, translate.t("LABEL.USERNAME"), username, UserFormAction::Username)
               }
            }
            { edit_field_text!(form_state, translate.t("LABEL.PASSWORD"), password, UserFormAction::Password, true) }
            { edit_field_text_option!(form_state,  translate.t("LABEL.TOKEN"), token, UserFormAction::Token, true) }
            { config_field_child!(translate.t("LABEL.PROXY"), {
               html! {
                     <ProxyTypeInput value={form_state.data().proxy}
                        on_change={Callback::from(move |proxy_type: ProxyType| {
                         instance_proxy.dispatch(UserFormAction::Proxy(proxy_type));
                        }
                    )}/>
            }})}
            { config_field_child!(translate.t("LABEL.SERVER"), {
               html! {
                <Select name="server"
                    multi_select={false}
                    onselect={Callback::from(move |(_name, selections):(String, Vec<Rc<DropDownOption>>)| {
                        if let Some(server_option) =  selections.first() {
                            instance_server.dispatch(UserFormAction::Server(Some(server_option.id.clone())));
                        } else {
                            instance_server.dispatch(UserFormAction::Server(None));
                        };
                    })}
                    options={(*server_list).clone()}
                />
            }})}
            { edit_field_number!(form_state,  translate.t("LABEL.MAX_CONNECTIONS"), max_connections, UserFormAction::MaxConnections) }
            { edit_field_date!(form_state,  translate.t("LABEL.EXP_DATE"), exp_date, UserFormAction::ExpDate) }
            { edit_field_text_option!(form_state,  translate.t("LABEL.EPG_TIMESHIFT"), epg_timeshift, UserFormAction::EpgTimeshift) }
            { edit_field_bool!(form_state,  translate.t("LABEL.USER_UI_ENABLED"), ui_enabled, UserFormAction::UiEnabled) }
            { edit_field_text_option!(form_state,  translate.t("LABEL.COMMENT"), comment, UserFormAction::Comment) }

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