use crate::app::components::config::HasFormData;
use crate::app::components::key_value_editor::KeyValueEditor;
use crate::app::components::select::Select;
use crate::app::components::{BlockId, BlockInstance, Card, DropDownOption, DropDownSelection, EditMode, Panel, RadioButtonGroup, SourceEditorContext, TextButton};
use crate::{config_field_child, edit_field_bool, edit_field_date, edit_field_number_i16, edit_field_number_u16, edit_field_text, edit_field_text_option, generate_form_reducer};
use shared::model::{ConfigInputAliasDto, ConfigInputDto, ConfigInputOptionsDto, EpgConfigDto, EpgSourceDto, InputFetchMethod, InputType, StagedInputDto};
use std::collections::HashMap;
use std::fmt::Display;
use std::rc::Rc;
use yew::{classes, function_component, html, use_context, use_effect_with, use_memo, use_reducer, use_state, Callback, Html, Properties, UseReducerHandle};
use yew_i18n::use_translation;

const LABEL_NAME: &str = "LABEL.NAME";
const LABEL_INPUT_TYPE: &str = "LABEL.INPUT_TYPE";
const LABEL_FETCH_METHOD: &str = "LABEL.METHOD";
const LABEL_HEADERS: &str = "LABEL.HEADERS";
const LABEL_URL: &str = "LABEL.URL";
const LABEL_EPG: &str = "LABEL.EPG";
const LABEL_EPG_SOURCES: &str = "LABEL.EPG_SOURCES";
const LABEL_EPG_SOURCE_URL: &str = "LABEL.EPG_SOURCE_URL";
const LABEL_USERNAME: &str = "LABEL.USERNAME";
const LABEL_PASSWORD: &str = "LABEL.PASSWORD";
const LABEL_PERSIST: &str = "LABEL.PERSIST";
const LABEL_ENABLED: &str = "LABEL.ENABLED";
const LABEL_ALIASES: &str = "LABEL.ALIASES";
const LABEL_ALIAS_ID: &str = "LABEL.ALIAS_ID";
const LABEL_ALIAS_NAME: &str = "LABEL.ALIAS_NAME";
const LABEL_PRIORITY: &str = "LABEL.PRIORITY";
const LABEL_MAX_CONNECTIONS: &str = "LABEL.MAX_CONNECTIONS";
const LABEL_EXP_DATE: &str = "LABEL.EXP_DATE";
const LABEL_ADD_HEADER: &str = "LABEL.ADD_HEADER";
const LABEL_ADD_EPG_SOURCE: &str = "LABEL.ADD_EPG_SOURCE";
const LABEL_ADD_ALIAS: &str = "LABEL.ADD_ALIAS";
const LABEL_XTREAM_SKIP_LIVE: &str = "LABEL.SKIP_LIVE";
const LABEL_XTREAM_SKIP_VOD: &str = "LABEL.SKIP_VOD";
const LABEL_XTREAM_SKIP_SERIES: &str = "LABEL.SKIP_SERIES";
const LABEL_XTREAM_LIVE_STREAM_USE_PREFIX: &str = "LABEL.LIVE_STREAM_USE_PREFIX";
const LABEL_XTREAM_LIVE_STREAM_WITHOUT_EXTENSION: &str = "LABEL.LIVE_STREAM_WITHOUT_EXTENSION";


#[derive(Copy, Clone, PartialEq, Eq)]
enum InputFormPage {
    Main,
    Options,
    Staged,
    Advanced,
}

impl Display for InputFormPage {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", match *self {
            InputFormPage::Main => "Main".to_string(),
            InputFormPage::Options => "Options".to_string(),
            InputFormPage::Staged => "Staged".to_string(),
            InputFormPage::Advanced => "Advanced".to_string(),
        })
    }
}

generate_form_reducer!(
    state: EpgSourceDtoFormState { form: EpgSourceDto },
    action_name: EpgSourceDtoFormAction,
    fields {
        Url => url: String,
        Priority => priority: i16,
        LogoOverride => logo_override: bool,
    }
);

