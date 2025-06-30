use yew::prelude::*;

#[derive(Properties, Clone, PartialEq, Debug)]
pub struct CardProps {
    pub children: Children,
}

#[function_component]
pub fn Card(props: &CardProps) -> Html {
    html! {
        <div class="card">
            { for props.children.iter() }
        </div>
    }
}