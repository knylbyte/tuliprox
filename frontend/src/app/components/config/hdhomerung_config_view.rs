use yew::prelude::*;
use yew_i18n::use_translation;
use shared::model::HdHomeRunDeviceConfigDto;
use crate::app::components::{Card, NoContent};
use crate::app::context::ConfigContext;
use crate::{config_field, config_field_bool, config_field_bool_empty};

#[function_component]
pub fn HdHomerunConfigView() -> Html {
    let translate = use_translation();
    let config_ctx = use_context::<ConfigContext>().expect("ConfigContext not found");

    let render_devices = |devices: &Vec<HdHomeRunDeviceConfigDto>| -> Html {
        html!{ for devices.iter().map(|entry| html! {
            <Card class = "tp__config-view__card" >
                <h1> { translate.t("LABEL.DEVICE") } </h1>
                {config_field!(entry, translate.t("LABEL.FRIENDLY_NAME"), friendly_name)}
                {config_field!(entry, translate.t("LABEL.MANUFACTURER"), manufacturer)}
                {config_field!(entry, translate.t("LABEL.MODEL_NAME"), model_name)}
                {config_field!(entry, translate.t("LABEL.MODEL_NUMBER"), model_number)}
                {config_field!(entry, translate.t("LABEL.FIRMWARE_NAME"), firmware_name)}
                {config_field!(entry, translate.t("LABEL.FIRMWARE_VERSION"), firmware_version)}
                {config_field!(entry, translate.t("LABEL.DEVICE_TYPE"), device_type)}
                {config_field!(entry, translate.t("LABEL.DEVICE_UDN"), device_udn)}
                {config_field!(entry, translate.t("LABEL.NAME"), name)}
                {config_field!(entry, translate.t("LABEL.PORT"), port)}
                {config_field!(entry, translate.t("LABEL.TUNER_COUNT"), tuner_count)}
            </Card>
        })}
    };

    let render_empty = || {
        html! {
          <>
            <div class="tp__hdhomerun-config-view__header tp__config-view-page__header">
                { config_field_bool_empty!(translate.t("LABEL.ENABLED")) }
                { config_field_bool_empty!(translate.t("LABEL.DEVICE_AUTH")) }
            </div>
            <div class="tp__hdhomerun-config-view__body tp__config-view-page__body">
                <Card class="tp__config-view__card">
                  <h1>{translate.t("LABEL.DEVICES")}</h1>
                  <NoContent />
                </Card>
            </div>
          </>
        }
    };


    html! {
        <div class="tp__hdhomerun-config-view tp__config-view-page">
            {
                if let Some(config) = &config_ctx.config {
                    if let Some(hdhomerun) = &config.config.hdhomerun {
                        html! {
                        <>
                        <div class="hdhomerun-config-view__body tp__config-view-page__header">
                          { config_field_bool!(hdhomerun, translate.t("LABEL.ENABLED"), enabled) }
                          { config_field_bool!(hdhomerun, translate.t("LABEL.DEVICE_AUTH"), auth) }
                        </div>
                        <div class="hdhomerun-config-view__body tp__config-view-page__body">
                          {render_devices(&hdhomerun.devices)}
                        </div>
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
    }
}