use crate::app::components::config::HasFormData;
use crate::app::components::{BlockInstance, Card, EditMode, Panel, SourceEditorContext, TextButton};
use crate::{ edit_field_bool,  edit_field_number_u16, edit_field_text_option, generate_form_reducer};
use shared::model::{ XtreamTargetOutputDto, TargetOutputDto};
use std::fmt::Display;
use std::rc::Rc;
use yew::{classes, function_component, html, use_context, use_effect_with, use_reducer, use_state, Callback, Html, Properties, UseReducerHandle};
use yew_i18n::use_translation;

const LABEL_SKIP_LIVE_DIRECT_SOURCE: &str = "LABEL.SKIP_LIVE_DIRECT_SOURCE";
const LABEL_SKIP_VOD_DIRECT_SOURCE: &str = "LABEL.SKIP_VOD_DIRECT_SOURCE";
const LABEL_SKIP_SERIES_DIRECT_SOURCE: &str = "LABEL.SKIP_SERIES_DIRECT_SOURCE";
const LABEL_RESOLVE_VOD: &str = "LABEL.RESOLVE_VOD";
const LABEL_RESOLVE_VOD_DELAY: &str = "LABEL.RESOLVE_VOD_DELAY_SEC";
const LABEL_RESOLVE_SERIES: &str = "LABEL.RESOLVE_SERIES";
const LABEL_RESOLVE_SERIES_DELAY: &str = "LABEL.RESOLVE_SERIES_DELAY_SEC";
const LABEL_FILTER: &str = "LABEL.FILTER";


#[derive(Copy, Clone, PartialEq, Eq)]
enum OutputFormPage {
    Main,
    Trakt,
}

impl Display for OutputFormPage {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", match *self {
            OutputFormPage::Main => "Main".to_string(),
            OutputFormPage::Trakt => "Trakt".to_string(),
        })
    }
}
//
// generate_form_reducer!(
//     state: TraktConfigFormState { form: TraktConfigDto },
//     action_name: TraktConfigFormAction,
//     fields {
//         IgnoreLogo => ignore_logo: bool,
//         ShareLiveStreams => share_live_streams: bool,
//         RemoveDuplicates => remove_duplicates: bool,
//         ForceRedirect => force_redirect: Option<ClusterFlags>,
//     }
// );

generate_form_reducer!(
    state: XtreamTargetOutputFormState { form: XtreamTargetOutputDto },
    action_name: XtreamTargetOutputFormAction,
    fields {
        SkipLiveDirectSource => skip_live_direct_source: bool,
        SkipVideoDirectSource => skip_video_direct_source: bool,
        SkipSeriesDirectSource =>  skip_series_direct_source: bool,
        ResolveSeries =>  resolve_series: bool,
        ResolveSeriesDelay =>  resolve_series_delay: u16,
        ResolveVod =>  resolve_vod: bool,
        ResolveVodDelay =>  resolve_vod_delay: u16,
        Filter => filter: Option<String>,
    }
);

#[derive(Properties, PartialEq, Clone)]
pub struct XtreamTargetOutputViewProps {
    pub(crate) block_id: usize,
    pub(crate) output: Option<Rc<XtreamTargetOutputDto>>,
}

