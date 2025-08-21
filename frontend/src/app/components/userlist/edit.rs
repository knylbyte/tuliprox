use yew::prelude::*;
use yew_i18n::use_translation;
use crate::app::components::{UserlistContext, UserlistPage, TextButton, Card};
use crate::app::components::userlist::proxy_user_credentials_form::ProxyUserCredentialsForm;
use crate::app::{ConfigContext, PlaylistContext};

#[function_component]
pub fn UserEdit() -> Html {
    let translate = use_translation();
    let userlist_ctx = use_context::<UserlistContext>().expect("Userlist context not found");
    let playlist_ctx = use_context::<PlaylistContext>().expect("Playlist context not found");
    let config_ctx = use_context::<ConfigContext>().expect("Config context not found");

    let targets = use_memo(playlist_ctx.clone(), |playlist_ctx| {
        match playlist_ctx.sources.as_ref() {
            None => vec![],
            Some(sources) => sources.iter().map(|(_, t)| t)
                .flatten()
                .cloned()
                .collect()
        }
    });

    let server = use_memo(config_ctx.clone(), |config_ctx| {
        match config_ctx.config.as_ref() {
            None => vec![],
            Some(app_config) => {
                match app_config.api_proxy.as_ref() {
                    None => vec![],
                    Some(api_proxy) => api_proxy.server.iter().cloned().collect()
                }
            }
        }
    });

    let handle_back = {
        let userlist_ctx = userlist_ctx.clone();
        Callback::from(move |_| {
            userlist_ctx.active_page.set(UserlistPage::List);
        })
    };

    html! {
      <div class="tp__userlist-edit tp__list-create">
        <div class="tp__userlist-edit__header tp__list-create__header">
           <h1>{ translate.t( if userlist_ctx.selected_user.is_none() { "LABEL.CREATE" } else { "LABEL.EDIT" } )}</h1>
           <TextButton class="primary" name="userlist"
               icon="Userlist"
               title={ translate.t("LABEL.LIST")}
               onclick={handle_back}></TextButton>
        </div>
        <div class="tp__userlist-edit__body tp__list-create__body">
            <Card>
               <ProxyUserCredentialsForm server={server.clone()} targets={targets.clone()} user={(*userlist_ctx.selected_user).clone()}/>
            </Card>
        </div>
      </div>
    }
}