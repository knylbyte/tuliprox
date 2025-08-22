use shared::model::{ClusterFlags, ProxyType};
use yew::prelude::*;
use yew_i18n::use_translation;

fn get_flags(pt: ProxyType)-> (bool, bool, bool, bool, bool) {
    match pt {
        ProxyType::Reverse(flags) => {
            let cluster = flags.unwrap_or_else(ClusterFlags::all);
            let live_flag = cluster.contains(ClusterFlags::Live);
            let vod_flag = cluster.contains(ClusterFlags::Vod);
            let series_flag = cluster.contains(ClusterFlags::Series);
            (false, true, live_flag, vod_flag, series_flag)
        }
        ProxyType::Redirect => (true, false, false, false, false),
    }
}

#[derive(Properties, Clone, PartialEq, Debug)]
pub struct ProxyTypeInputProps {
    pub value: ProxyType,
    #[prop_or_default]
    pub onchange: Callback<ProxyType>,
}

#[function_component]
pub fn ProxyTypeInput(props: &ProxyTypeInputProps) -> Html {
    let translate = use_translation();
    let selections = use_state(|| get_flags(props.value));

    {
        let set_selections = selections.clone();
        use_effect_with(props.value, move |pt| {
            set_selections.set(get_flags(*pt));
        });
    }

    let handle_change = {
      let onchange = props.onchange.clone();
      Callback::from(move |(redirect, _reverse, live, vod, series)| {
        if redirect {
            onchange.emit(ProxyType::Redirect);
        } else {
            let cluster_flags = if live && vod && series {
                None
            } else {
                let mut flags = ClusterFlags::empty();
                if live {
                    flags.insert(ClusterFlags::Live);
                }
                if vod {
                    flags.insert(ClusterFlags::Vod);
                }
                if series {
                    flags.insert(ClusterFlags::Series);
                }
                Some(flags)
            };
            onchange.emit(ProxyType::Reverse(cluster_flags));
        }
      })
    };

    let handle_redirect_click = {
        let set_selections = selections.clone();
        let emit_change = handle_change.clone();
        Callback::from(move |_| {
            let new_flags = (true, false, false, false, false);
            set_selections.set(new_flags);
            emit_change.emit(new_flags);
        })
    };
    let handle_reverse_click = {
        let set_selections = selections.clone();
        let emit_change = handle_change.clone();
        Callback::from(move |_| {
            let new_flags = (false, true, true, true, true);
            set_selections.set(new_flags);
            emit_change.emit(new_flags);
        })
    };
    let handle_reverse_live_click = {
        let set_selections = selections.clone();
        let emit_change = handle_change.clone();
        Callback::from(move |_| {
            let flags = *set_selections;
            let new_flags = if flags.0 {
                (false, true, true, true, true)
            } else {
               (false, true, !flags.2, flags.3, flags.4)
            };
            set_selections.set(new_flags);
            emit_change.emit(new_flags);
        })
    };
    let handle_reverse_vod_click = {
        let set_selections = selections.clone();
        let emit_change = handle_change.clone();
        Callback::from(move |_| {
            let flags = *set_selections;
            let new_flags = if flags.0 {
                (false, true, true, true, true)
            } else {
                (false, true, flags.2, !flags.3, flags.4)
            };
            set_selections.set(new_flags);
            emit_change.emit(new_flags);
        })
    };

    let handle_reverse_series_click = {
        let set_selections = selections.clone();
        let emit_change = handle_change.clone();
        Callback::from(move |_| {
            let flags = *set_selections;
            let new_flags = if flags.0 {
                (false, true, true, true, true)
            } else {
                (false, true, flags.2, flags.3, !flags.4)
            };
            set_selections.set(new_flags);
            emit_change.emit(new_flags);
        })
    };

    let (redirect, reverse, reverse_live, reverse_vod, reverse_series) = *selections;

    html! {
        <div class="tp__proxy-type-input">
          <span onclick={handle_redirect_click} class={classes!("tp__chip", "tp__proxy-type-input__redirect", if redirect {"active"} else {""})}>
            <span>{ translate.t("LABEL.REDIRECT") }</span>
          </span>

          <span class={classes!("tp__chip", "tp__chip__group", "tp__proxy-type-input__reverse" , if reverse {"active"} else {""})}>
            <span onclick={handle_reverse_click}>{ translate.t("LABEL.REVERSE") }</span>
            <span class={"tp__chip__group__sub tp__proxy-type-input__mixed"}>
                <span onclick={handle_reverse_live_click} class={classes!("tp__chip", "tp__proxy-type-input__reverse-live", if reverse_live {"active"} else { if reverse {"redirect-active"} else {""} })}>{ translate.t("LABEL.LIVE_SHORT") }</span>
                <span onclick={handle_reverse_vod_click} class={classes!("tp__chip", "tp__proxy-type-input__reverse-vod", if reverse_vod {"active"} else {if reverse {"redirect-active"} else {""} })}>{ translate.t("LABEL.VOD_SHORT") }</span>
                <span onclick={handle_reverse_series_click} class={classes!("tp__chip", "tp__proxy-type-input__reverse-series", if reverse_series {"active"} else {if reverse {"redirect-active"} else {""} })}>{ translate.t("LABEL.SERIES_SHORT") }</span>
            </span>
          </span>
        </div>
    }
}