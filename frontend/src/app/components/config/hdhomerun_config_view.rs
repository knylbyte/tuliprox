use yew::prelude::*;
use yew_i18n::use_translation;
use shared::model::{HdHomeRunDeviceConfigDto, HdHomeRunConfigDto};
use crate::app::components::{Card, NoContent, TextButton};
use crate::app::context::ConfigContext;
use crate::{config_field_bool, edit_field_bool, generate_form_reducer, html_if};
use crate::app::components::config::config_page::ConfigForm;
use crate::app::components::config::hdhomerun_device_view::{HdHomerunDeviceView};
use crate::app::components::config::config_view_context::ConfigViewContext;
use crate::app::components::config::HasFormData;

generate_form_reducer!(
    state: HdHomeRunConfigFormState { form: HdHomeRunConfigDto },
    action_name: HdHomeRunConfigFormAction,
    fields {
        Enabled => enabled: bool,
        Auth => auth: bool,
        Devices => devices: Vec<HdHomeRunDeviceConfigDto>,
    }
);

#[function_component]
pub fn HdHomerunConfigView() -> Html {
    let translate = use_translation();
    let config_ctx = use_context::<ConfigContext>().expect("ConfigContext not found");
    let config_view_ctx = use_context::<ConfigViewContext>().expect("ConfigViewContext not found");

    let form_state: UseReducerHandle<HdHomeRunConfigFormState> = use_reducer(|| {
        HdHomeRunConfigFormState { form: HdHomeRunConfigDto::default(), modified: false }
    });

    {
        let on_form_change = config_view_ctx.on_form_change.clone();
        use_effect_with(form_state.clone(), move |state| {
            on_form_change.emit(ConfigForm::HdHomerun(state.modified, state.form.clone()));
        });
    }

    {
        let form_state = form_state.clone();
        let hdhr_config = config_ctx
            .config
            .as_ref()
            .and_then(|c| c.config.hdhomerun.clone());

        use_effect_with((hdhr_config, config_view_ctx.edit_mode.clone()), move |(hdhr_cfg, _mode)| {
            if let Some(hdhr) = hdhr_cfg {
                form_state.dispatch(HdHomeRunConfigFormAction::SetAll((*hdhr).clone()));
            } else {
                form_state.dispatch(HdHomeRunConfigFormAction::SetAll(HdHomeRunConfigDto::default()));
            }
            || ()
        });
    }

    let handle_add_device = {
        let form_state = form_state.clone();
        Callback::from(move |_| {
            let mut new_state = form_state.data().clone();
            let mut new_device = HdHomeRunDeviceConfigDto::default();
            new_device.prepare(new_state.devices.len() as u8);
            new_state.devices.push(new_device);
            form_state.dispatch(HdHomeRunConfigFormAction::SetAll(new_state));
        })
    };

    let render_empty = || {
        html! {
            <div class="tp__hdhomerun-config-view__body tp__config-view-page__body">
                <Card class="tp__config-view__card">
                  <h1>{translate.t("LABEL.DEVICES")}</h1>
                  <NoContent />
                </Card>
            </div>
        }
    };

    let edit_mode = *config_view_ctx.edit_mode.clone();

    let render_devices = |devices: &Vec<HdHomeRunDeviceConfigDto>| -> Html {
        if devices.is_empty() {
            render_empty()
        } else {
            html!{ for devices.iter().map(|entry| html! {
                <HdHomerunDeviceView device={entry.clone()} edit_mode={edit_mode} />
            })}
        }
    };

    let hdhomerun = form_state.data();
    html! {
        <div class="tp__hdhomerun-config-view tp__config-view-page">
            <div class="hdhomerun-config-view__body tp__config-view-page__header">
              {if  edit_mode {
                 html! {
                 <>
                 { edit_field_bool!(form_state, translate.t("LABEL.ENABLED"), enabled, HdHomeRunConfigFormAction::Enabled) }
                 { edit_field_bool!(form_state, translate.t("LABEL.DEVICE_AUTH"), auth, HdHomeRunConfigFormAction::Auth) }
                 </>
                }
              } else {
                html! {
                  <>
                  { config_field_bool!(hdhomerun, translate.t("LABEL.ENABLED"), enabled) }
                  { config_field_bool!(hdhomerun, translate.t("LABEL.DEVICE_AUTH"), auth) }
                  </>
                }
              }
              }
            </div>
            {html_if!(edit_mode, {
                <div class="tp__hdhomerun-config-view__form-action">
                    <TextButton class="primary" name="add_hdhomerun_device" title={ translate.t("LABEL.ADD_DEVICE")} onclick={handle_add_device}></TextButton>
                </div>
            })}
            <div class="hdhomerun-config-view__body tp__config-view-page__body">
              {render_devices(&hdhomerun.devices)}
            </div>
        </div>
    }
}