use crate::app::components::{Card, Chip};
use crate::app::context::ConfigContext;
use crate::{
    config_field, config_field_bool, config_field_bool_empty, config_field_child,
    config_field_empty, config_field_hide, config_field_optional,
};
use yew::prelude::*;
use yew_i18n::use_translation;

const LABEL_AUTH: &str = "LABEL.AUTH";
const LABEL_ENABLED: &str = "LABEL.ENABLED";
const LABEL_ISSUER: &str = "LABEL.ISSUER";
const LABEL_SECRET: &str = "LABEL.SECRET";
const LABEL_TOKEN_TTL_MINS: &str = "LABEL.TOKEN_TTL_MINS";
const LABEL_USERFILE: &str = "LABEL.USERFILE";
const LABEL_PLAYER_SERVER: &str = "LABEL.PLAYER_SERVER";
const LABEL_USER_UI_ENABLED: &str = "LABEL.USER_UI_ENABLED";
const LABEL_CONTENT_SECURITY_POLICY: &str = "LABEL.CONTENT_SECURITY_POLICY";
const LABEL_CONTENT_SECURITY_POLICY_CUSTOM_ATTRIBUTES: &str = "LABEL.CUSTOM_ATTRIBUTES";
const LABEL_PATH: &str = "LABEL.PATH";

#[function_component]
pub fn WebUiConfigView() -> Html {
    let translate = use_translation();
    let config_ctx = use_context::<ConfigContext>().expect("ConfigContext not found");

    let render_empty_auth = || {
        html! {
        <Card>
            <h1>{translate.t(LABEL_AUTH)}</h1>
                { config_field_empty!(translate.t(LABEL_ENABLED)) }
                { config_field_empty!(translate.t(LABEL_ISSUER)) }
                { config_field_empty!(translate.t(LABEL_SECRET)) }
                { config_field_empty!(translate.t(LABEL_TOKEN_TTL_MINS)) }
                { config_field_empty!(translate.t(LABEL_USERFILE)) }
        </Card>
        }
    };

    let render_empty = || {
        html! {
           <>
            { config_field_bool_empty!(translate.t(LABEL_ENABLED)) }
            { config_field_bool_empty!(translate.t(LABEL_USER_UI_ENABLED)) }
            { config_field_child!(translate.t(LABEL_CONTENT_SECURITY_POLICY), {
                html! {
                    <>
                        { config_field_bool_empty!(translate.t(LABEL_ENABLED)) }
                        { config_field_empty!(translate.t(LABEL_CONTENT_SECURITY_POLICY_CUSTOM_ATTRIBUTES)) }
                    </>
                }
            }) }
            { config_field_empty!(translate.t(LABEL_PATH)) }
            { config_field_empty!(translate.t(LABEL_PLAYER_SERVER)) }
            { render_empty_auth() }
           </>
        }
    };

    html! {
        <div class="tp__webui-config-view tp__config-view-page">
            <div class="tp__webui-config-config-view__body tp__config-view-page__body">
            {
                if let Some(config) = &config_ctx.config {
                    if let Some(web_ui) = &config.config.web_ui {
                        html! {
                        <>
                            { config_field_bool!(web_ui, translate.t(LABEL_ENABLED), enabled) }
                            { config_field_bool!(web_ui, translate.t(LABEL_USER_UI_ENABLED), user_ui_enabled) }
                            { config_field_child!(translate.t(LABEL_CONTENT_SECURITY_POLICY), {
                                html! {
                                    match web_ui.content_security_policy.as_ref() {
                                        Some(csp) => html! {
                                            <>
                                                { config_field_bool!(csp, translate.t(LABEL_ENABLED), enabled) }
                                                { config_field_child!(translate.t(LABEL_CONTENT_SECURITY_POLICY_CUSTOM_ATTRIBUTES), {
                                                    html! {
                                                        <div class="tp__config-view__tags">
                                                            { for csp.custom_attributes.iter().map(|a| html! { <Chip label={a.clone()} /> }) }
                                                        </div>
                                                    }
                                                }) }
                                            </>
                                        },
                                        None => html! {
                                            <>
                                                { config_field_bool_empty!(translate.t(LABEL_ENABLED)) }
                                                { config_field_empty!(translate.t(LABEL_CONTENT_SECURITY_POLICY_CUSTOM_ATTRIBUTES)) }
                                            </>
                                        }
                                    }
                                }
                            }) }
                            { config_field_optional!(web_ui, translate.t(LABEL_PATH), path) }
                            { config_field_optional!(web_ui, translate.t(LABEL_PLAYER_SERVER), player_server) }
                            <Card>
                              <h1>{translate.t(LABEL_AUTH)}</h1>
                              {
                                match web_ui.auth.as_ref() {
                                    Some(auth) => html!{
                                        <>
                                        { config_field_bool!(auth, translate.t(LABEL_ENABLED), enabled) }
                                        { config_field!(auth, translate.t(LABEL_ISSUER), issuer) }
                                        { config_field_hide!(auth, translate.t(LABEL_SECRET), secret) }
                                        { config_field!(auth, translate.t(LABEL_TOKEN_TTL_MINS), token_ttl_mins) }
                                        { config_field_optional!(auth, translate.t(LABEL_USERFILE), userfile) }
                                        </>
                                    },
                                    None => render_empty_auth(),
                                }}
                            </Card>
                        </>
                        }
                    } else {
                         { render_empty() }
                    }
                } else {
                     { render_empty() }
                }
            }
        </div>
        </div>
    }
}
