use gloo_timers::callback::Timeout;
use web_sys::HtmlInputElement;
use yew::prelude::*;
use shared::model::SearchRequest;
use crate::app::components::{AppIcon, IconButton};

const MIN_LENGTH: usize = 3;
const DEBOUNCE_TIMEOUT_MS:  u32 = 500;

enum RegexState {
    Active,
    Inactive,
    Invalid,
}

#[derive(Properties, Clone, PartialEq, Debug)]
pub struct SearchProps {
    #[prop_or_default]
    pub class: String,
    pub onsearch: Option<Callback<SearchRequest>>,
}

#[function_component]
pub fn Search(props: &SearchProps) -> Html {

    let input_ref = use_node_ref();
    let regex_active = use_state(|| RegexState::Inactive);

    let handle_regex_click = {
        let regex_active = regex_active.clone();
        let input = input_ref.clone();
        Callback::from(move |_: String| {
            match *regex_active {
                RegexState::Active
                | RegexState::Invalid => {
                    regex_active.set(RegexState::Inactive);
                }
                RegexState::Inactive => {
                    if let Some(input) = input.cast::<HtmlInputElement>() {
                        let text = input.value();
                        if regex::Regex::new(&text).is_ok() {
                            regex_active.set(RegexState::Active);
                        } else {
                            regex_active.set(RegexState::Invalid);
                        }
                    }
                }
            }
        })
    };

    let debounce_timeout = use_mut_ref(|| None::<Timeout>);

    let handle_key_down = {
        let regex = regex_active.clone();
        let input = input_ref.clone();
        let on_search = props.onsearch.clone();
        Callback::from(move |e: KeyboardEvent| {
            let regex = regex.clone();
            let input = input.clone();
            let on_search = on_search.clone();
            if let Some(timeout) = debounce_timeout.borrow_mut().take() {
                timeout.cancel();
            }

            let do_search = move || {
                if let Some(cb_search) = on_search.as_ref() {
                    if let Some(input) = input.cast::<HtmlInputElement>() {
                        let text = input.value();
                        if text.len() >= MIN_LENGTH {
                            if !matches!(*regex, RegexState::Inactive) {
                                if regex::Regex::new(&text).is_ok() {
                                    regex.set(RegexState::Active);
                                    cb_search.emit(SearchRequest::Regexp(text));
                                } else {
                                    regex.set(RegexState::Invalid);
                                }
                            } else {
                                cb_search.emit(SearchRequest::Text(text));
                            }
                        } else if text.is_empty() {
                            cb_search.emit(SearchRequest::Clear);
                        }
                    }
                }
            };

            if e.code() == "Enter" {
                do_search();
            } else {
                // Set new timeout
                *debounce_timeout.borrow_mut() = Some(Timeout::new(DEBOUNCE_TIMEOUT_MS, move || {
                    do_search();
                }));
            }
        })
    };

    html! {
        <div class="tp__search">
            <div class="tp__search-wrapper">
               <AppIcon name="Search" />
                <input ref={input_ref.clone()} type="text"
                    name="search"
                    autocomplete={"on"}
                    onkeydown={handle_key_down}
                    />
                <IconButton style={match *regex_active {
                    RegexState::Active => "option-active",
                    RegexState::Invalid => "option-invalid",
                    RegexState::Inactive => ""}}
                 name="regex" icon="Regexp" onclick={handle_regex_click} />
            </div>
        </div>
    }
}