#[function_component]
pub fn XtreamTargetOutputView(props: &XtreamTargetOutputViewProps) -> Html {
    let translate = use_translation();
    let source_editor_ctx = use_context::<SourceEditorContext>().expect("SourceEditorContext not found");

    let output_form_state: UseReducerHandle<XtreamTargetOutputFormState> =
        use_reducer(|| XtreamTargetOutputFormState {
            form: XtreamTargetOutputDto::default(),
            modified: false,
        });
    // let target_options_state: UseReducerHandle<TraktConfigFormState> =
    //     use_reducer(|| TraktConfigFormState {
    //         form: TraktConfigDto::default(),
    //         modified: false,
    //     });

    let view_visible = use_state(|| OutputFormPage::Main.to_string());

    let on_tab_click = {
        let view_visible = view_visible.clone();
        Callback::from(move |page: OutputFormPage| view_visible.set(page.to_string()))
    };

    {
        let output_form_state = output_form_state.clone();

        let config_output = props.output.clone();

        use_effect_with(config_output, move |cfg| {
            if let Some(target) = cfg {
                output_form_state.dispatch(XtreamTargetOutputFormAction::SetAll(target.as_ref().clone()));
            } else {
                output_form_state.dispatch(XtreamTargetOutputFormAction::SetAll(XtreamTargetOutputDto::default()));
            }
            || ()
        });
    }
    let render_output = || {
        html! {
            <Card class="tp__config-view__card">
                { edit_field_bool!(output_form_state, translate.t(LABEL_SKIP_LIVE_DIRECT_SOURCE), skip_live_direct_source,  XtreamTargetOutputFormAction::SkipLiveDirectSource) }
                { edit_field_bool!(output_form_state, translate.t(LABEL_SKIP_VOD_DIRECT_SOURCE), skip_video_direct_source,  XtreamTargetOutputFormAction::SkipVideoDirectSource) }
                { edit_field_bool!(output_form_state, translate.t(LABEL_SKIP_SERIES_DIRECT_SOURCE), skip_series_direct_source,  XtreamTargetOutputFormAction::SkipSeriesDirectSource) }
                { edit_field_bool!(output_form_state, translate.t(LABEL_RESOLVE_VOD), resolve_vod,  XtreamTargetOutputFormAction::ResolveVod) }
                { edit_field_bool!(output_form_state, translate.t(LABEL_RESOLVE_SERIES), resolve_series,  XtreamTargetOutputFormAction::ResolveSeries) }
                { edit_field_number_u16!(output_form_state, translate.t(LABEL_RESOLVE_VOD_DELAY), resolve_vod_delay,  XtreamTargetOutputFormAction::ResolveVodDelay) }
                { edit_field_number_u16!(output_form_state, translate.t(LABEL_RESOLVE_SERIES_DELAY), resolve_series_delay,  XtreamTargetOutputFormAction::ResolveSeriesDelay) }
                { edit_field_text_option!(output_form_state, translate.t(LABEL_FILTER), filter, XtreamTargetOutputFormAction::Filter) }
            </Card>
        }
    };


    let render_edit_mode = || {
        html! {
            <div class="tp__input-form__body">
                <div class="tp__tab-header">
                {
                    for [
                        OutputFormPage::Main,
                        OutputFormPage::Trakt,
                    ].iter().map(|page| {
                        let page_str = page.to_string();
                        let active = *view_visible == page_str;
                        let on_tab_click = {
                            let on_tab_click = on_tab_click.clone();
                            let page = *page;
                            Callback::from(move |_| on_tab_click.emit(page))
                        };
                        html! {
                            <button
                                class={classes!("tp__tab-button", if active { "active" } else { "" })}
                                onclick={on_tab_click}
                            >
                                { page_str.clone() }
                            </button>
                        }
                    })
                }
            </div>
            <div class="tp__input-form__body__pages">
                <Panel value={OutputFormPage::Main.to_string()} active={view_visible.to_string()}>
                {render_output()}
                </Panel>
                <Panel value={OutputFormPage::Trakt.to_string()} active={view_visible.to_string()}>
                    {"TODO"}
                </Panel>
            </div>
            </div>
        }
    };

    let handle_apply_target = {
        let source_editor_ctx = source_editor_ctx.clone();
        let output_form_state = output_form_state.clone();
        let block_id = props.block_id;
        Callback::from(move |_| {
            let output = output_form_state.data().clone();
            source_editor_ctx.on_form_change.emit((block_id, BlockInstance::Output(Rc::new(TargetOutputDto::Xtream(output)))));
            source_editor_ctx.edit_mode.set(EditMode::Inactive);
        })
    };
    let handle_cancel = {
        let source_editor_ctx = source_editor_ctx.clone();
        Callback::from(move |_| {
            source_editor_ctx.edit_mode.set(EditMode::Inactive);
        })
    };

    html! {
        <div class="tp__source-editor-form tp__config-view-page">
             <div class="tp__source-editor-form__toolbar tp__form-page__toolbar">
             <TextButton class="primary" name="apply_input"
                icon="Accept"
                title={ translate.t("LABEL.OK")}
                onclick={handle_apply_target}></TextButton>
             <TextButton class="secondary" name="cancel_input"
                icon="Cancel"
                title={ translate.t("LABEL.CANCEL")}
                onclick={handle_cancel}></TextButton>
          </div>
            { render_edit_mode() }
        </div>
        }
}
