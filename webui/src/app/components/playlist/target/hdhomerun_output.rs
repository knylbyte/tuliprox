use yew::prelude::*;
use shared::model::HdHomeRunTargetOutputDto;

#[derive(Properties, PartialEq, Clone)]
pub struct HdHomeRunOutputProps {
    pub output: HdHomeRunTargetOutputDto,
}

#[function_component]
pub fn HdHomeRunOutput(props: &HdHomeRunOutputProps) -> Html {

    html! {
      <div class="tp__hdhomerun_output tp__target_output__output">
      </div>
    }
}