use crate::app::context::ConfigContext;
use yew::prelude::*;
use yew_i18n::use_translation;
use shared::model::{ConfigApiDto};
use crate::{config_field, config_field_empty, edit_field_number_u16, edit_field_text, generate_form_reducer, html_if};
use crate::app::components::AppIcon;
use crate::app::components::config::config_view_context::ConfigViewContext;
use crate::app::components::config::config_page::ConfigForm;

const LABEL_HOST: &str = "LABEL.HOST";
const LABEL_PORT: &str = "LABEL.PORT";
const LABEL_WEB_ROOT: &str = "LABEL.WEB_ROOT";

// Generate form reducer for edit mode
generate_form_reducer!(
    state: ApiConfigFormState { form: ConfigApiDto },
    action_name: ApiConfigFormAction,
    fields {
        Host => host: String,
        Port => port: u16,
        WebRoot => web_root: String,
    }
);

#[function_component]
pub fn ApiConfigView() -> Html {
    let translate = use_translation();
    let config_ctx = use_context::<ConfigContext>().expect("Config context not found");
    let config_view_ctx = use_context::<ConfigViewContext>().expect("ConfigViewContext not found");

    let form_state: UseReducerHandle<ApiConfigFormState> = use_reducer(|| {
        ApiConfigFormState { form: ConfigApiDto::default(), modified: false }
    });

    {
        let on_form_change = config_view_ctx.on_form_change.clone();
        let deps = (form_state.clone(), form_state.modified);
        use_effect_with(deps, move |(state, modified)| {
            on_form_change.emit(ConfigForm::Api(*modified, state.form.clone()));
            || ()
        });
    }

    {
        let form_state = form_state.clone();
        let api_config = config_ctx
            .config
            .as_ref()
            .map(|c| c.config.api.clone());

        let deps = (api_config, *config_view_ctx.edit_mode);
        use_effect_with(deps, move |(cfg, _mode)| {
            if let Some(api) = cfg {
                form_state.dispatch(ApiConfigFormAction::SetAll(api.clone()));
            } else {
                form_state.dispatch(ApiConfigFormAction::SetAll(ConfigApiDto::default()));
            }
            || ()
        });

    }

    let render_empty = || {
        html! {
            <>
                { config_field_empty!(translate.t(LABEL_HOST)) }
                { config_field_empty!(translate.t(LABEL_PORT)) }
                { config_field_empty!(translate.t(LABEL_WEB_ROOT)) }
            </>
        }
    };

    let render_view_mode = || {
        if let Some(config) = &config_ctx.config {
            html! {
                <>
                    { config_field!(config.config.api, translate.t(LABEL_HOST), host) }
                    { config_field!(config.config.api, translate.t(LABEL_PORT), port) }
                    { config_field!(config.config.api, translate.t(LABEL_WEB_ROOT), web_root) }
                </>
            }
        } else {
            render_empty()
        }
    };

    let render_edit_mode = || {
        html! {
            <>
                { edit_field_text!(form_state, translate.t(LABEL_HOST), host, ApiConfigFormAction::Host) }
                { edit_field_number_u16!(form_state, translate.t(LABEL_PORT), port, ApiConfigFormAction::Port) }
                { edit_field_text!(form_state, translate.t(LABEL_WEB_ROOT), web_root, ApiConfigFormAction::WebRoot) }
            </>
        }
    };

    html! {
        <div class="tp__api-config-view tp__config-view-page">
            {
             html_if!(*config_view_ctx.edit_mode, {
                  <div class="tp__webui-config-view__info tp__config-view-page__info">
                    <AppIcon name="Warn"/> <span class="info">{translate.t("INFO.RESTART_TO_APPLY_CHANGES")}</span>
                  </div>
            })}
            <div class="tp__api-config-view__body tp__config-view-page__body">
                {
                    if *config_view_ctx.edit_mode {
                        render_edit_mode()
                    } else {
                        render_view_mode()
                    }
                }
            </div>
        </div>
    }
}
