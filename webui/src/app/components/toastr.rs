use log::info;
use crate::hooks::use_service_context;
use crate::services::{Toast, ToastType};
use yew::prelude::*;
use yew_hooks::use_mount;

#[function_component]
pub fn ToastrView() -> Html {
    let service_ctx = use_service_context();
    let toasts = use_state(Vec::<Toast>::new);
    {
        let service_ctx = service_ctx.clone();
        let toasts = toasts.clone();
        use_mount(move || service_ctx.toastr.subscribe(move |new_toasts| {
            toasts.set(new_toasts);
        }));
    }

    if toasts.is_empty() {
        html! {}
    } else {
        info!("view toassts {}", toasts.len());
        html! {
            <div class="tp__toastr-container">
               { for toasts.iter().map(render_toast) }
            </div>
        }
    }
}

fn render_toast(toast: &Toast) -> Html {
    let class = match toast.toast_type {
        ToastType::Success => "tp__toast success",
        ToastType::Error => "tp__toast error",
        ToastType::Info => "tp__toast info",
        ToastType::Warning => "tp__toast warning",
    };
    html! {
        <div key={toast.id} class={classes!(class)}>
            { &toast.message }
        </div>
    }
}