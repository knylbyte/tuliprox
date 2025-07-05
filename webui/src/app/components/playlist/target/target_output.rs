use std::rc::Rc;
use yew::prelude::*;
use shared::model::{ConfigTargetDto, TargetOutputDto};
use crate::app::components::{HdHomerunOutput, M3uOutput, StrmOutput, XtreamOutput};

#[derive(Properties, Clone, PartialEq, Debug)]
pub struct TargetOutputProps {
    pub target: Rc<ConfigTargetDto>,
}

#[function_component]
pub fn TargetOutput(props: &TargetOutputProps) -> Html {
    html! {
        <div class="tp__target_output">
            {
                props.target.output.iter().map(|output| {
                    match output {
                        TargetOutputDto::Xtream(xc) => html! { <XtreamOutput output={xc} /> },
                        TargetOutputDto::M3u(m3u) => html! { <M3uOutput /> },
                        TargetOutputDto::Strm(strm) => html! { <StrmOutput /> },
                        TargetOutputDto::HdHomeRun(hdhr) => html! { <HdHomerunOutput /> },
                    }
                }).collect::<Html>()
            }
        </div>
    }
}