generate_form_reducer!(
    state: ConfigInputAliasDtoFormState { form: ConfigInputAliasDto },
    action_name: ConfigInputAliasDtoFormAction,
    fields {
        Id => id: u16,
        Name => name: String,
        Url => url: String,
        Username => username: Option<String>,
        Password => password: Option<String>,
        Priority => priority: i16,
        MaxConnections => max_connections: u16,
        ExpDate => exp_date: Option<i64>,
    }
);

generate_form_reducer!(
    state: ConfigInputOptionsDtoFormState { form: ConfigInputOptionsDto },
    action_name: ConfigInputOptionsFormAction,
    fields {
      XtreamSkipLive => xtream_skip_live: bool,
      XtreamSkipVod => xtream_skip_vod: bool,
      XtreamSkipSeries => xtream_skip_series: bool,
      XtreamLiveStreamUsePrefix => xtream_live_stream_use_prefix: bool,
      XtreamLiveStreamWithoutExtension => xtream_live_stream_without_extension: bool,
    }
);

generate_form_reducer!(
    state: StagedInputDtoFormState { form: StagedInputDto },
    action_name: StagedInputFormAction,
    fields {
        Url => url: String,
        Username => username: Option<String>,
        Password => password: Option<String>,
        Method => method: InputFetchMethod,
        InputType => input_type: InputType,
        // Headers => headers: HashMap<String, String>,
    }
);

generate_form_reducer!(
    state: ConfigInputFormState { form: ConfigInputDto },
    action_name: ConfigInputFormAction,
    fields {
        Name => name: String,
        Url => url: String,
        Username => username: Option<String>,
        Password => password: Option<String>,
        Persist => persist: Option<String>,
        Enabled => enabled: bool,
        Priority => priority: i16,
        MaxConnections => max_connections: u16,
        Method => method: InputFetchMethod,
        Headers => headers: HashMap<String, String>,
        ExpDate => exp_date: Option<i64>,
    }
);

#[derive(Properties, PartialEq, Clone)]
pub struct ConfigInputViewProps {
    pub(crate) block_id: BlockId,
    pub(crate) input: Option<Rc<ConfigInputDto>>,
}

