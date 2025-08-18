use crate::app::components::{Card, Chip};
use crate::app::context::ConfigContext;
use crate::{config_field_bool, config_field_child, config_field_empty, config_field_optional};
use shared::model::VideoDownloadConfigDto;
use yew::prelude::*;
use yew_i18n::use_translation;

const LABEL_DOWNLOAD: &str = "LABEL.DOWNLOAD";
const LABEL_ORGANIZE_INTO_DIRECTORIES: &str = "LABEL.ORGANIZE_INTO_DIRECTORIES";
const LABEL_DIRECTORY: &str = "LABEL.DIRECTORY";
const LABEL_EPISODE_PATTERN: &str = "LABEL.EPISODE_PATTERN";
const LABEL_HEADERS: &str = "LABEL.HEADERS";
const LABEL_EXTENSIONS: &str = "LABEL.EXTENSIONS";
const LABEL_WEB_SEARCH: &str = "LABEL.WEB_SEARCH";

#[function_component]
pub fn VideoConfigView() -> Html {
    let translate = use_translation();
    let config_ctx = use_context::<ConfigContext>().expect("ConfigContext not found");

    let render_extensions = |extensions: &Vec<String>| html! {
        <Card>
        { config_field_child!(translate.t(LABEL_EXTENSIONS), {
           html! {
             <div class="tp__config-view__tags">
             {
                html! { for extensions.iter().map(|t| html! { <Chip label={t.clone()} /> }) }
             }
             </div>
            }})}
        </Card>
    };

    let render_download = |download: Option<&VideoDownloadConfigDto>| html! {
        match download {
            Some(entry) => html! {
            <Card class="tp__config-view__card">
                <h1>{translate.t(LABEL_DOWNLOAD)}</h1>
                { config_field_bool!(entry, translate.t(LABEL_ORGANIZE_INTO_DIRECTORIES), organize_into_directories) }
                { config_field_optional!(entry, translate.t(LABEL_DIRECTORY), directory) }
                { config_field_optional!(entry, translate.t(LABEL_EPISODE_PATTERN), episode_pattern) }
                { config_field_child!(translate.t(LABEL_HEADERS), {
                    html! {
                        <div class="tp__config-view__tags">
                          <ul>
                            {
                                if entry.headers.is_empty() {
                                    html! {}
                                } else {
                                    html! {
                                       for entry.headers.iter().map(|(key, value)| html! {
                                          <li>{key}{":"} {value}</li>
                                       })
                                    }
                                }
                            }
                          </ul>
                        </div>
                    }
                })}
            </Card>
            },
            None => html! {
            <Card class="tp__config-view__card">
                <h1>{translate.t(LABEL_DOWNLOAD)}</h1>
                { config_field_empty!(translate.t(LABEL_ORGANIZE_INTO_DIRECTORIES)) }
                { config_field_empty!(translate.t(LABEL_DIRECTORY)) }
                { config_field_empty!(translate.t(LABEL_EPISODE_PATTERN)) }
                { config_field_empty!(translate.t(LABEL_HEADERS)) }
            </Card>
          },
        }
    };

    let render_empty = || {
        html! {
         <>
          <div class="tp__video-config-config-view__body tp__config-view-page__header">
            { config_field_empty!(translate.t(LABEL_WEB_SEARCH)) }
          </div>
          <div class="tp__video-config-config-view__body tp__config-view-page__body">
            { config_field_empty!(translate.t(LABEL_EXTENSIONS)) }
            { render_download(None) }
          </div>
         </>
        }
    };

    html! {
        <div class="tp__video-config-view tp__config-view-page">
            {
                if let Some(config) = &config_ctx.config {
                    if let Some(video) = &config.config.video {
                        html! {
                          <>
                            <div class="tp__video-config-view__body tp__config-view-page__body">
                              { config_field_optional!(video, translate.t(LABEL_WEB_SEARCH), web_search) }
                            </div>
                            <div class="tp__video-config-view__body tp__config-view-page__body">
                            { render_extensions(&video.extensions) }
                            { render_download(video.download.as_ref()) }
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