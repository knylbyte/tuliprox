use yew::{function_component, html, use_effect_with, use_state, Callback, Children, ContextProvider, Html, Properties};
use crate::app::ConfirmDialog;
use crate::services::{ConfirmRequest, DialogResult, DialogService};

#[derive(Properties, PartialEq)]
pub struct ConfirmProviderProps {
    pub children: Children,
}

#[function_component]
pub fn DialogProvider(props: &ConfirmProviderProps) -> Html {
    let service = use_state(DialogService::new);
    let confirm_request = use_state(|| None::<ConfirmRequest>);

    {
        let service = service.clone();
        let request = confirm_request.clone();
        use_effect_with((),
            move |_| {
                service.register_confirm(Callback::from(move |req: ConfirmRequest| {
                    request.set(Some(req));
                }));
                || ()
            },
        );
    }

    let on_confirm = {
        let request = confirm_request.clone();
        Callback::from(move |result: bool| {
            if let Some(req) = &*request {
                if let Some(cb) = req.resolve.borrow_mut().take() {
                    cb( if result {DialogResult::Ok} else { DialogResult::Cancel});
                }
            }
            request.set(None);
        })
    };

    html! {
        <ContextProvider<DialogService> context={(*service).clone()}>
            { for props.children.iter() }
            {
                if let Some(req) = &*confirm_request {
                    html! {
                        <ConfirmDialog
                            title={req.title.clone()}
                            ok_caption={req.ok_caption.clone()}
                            cancel_caption={req.cancel_caption.clone()}
                            on_confirm={on_confirm.clone()}
                        />
                    }
                } else {
                    html! {}
                }
            }
        </ContextProvider<DialogService>>
    }
}
