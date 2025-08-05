mod storage;

pub use storage::*;

#[macro_export]
macro_rules! html_if {
    ($cond:expr, $body:tt) => {
        if $cond {
            yew::html! $body
        } else {
            yew::Html::default()
        }
    };
}

pub use html_if;