use yew::prelude::*;
use yew_i18n::use_translation;
use shared::model::HdHomeRunDeviceConfigDto;
use crate::app::components::{Card};
use crate::{config_field};

#[derive(Properties, PartialEq)]
pub struct HdHomerunDeviceViewProps {
    pub device: HdHomeRunDeviceConfigDto,
    #[prop_or(false)]
    pub edit_mode: bool,
}

#[function_component]
pub fn HdHomerunDeviceView(props: &HdHomerunDeviceViewProps) -> Html {
    let translate = use_translation();

    let device = use_state(HdHomeRunDeviceConfigDto::default);

    {
        let set_device = device.clone();
        use_effect_with(props.device.clone(), move |device| {
            set_device.set(device.clone());
        })
    }

    let render_device = |device: &HdHomeRunDeviceConfigDto| -> Html {
        html! {
            <Card class = "tp__config-view__card" >
                <h1> { translate.t("LABEL.DEVICE") } </h1>
                {config_field!(device, translate.t("LABEL.FRIENDLY_NAME"), friendly_name)}
                {config_field!(device, translate.t("LABEL.MANUFACTURER"), manufacturer)}
                {config_field!(device, translate.t("LABEL.MODEL_NAME"), model_name)}
                {config_field!(device, translate.t("LABEL.MODEL_NUMBER"), model_number)}
                {config_field!(device, translate.t("LABEL.FIRMWARE_NAME"), firmware_name)}
                {config_field!(device, translate.t("LABEL.FIRMWARE_VERSION"), firmware_version)}
                {config_field!(device, translate.t("LABEL.DEVICE_TYPE"), device_type)}
                {config_field!(device, translate.t("LABEL.DEVICE_UDN"), device_udn)}
                {config_field!(device, translate.t("LABEL.NAME"), name)}
                {config_field!(device, translate.t("LABEL.PORT"), port)}
                {config_field!(device, translate.t("LABEL.TUNER_COUNT"), tuner_count)}
            </Card>
        }
    };

    render_device(&device)
}