use yew::{function_component, html, Callback, Html};
use crate::app::components::loading_indicator::BusyIndicator;
use crate::app::components::{AppIcon, IconButton, ToastrView};
use crate::hooks::use_service_context;
use crate::provider::DialogProvider;

#[function_component]
pub fn ApiUserView() -> Html {
    let services = use_service_context();

    let handle_logout = {
        let services_ctx = services.clone();
        Callback::from(move |_| services_ctx.auth.logout())
    };

    html! {
        <DialogProvider>
            <ToastrView />
            <div class="tp__app">
               <BusyIndicator />

              <div class="tp__app-main">
                    <div class="tp__app-main__header tp__app-header">
                      <div class="tp__app-main__header-left">
                        {
                            if let Some(ref title) = services.config.ui_config.app_title {
                                 html! { <span class="tp__app-title">{ title }</span> }
                            } else {
                                html! { <AppIcon name="AppTitle" /> }
                            }
                        }
                        </div>
                        <div class={"tp__app-header-toolbar"}>
                            <IconButton name="Logout" icon="Logout" onclick={handle_logout} />
                        </div>
                    </div>
                    <div class="tp__app-main__body">
                        {" TODO "}
                    </div>
              </div>
            </div>
        </DialogProvider>
    }
}