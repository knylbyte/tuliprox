use yew::prelude::*;
use shared::model::{StrmTargetOutputDto};

#[derive(Properties, PartialEq, Clone)]
pub struct StrmOutputProps {
    pub output: StrmTargetOutputDto,
}

#[function_component]
pub fn StrmOutput(props: &StrmOutputProps) -> Html {

    html! {
      <div class="tp__strm_output tp__target_output__output">
      </div>
    }
}