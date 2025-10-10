mod storage;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;
use web_sys::window;
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

pub fn set_timeout<F>(callback: F, millis: i32)
where
    F: FnOnce() + 'static,
{
    let cb = Closure::once_into_js(Box::new(callback) as Box<dyn FnOnce()>);
    window()
        .unwrap()
        .set_timeout_with_callback_and_timeout_and_arguments_0(
            cb.unchecked_ref(),
            millis,
        )
        .unwrap();
}