#[function_component]
pub fn ConfigInputView(props: &ConfigInputViewProps) -> Html {

    let translate = use_translation();
    let source_editor_ctx = use_context::<SourceEditorContext>().expect("SourceEditorContext not found");
    let fetch_methods = use_memo((), |_| {
        [InputFetchMethod::GET, InputFetchMethod::POST]
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<String>>()
    });
    let view_visible = use_state(|| InputFormPage::Main.to_string());

    let on_tab_click = {
        let view_visible = view_visible.clone();
        Callback::from(move |page: InputFormPage| view_visible.set(page.to_string()))
    };

    let input_form_state: UseReducerHandle<ConfigInputFormState> =
        use_reducer(|| ConfigInputFormState {
            form: ConfigInputDto::default(),
            modified: false,
        });
    let input_options_state: UseReducerHandle<ConfigInputOptionsDtoFormState> =
        use_reducer(|| ConfigInputOptionsDtoFormState {
            form: ConfigInputOptionsDto::default(),
            modified: false,
        });
    let staged_input_state: UseReducerHandle<StagedInputDtoFormState> =
        use_reducer(|| StagedInputDtoFormState {
            form: StagedInputDto::default(),
            modified: false,
        });

    // State for EPG sources and Aliases
    let epg_sources_state = use_state(|| Vec::<EpgSourceDto>::new());
    let aliases_state = use_state(|| Vec::<ConfigInputAliasDto>::new());

    let staged_input_types = use_memo(staged_input_state.form.input_type, |input_type| {
        let default_it = input_type;
        [
            InputType::M3u,
            InputType::Xtream,
            // InputType::M3uBatch,
            // InputType::XtreamBatch,
        ]
            .iter()
            .map(|t| DropDownOption {
                id: t.to_string(),
                label: html! { t.to_string() },
                selected: t == default_it,
            }).collect::<Vec<DropDownOption>>()
    });

    {
        let input_form_state = input_form_state.clone();
        let input_options_state = input_options_state.clone();
        let staged_input_state = staged_input_state.clone();
        let epg_sources_state = epg_sources_state.clone();
        let aliases_state = aliases_state.clone();

        let config_input = props.input.clone();

        use_effect_with(config_input, move |cfg| {
            if let Some(input) = cfg {
                input_form_state.dispatch(ConfigInputFormAction::SetAll(input.as_ref().clone()));
                input_options_state.dispatch(ConfigInputOptionsFormAction::SetAll(
                    input
                        .options
                        .as_ref()
                        .map_or_else(ConfigInputOptionsDto::default, |d| d.clone()),
                ));
                staged_input_state.dispatch(StagedInputFormAction::SetAll(
                    input
                        .staged
                        .as_ref()
                        .map_or_else(StagedInputDto::default, |c| c.clone()),
                ));

                // Load EPG sources
                epg_sources_state.set(
                    input
                        .epg
                        .as_ref()
                        .and_then(|epg| epg.sources.clone())
                        .unwrap_or_default()
                );

                // Load aliases
                aliases_state.set(input.aliases.clone().unwrap_or_default());
            } else {
                input_form_state.dispatch(ConfigInputFormAction::SetAll(ConfigInputDto::default()));
                input_options_state.dispatch(ConfigInputOptionsFormAction::SetAll(
                    ConfigInputOptionsDto::default(),
                ));
                staged_input_state
                    .dispatch(StagedInputFormAction::SetAll(StagedInputDto::default()));
                epg_sources_state.set(Vec::new());
                aliases_state.set(Vec::new());
            }
            || ()
        });
    }

    let render_options = || {
        html! {
            <Card class="tp__config-view__card">
            { edit_field_bool!(input_options_state, translate.t(LABEL_XTREAM_SKIP_LIVE), xtream_skip_live, ConfigInputOptionsFormAction::XtreamSkipLive) }
            { edit_field_bool!(input_options_state, translate.t(LABEL_XTREAM_SKIP_VOD), xtream_skip_vod, ConfigInputOptionsFormAction::XtreamSkipVod) }
            { edit_field_bool!(input_options_state, translate.t(LABEL_XTREAM_SKIP_SERIES), xtream_skip_series, ConfigInputOptionsFormAction::XtreamSkipSeries) }
            { edit_field_bool!(input_options_state, translate.t(LABEL_XTREAM_LIVE_STREAM_USE_PREFIX), xtream_live_stream_use_prefix, ConfigInputOptionsFormAction::XtreamLiveStreamUsePrefix) }
            { edit_field_bool!(input_options_state, translate.t(LABEL_XTREAM_LIVE_STREAM_WITHOUT_EXTENSION), xtream_live_stream_without_extension, ConfigInputOptionsFormAction::XtreamLiveStreamWithoutExtension) }
            </Card>
        }
    };

    let render_staged = || {
        let staged_method_selection = Rc::new(vec![staged_input_state.form.method.to_string()]);
        let staged_input_state_1 = staged_input_state.clone();
        let staged_input_state_2 = staged_input_state.clone();
        html! {
            <Card class="tp__config-view__card">
                { edit_field_text!(staged_input_state, translate.t(LABEL_URL),  url, StagedInputFormAction::Url) }
                { edit_field_text_option!(staged_input_state, translate.t(LABEL_USERNAME), username, StagedInputFormAction::Username) }
                { edit_field_text_option!(staged_input_state, translate.t(LABEL_PASSWORD), password, StagedInputFormAction::Password, true) }
                { config_field_child!(translate.t(LABEL_FETCH_METHOD), {

                   html! {
                       <RadioButtonGroup
                        multi_select={false} none_allowed={false}
                        on_select={Callback::from(move |selections: Rc<Vec<String>>| {
                            if let Some(first) = selections.first() {
                                staged_input_state_1.dispatch(StagedInputFormAction::Method(first.parse::<InputFetchMethod>().unwrap_or(InputFetchMethod::GET)));
                            }
                        })}
                        options={&fetch_methods}
                        selected={staged_method_selection}
                    />
               }})}
               { config_field_child!(translate.t(LABEL_INPUT_TYPE), {
                   html! {
                       <Select
                        name={"staged_input_types"}
                        multi_select={false}
                        on_select={Callback::from(move |(_, selections):(String, DropDownSelection)| {
                           match selections {
                            DropDownSelection::Empty => {
                                   staged_input_state_2.dispatch(StagedInputFormAction::InputType(InputType::Xtream));
                            }
                            DropDownSelection::Single(option) => {
                                staged_input_state_2.dispatch(StagedInputFormAction::InputType(option.parse::<InputType>().unwrap_or(InputType::Xtream)));
                            }
                            DropDownSelection::Multi(options) => {
                              if let Some(first) = options.first() {
                                staged_input_state_2.dispatch(StagedInputFormAction::InputType(first.parse::<InputType>().unwrap_or(InputType::Xtream)));
                               }
                             }
                           }
                        })}
                        options={staged_input_types.clone()}
                    />
               }})}

                //{ edit_field_list!(staged_input_state, translate.t(LABEL_HEADERS), headers, StagedInputFormAction::Headers, translate.t(LABEL_ADD_HEADER)) }
            </Card>
        }
    };

    let render_input = || {
        let input_method_selection = Rc::new(vec![input_form_state.form.method.to_string()]);
        let input_form_state_disp = input_form_state.clone();
        html! {
             <Card class="tp__config-view__card">
               { edit_field_bool!(input_form_state, translate.t(LABEL_ENABLED), enabled, ConfigInputFormAction::Enabled) }
               { edit_field_text!(input_form_state, translate.t(LABEL_NAME),  name, ConfigInputFormAction::Name) }
               { edit_field_text!(input_form_state, translate.t(LABEL_URL),  url, ConfigInputFormAction::Url) }
               { edit_field_text_option!(input_form_state, translate.t(LABEL_USERNAME), username, ConfigInputFormAction::Username) }
               { edit_field_text_option!(input_form_state, translate.t(LABEL_PASSWORD), password, ConfigInputFormAction::Password, true) }
               { edit_field_number_u16!(input_form_state, translate.t(LABEL_MAX_CONNECTIONS), max_connections, ConfigInputFormAction::MaxConnections) }
               { edit_field_number_i16!(input_form_state, translate.t(LABEL_PRIORITY), priority, ConfigInputFormAction::Priority) }
               { edit_field_date!(input_form_state, translate.t(LABEL_EXP_DATE), exp_date, ConfigInputFormAction::ExpDate) }
               { config_field_child!(translate.t(LABEL_FETCH_METHOD), {
                   html! {
                       <RadioButtonGroup
                        multi_select={false} none_allowed={false}
                        on_select={Callback::from(move |selections: Rc<Vec<String>>| {
                            if let Some(first) = selections.first() {
                                input_form_state_disp.dispatch(ConfigInputFormAction::Method(first.parse::<InputFetchMethod>().unwrap_or(InputFetchMethod::GET)));
                            }
                        })}
                        options={fetch_methods.clone()}
                        selected={input_method_selection}
                    />
               }})}
               { edit_field_text_option!(input_form_state, translate.t(LABEL_PERSIST), persist, ConfigInputFormAction::Persist) }
            </Card>
        }
    };

    let render_advanced = || {
        let input_form_state_headers = input_form_state.clone();
        let epg_sources = epg_sources_state.clone();
        let aliases = aliases_state.clone();

        html! {
            <Card class="tp__config-view__card">
                // Headers Section
                { config_field_child!(translate.t(LABEL_HEADERS), {
                    html! {
                        <KeyValueEditor
                            entries={input_form_state_headers.form.headers.clone()}
                            readonly={false}
                            key_placeholder="Header name"
                            value_placeholder="Header value"
                            on_change={Callback::from(move |headers: HashMap<String, String>| {
                                input_form_state_headers.dispatch(ConfigInputFormAction::Headers(headers));
                            })}
                        />
                    }
                })}

                // EPG Sources Section
                { config_field_child!(translate.t(LABEL_EPG_SOURCES), {
                    let epg_sources_add = epg_sources.clone();
                    let epg_sources_list = epg_sources.clone();
                    html! {
                        <div class="tp__form-list">
                            <div class="tp__form-list__items">
                            {
                                for (*epg_sources).iter().enumerate().map(|(idx, source)| {
                                    let epg_sources_remove = epg_sources.clone();
                                    html! {
                                        <div class="tp__form-list__item" key={idx}>
                                            <div class="tp__form-list__item-content">
                                                <span>{&source.url}</span>
                                            </div>
                                            <button
                                                class="tp__form-list__item-remove"
                                                onclick={Callback::from(move |_| {
                                                    let mut sources = (*epg_sources_remove).clone();
                                                    sources.remove(idx);
                                                    epg_sources_remove.set(sources);
                                                })}
                                            >
                                                {"×"}
                                            </button>
                                        </div>
                                    }
                                })
                            }
                            </div>
                            <div class="tp__form-list__add">
                                <input
                                    type="text"
                                    placeholder={translate.t(LABEL_EPG_SOURCE_URL)}
                                    id="epg-source-input"
                                />
                                <button
                                    onclick={Callback::from(move |_| {
                                        if let Some(input) = web_sys::window()
                                            .and_then(|w| w.document())
                                            .and_then(|d| d.get_element_by_id("epg-source-input"))
                                            .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
                                        {
                                            let url = input.value().trim().to_string();
                                            if !url.is_empty() {
                                                let mut sources = (*epg_sources_add).clone();
                                                sources.push(EpgSourceDto {
                                                    url,
                                                    priority: 0,
                                                    logo_override: false,
                                                });
                                                epg_sources_add.set(sources);
                                                input.set_value("");
                                            }
                                        }
                                    })}
                                >
                                    {"+"}
                                </button>
                            </div>
                        </div>
                    }
                })}

                // Aliases Section
                { config_field_child!(translate.t(LABEL_ALIASES), {
                    let aliases_add = aliases.clone();
                    let aliases_list = aliases.clone();
                    html! {
                        <div class="tp__form-list">
                            <div class="tp__form-list__items">
                            {
                                for (*aliases).iter().enumerate().map(|(idx, alias)| {
                                    let aliases_remove = aliases.clone();
                                    html! {
                                        <div class="tp__form-list__item" key={idx}>
                                            <div class="tp__form-list__item-content">
                                                <span><strong>{&alias.name}</strong>{" - "}{&alias.url}</span>
                                            </div>
                                            <button
                                                class="tp__form-list__item-remove"
                                                onclick={Callback::from(move |_| {
                                                    let mut items = (*aliases_remove).clone();
                                                    items.remove(idx);
                                                    aliases_remove.set(items);
                                                })}
                                            >
                                                {"×"}
                                            </button>
                                        </div>
                                    }
                                })
                            }
                            </div>
                            <div class="tp__form-list__add-alias">
                                <input type="text" placeholder={translate.t(LABEL_ALIAS_NAME)} id="alias-name-input" />
                                <input type="text" placeholder={translate.t(LABEL_URL)} id="alias-url-input" />
                                <button
                                    onclick={Callback::from(move |_| {
                                        if let Some(window) = web_sys::window() {
                                            if let Some(document) = window.document() {
                                                let name_input = document
                                                    .get_element_by_id("alias-name-input")
                                                    .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok());
                                                let url_input = document
                                                    .get_element_by_id("alias-url-input")
                                                    .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok());

                                                if let (Some(name_el), Some(url_el)) = (name_input, url_input) {
                                                    let name = name_el.value().trim().to_string();
                                                    let url = url_el.value().trim().to_string();
                                                    if !name.is_empty() && !url.is_empty() {
                                                        let mut items = (*aliases_add).clone();
                                                        let new_id = items.iter().map(|a| a.id).max().unwrap_or(0) + 1;
                                                        items.push(ConfigInputAliasDto {
                                                            id: new_id,
                                                            name,
                                                            url,
                                                            username: None,
                                                            password: None,
                                                            priority: 0,
                                                            max_connections: 1,
                                                            exp_date: None,
                                                        });
                                                        aliases_add.set(items);
                                                        name_el.set_value("");
                                                        url_el.set_value("");
                                                    }
                                                }
                                            }
                                        }
                                    })}
                                >
                                    {"+"}
                                </button>
                            </div>
                        </div>
                    }
                })}
            </Card>
        }
    };

    let handle_apply_input = {    // source_editor_ctx.on_form_change.emit();
        let source_editor_ctx = source_editor_ctx.clone();
        let input_form_state = input_form_state.clone();
        let input_options_state = input_options_state.clone();
        let staged_input_state = staged_input_state.clone();
        let epg_sources_state = epg_sources_state.clone();
        let aliases_state = aliases_state.clone();
        let block_id = props.block_id;
        Callback::from(move |_| {
            let mut input = input_form_state.data().clone();

            let options = input_options_state.data();
            input.options = if options.is_empty() {
                None
            } else {
                Some(options.clone())
            };

            let staged_input = staged_input_state.data();
            input.staged = if staged_input.is_empty() {
                None
            } else {
                Some(staged_input.clone())
            };

            // Handle EPG
            let epg_sources = (*epg_sources_state).clone();
            input.epg = if epg_sources.is_empty() {
                None
            } else {
                Some(EpgConfigDto {
                    sources: Some(epg_sources),
                    smart_match: None,
                    t_sources: Vec::new(),
                })
            };

            // Handle Aliases
            let aliases = (*aliases_state).clone();
            input.aliases = if aliases.is_empty() {
                None
            } else {
                Some(aliases)
            };

            source_editor_ctx.on_form_change.emit((block_id, BlockInstance::Input(Rc::new(input))));
            source_editor_ctx.edit_mode.set(EditMode::Inactive);
        })
    };
    let handle_cancel = {
      let source_editor_ctx = source_editor_ctx.clone();
      Callback::from(move |_| {
          source_editor_ctx.edit_mode.set(EditMode::Inactive);
      })
    };

    let render_edit_mode = || {
        html! {
            <div class="tp__input-form__body">
                <div class="tp__tab-header">
                {
                    for [
                        InputFormPage::Main,
                        InputFormPage::Options,
                        InputFormPage::Staged,
                        InputFormPage::Advanced
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
                <Panel value={InputFormPage::Main.to_string()} active={view_visible.to_string()}>
                {render_input()}
                </Panel>
                <Panel value={InputFormPage::Options.to_string()} active={view_visible.to_string()}>
                {render_options()}
                </Panel>
                <Panel value={InputFormPage::Staged.to_string()} active={view_visible.to_string()}>
                {render_staged()}
                </Panel>
                <Panel value={InputFormPage::Advanced.to_string()} active={view_visible.to_string()}>
                {render_advanced()}
                </Panel>
            </div>
            </div>
        }
    };

    html! {
        <div class="tp__source-editor-form tp__config-view-page">
          <div class="tp__source-editor-form__toolbar tp__form-page__toolbar">
             <TextButton class="primary" name="apply_input"
                icon="Accept"
                title={ translate.t("LABEL.OK")}
                onclick={handle_apply_input}></TextButton>
             <TextButton class="secondary" name="cancel_input"
                icon="Cancel"
                title={ translate.t("LABEL.CANCEL")}
                onclick={handle_cancel}></TextButton>
          </div>
            { render_edit_mode() }
        </div>
    }
}
