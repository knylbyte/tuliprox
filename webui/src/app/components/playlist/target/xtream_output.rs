use std::rc::Rc;
use yew::prelude::*;
use shared::model::XtreamTargetOutputDto;

#[derive(Properties, PartialEq, Clone)]
pub struct XtreamOutputProps {
   pub output: XtreamTargetOutputDto,
}

#[function_component]
pub fn XtreamOutput(props: &XtreamOutputProps) -> Html {

    html! {
      <div class="tp__xtream_output">
      </div>
    }
}

/*

    <div class="tp__tp__target_output_row">
                        <label>{translate.t("LABEL.SKIP_DIRECT_SOURCE")}</label>
                        <TagList>
                            <Tag style={if xc.skip_live_direct_source { "on" } else { "off "}} label={translate.t("LABEL.LIVE")}/>
                            <Tag style={if xc.skip_vod_direct_source { "on" } else { "off "}} label={translate.t("LABEL.VOD")}/>
                            <Tag style={if xc.skip_series_direct_source { "on" } else { "off "}} label={translate.t("LABEL.SERIES")}/>
                        </TagList>
     </div>
    <div class="tp__tp__target_output_row"><label>{translate.t("LABEL.RESOVE")}</label></div>

                        #[serde(default = "default_as_true")]
    pub skip_live_direct_source: bool,
    #[serde(default = "default_as_true")]
    pub skip_video_direct_source: bool,
    #[serde(default = "default_as_true")]
    pub skip_series_direct_source: bool,
    #[serde(default)]
    pub resolve_series: bool,
    #[serde(default = "default_resolve_delay_secs")]
    pub resolve_series_delay: u16,
    #[serde(default)]
    pub resolve_vod: bool,
    #[serde(default = "default_resolve_delay_secs")]
    pub resolve_vod_delay: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trakt: Option<TraktConfigDto>,
                                <span>{"xc"}</span>
                        },
 */