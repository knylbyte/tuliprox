use crate::app::components::config::HasFormData;
use crate::app::components::select::Select;
use crate::app::components::{BlockId, BlockInstance, Card, DropDownOption, DropDownSelection, EditMode, Panel, SourceEditorContext, TextButton};
use crate::{config_field_child, edit_field_bool, edit_field_number_u16, edit_field_number_u8, edit_field_text, edit_field_text_option, generate_form_reducer};
use shared::model::{TargetOutputDto, TraktApiConfigDto, TraktConfigDto, TraktContentType, TraktListConfigDto, XtreamTargetOutputDto};
use std::fmt::Display;
use std::rc::Rc;
use yew::{classes, function_component, html, use_context, use_effect_with, use_memo, use_reducer, use_state, Callback, Html, Properties, UseReducerHandle};
use yew_i18n::use_translation;

const LABEL_SKIP_LIVE_DIRECT_SOURCE: &str = "LABEL.SKIP_LIVE_DIRECT_SOURCE";
const LABEL_SKIP_VOD_DIRECT_SOURCE: &str = "LABEL.SKIP_VOD_DIRECT_SOURCE";
const LABEL_SKIP_SERIES_DIRECT_SOURCE: &str = "LABEL.SKIP_SERIES_DIRECT_SOURCE";
const LABEL_RESOLVE_VOD: &str = "LABEL.RESOLVE_VOD";
const LABEL_RESOLVE_VOD_DELAY: &str = "LABEL.RESOLVE_VOD_DELAY_SEC";
const LABEL_RESOLVE_SERIES: &str = "LABEL.RESOLVE_SERIES";
const LABEL_RESOLVE_SERIES_DELAY: &str = "LABEL.RESOLVE_SERIES_DELAY_SEC";
const LABEL_FILTER: &str = "LABEL.FILTER";
const LABEL_TRAKT_API_KEY: &str = "LABEL.TRAKT_API_KEY";
const LABEL_TRAKT_API_VERSION: &str = "LABEL.TRAKT_API_VERSION";
const LABEL_TRAKT_API_URL: &str = "LABEL.TRAKT_API_URL";
const LABEL_TRAKT_LISTS: &str = "LABEL.TRAKT_LISTS";
const LABEL_TRAKT_USER: &str = "LABEL.TRAKT_USER";
const LABEL_TRAKT_LIST_SLUG: &str = "LABEL.TRAKT_LIST_SLUG";
const LABEL_TRAKT_CATEGORY_NAME: &str = "LABEL.TRAKT_CATEGORY_NAME";
const LABEL_TRAKT_CONTENT_TYPE: &str = "LABEL.TRAKT_CONTENT_TYPE";
const LABEL_TRAKT_FUZZY_MATCH_THRESHOLD: &str = "LABEL.TRAKT_FUZZY_MATCH_THRESHOLD";
const LABEL_ADD_TRAKT_LIST: &str = "LABEL.ADD_TRAKT_LIST";


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
generate_form_reducer!(
    state: TraktApiConfigFormState { form: TraktApiConfigDto },
    action_name: TraktApiConfigFormAction,
    fields {
        Key => key: String,
        Version => version: String,
        Url => url: String,
    }
);

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
    pub(crate) block_id: BlockId,
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

    let trakt_api_state: UseReducerHandle<TraktApiConfigFormState> =
        use_reducer(|| TraktApiConfigFormState {
            form: TraktApiConfigDto::default(),
            modified: false,
        });

    // State for Trakt lists
    let trakt_lists_state = use_state(|| Vec::<TraktListConfigDto>::new());

    // State for showing trakt list form
    let show_trakt_list_form_state = use_state(|| false);

    let view_visible = use_state(|| OutputFormPage::Main.to_string());

    let on_tab_click = {
        let view_visible = view_visible.clone();
        Callback::from(move |page: OutputFormPage| view_visible.set(page.to_string()))
    };

    {
        let output_form_state = output_form_state.clone();
        let trakt_api_state = trakt_api_state.clone();
        let trakt_lists_state = trakt_lists_state.clone();

        let config_output = props.output.clone();

        use_effect_with(config_output, move |cfg| {
            if let Some(target) = cfg {
                output_form_state.dispatch(XtreamTargetOutputFormAction::SetAll(target.as_ref().clone()));

                // Load Trakt configuration
                if let Some(trakt) = &target.trakt {
                    trakt_api_state.dispatch(TraktApiConfigFormAction::SetAll(trakt.api.clone()));
                    trakt_lists_state.set(trakt.lists.clone());
                } else {
                    trakt_api_state.dispatch(TraktApiConfigFormAction::SetAll(TraktApiConfigDto::default()));
                    trakt_lists_state.set(Vec::new());
                }
            } else {
                output_form_state.dispatch(XtreamTargetOutputFormAction::SetAll(XtreamTargetOutputDto::default()));
                trakt_api_state.dispatch(TraktApiConfigFormAction::SetAll(TraktApiConfigDto::default()));
                trakt_lists_state.set(Vec::new());
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

    let render_trakt = || {
        let trakt_lists = trakt_lists_state.clone();
        let trakt_api = trakt_api_state.clone();
        let show_trakt_list_form = show_trakt_list_form_state.clone();

        html! {
            <Card class="tp__config-view__card">
                // Trakt API Configuration
                <div class="tp__form-section">
                    <h3>{"API Configuration"}</h3>
                    { edit_field_text!(trakt_api, translate.t(LABEL_TRAKT_API_KEY), key, TraktApiConfigFormAction::Key) }
                    { edit_field_text!(trakt_api, translate.t(LABEL_TRAKT_API_VERSION), version, TraktApiConfigFormAction::Version) }
                    { edit_field_text!(trakt_api, translate.t(LABEL_TRAKT_API_URL), url, TraktApiConfigFormAction::Url) }
                </div>

                // Trakt Lists
                { config_field_child!(translate.t(LABEL_TRAKT_LISTS), {
                    let trakt_lists_list = trakt_lists.clone();
                    let trakt_lists_add = trakt_lists.clone();
                    let show_trakt_list_form_toggle = show_trakt_list_form.clone();
                    let show_trakt_list_form_check = show_trakt_list_form.clone();

                    html! {
                        <div class="tp__form-list">
                            <div class="tp__form-list__items">
                            {
                                for (*trakt_lists_list).iter().enumerate().map(|(idx, list)| {
                                    let trakt_lists_remove = trakt_lists_list.clone();
                                    let content_type_str = match list.content_type {
                                        TraktContentType::Vod => "Vod",
                                        TraktContentType::Series => "Series",
                                        TraktContentType::Both => "Both",
                                    };
                                    html! {
                                        <div class="tp__form-list__item" key={idx}>
                                            <div class="tp__form-list__item-content">
                                                <span>
                                                    <strong>{&list.user}</strong>
                                                    {" / "}
                                                    {&list.list_slug}
                                                    {" - "}
                                                    {&list.category_name}
                                                    {" ("}
                                                    {content_type_str}
                                                    {", "}
                                                    {list.fuzzy_match_threshold}
                                                    {"%)"}
                                                </span>
                                            </div>
                                            <button
                                                class="tp__form-list__item-remove"
                                                onclick={Callback::from(move |_| {
                                                    let mut items = (*trakt_lists_remove).clone();
                                                    items.remove(idx);
                                                    trakt_lists_remove.set(items);
                                                })}
                                            >
                                                {"×"}
                                            </button>
                                        </div>
                                    }
                                })
                            }
                            </div>

                            if *show_trakt_list_form_check {
                                <TraktListItemForm
                                    on_submit={Callback::from(move |list: TraktListConfigDto| {
                                        let mut items = (*trakt_lists_add).clone();
                                        items.push(list);
                                        trakt_lists_add.set(items);
                                        show_trakt_list_form.set(false);
                                    })}
                                    on_cancel={Callback::from(move |_| {
                                        show_trakt_list_form.set(false);
                                    })}
                                />
                            } else {
                                <TextButton
                                    class="primary"
                                    name="add_trakt_list"
                                    icon="Add"
                                    title={translate.t(LABEL_ADD_TRAKT_LIST)}
                                    onclick={Callback::from(move |_| {
                                        show_trakt_list_form_toggle.set(true);
                                    })}
                                />
                            }
                        </div>
                    }
                })}
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
                {render_trakt()}
                </Panel>
            </div>
            </div>
        }
    };

    let handle_apply_target = {
        let source_editor_ctx = source_editor_ctx.clone();
        let output_form_state = output_form_state.clone();
        let trakt_api_state = trakt_api_state.clone();
        let trakt_lists_state = trakt_lists_state.clone();
        let block_id = props.block_id;
        Callback::from(move |_| {
            let mut output = output_form_state.data().clone();

            // Handle Trakt configuration
            let trakt_lists = (*trakt_lists_state).clone();
            output.trakt = if trakt_lists.is_empty() {
                None
            } else {
                Some(TraktConfigDto {
                    api: trakt_api_state.data().clone(),
                    lists: trakt_lists,
                })
            };

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
