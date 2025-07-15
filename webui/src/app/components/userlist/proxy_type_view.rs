use log::info;
use crate::app::components::{convert_bool_to_chip_style, Chip};
use shared::model::{ClusterFlags, ProxyType};
use yew::prelude::*;
use yew_i18n::use_translation;


#[derive(Properties, Clone, PartialEq, Debug)]
pub struct ProxyTypeViewProps {
    pub value: ProxyType,
}

#[function_component]
pub fn ProxyTypeView(props: &ProxyTypeViewProps) -> Html {
    let translate = use_translation();
    match props.value {
        ProxyType::Reverse(flags) => {
            let cluster = flags.unwrap_or_else(ClusterFlags::all);
            info!("{cluster:?}");
            html! {
                <>
                    <Chip class={ format!("tp__proxy-type__reverse-live {}", convert_bool_to_chip_style(cluster.contains(ClusterFlags::Live)).unwrap_or_default())} label={translate.t("LABEL.LIVE")} />
                    <Chip class={ format!("tp__proxy-type__reverse-vod {}", convert_bool_to_chip_style(cluster.contains(ClusterFlags::Vod)).unwrap_or_default())} label={translate.t("LABEL.VOD")} />
                    <Chip class={format!("tp__proxy-type__reverse-series {}", convert_bool_to_chip_style(cluster.contains(ClusterFlags::Series)).unwrap_or_default())}  label={translate.t("LABEL.SERIES")} />
                </>
             }
        }
        ProxyType::Redirect => html! {
            <Chip label={translate.t("LABEL.REDIRECT")} class={"tp__proxy-type__redirect"} />
        },
    }
}