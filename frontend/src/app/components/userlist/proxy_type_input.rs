use shared::model::{ProxyType};
use yew::prelude::*;
use yew_i18n::use_translation;

#[derive(Properties, Clone, PartialEq, Debug)]
pub struct ProxyTypeInputProps {
    pub value: ProxyType,
}

#[function_component]
pub fn ProxyTypeInput(props: &ProxyTypeInputProps) -> Html {
    let translate = use_translation();
    //
    // let render_chip = |flag: bool, class_sfx: &str,  label: &str, | -> Html {
    //     if flag {
    //         html! {
    //             <Chip class={ format!("tp__proxy-type__reverse tp__proxy-type__reverse-{} active", class_sfx) } label={translate.t(label)} />
    //         }
    //     } else {
    //         html!{
    //             <Chip class={ format!("tp__proxy-type__redirect tp__proxy-type__redirect-{}", class_sfx)} label={translate.t(label)} />
    //         }
    //     }
    // };
    //
    // match props.value {
    //     ProxyType::Reverse(flags) => {
    //         let cluster = flags.unwrap_or_else(ClusterFlags::all);
    //         let live_flag = cluster.contains(ClusterFlags::Live);
    //         let vod_flag = cluster.contains(ClusterFlags::Vod);
    //         let series_flag = cluster.contains(ClusterFlags::Series);
    //         if live_flag && vod_flag && series_flag {
    //             html! { <Chip label={translate.t("LABEL.REVERSE")} class={"tp__proxy-type__reverse"} /> }
    //         } else {
    //             html! {
    //             <div class="tp__proxy-type__mixed">
    //                { render_chip(live_flag, "live", "LABEL.LIVE_SHORT") }
    //                { render_chip(vod_flag, "vod", "LABEL.VOD_SHORT") }
    //                { render_chip(series_flag, "series", "LABEL.SERIES_SHORT" ) }
    //              </div>
    //             }
    //         }
    //     }
    //     ProxyType::Redirect => html! {
    //         <Chip label={translate.t("LABEL.REDIRECT")} class={"tp__proxy-type__redirect"} />
    //     },
    // }

    let handle_redirect_click = {
      Callback::from(move |_| {
          println!("redirect");
      })
    };

    html! {
        <div class="tp__proxy-type-input">
          <span class={"tp__chip"} onclick={handle_redirect_click}>
            <span class="tp__chip__label">{ translate.t("LABEL.REDIRECT") }</span>
          </span>

          <span class={"tp__chip"}>
            <span class="tp__chip__label">{ translate.t("LABEL.REVERSE") }</span>
            <span class={"tp__chip__group"}>
                <span class="tp__chip__label">{ translate.t("LABEL.LIVE") }</span>
                <span class="tp__chip__label">{ translate.t("LABEL.VIDEO") }</span>
                <span class="tp__chip__label">{ translate.t("LABEL.SERIES") }</span>
            </span>
          </span>
        </div>
    }
}