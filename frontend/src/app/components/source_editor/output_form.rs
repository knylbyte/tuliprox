use shared::model::{TargetOutputDto};
use std::rc::Rc;
use yew::{function_component, html, Html, Properties};

#[derive(Properties, PartialEq, Clone)]
pub struct ConfigOutputViewProps {
    pub(crate) block_id: usize,
    pub(crate) output: Option<Rc<TargetOutputDto>>,
}

#[function_component]
pub fn ConfigOutputView(props: &ConfigOutputViewProps) -> Html {
    html! {
        <div class="tp__output-form tp__config-view-page">
        </div>
    }
}
