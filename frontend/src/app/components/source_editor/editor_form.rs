use yew::{function_component, html, Html};
use crate::app::components::source_editor::input_form::ConfigInputView;

#[function_component]
pub fn SourceEditorForm() -> Html {

    html! {
        <div class="tp__source-editor-form">
            <ConfigInputView></ConfigInputView>
        </div>
    }
}