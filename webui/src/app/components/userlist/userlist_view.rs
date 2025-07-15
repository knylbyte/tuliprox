use crate::app::components::userlist::create::UserlistCreate;
use crate::app::components::userlist::list::UserlistList;
use crate::app::components::userlist::page::UserlistPage;
use crate::app::components::{Breadcrumbs, Panel, TargetUser};
use crate::app::context::{ConfigContext, UserlistContext};
use std::rc::Rc;
use yew::prelude::*;
use yew_i18n::use_translation;

#[function_component]
pub fn UserlistView() -> Html {
    let translate = use_translation();
    let breadcrumbs = use_state(|| Rc::new(vec![translate.t("LABEL.USERLIST"), translate.t("LABEL.LIST")]));

    let active_page = use_state(|| UserlistPage::List);

    let config_ctx = use_context::<ConfigContext>().expect("Config context not found");

    let handle_breadcrumb_select = {
        let view_visible = active_page.clone();
        Callback::from(move |(_name, index)| {
            if index == 0 && *view_visible != UserlistPage::List {
                view_visible.set(UserlistPage::List);
            }
        })
    };

    {
        let breadcrumbs = breadcrumbs.clone();
        let view_visible_dep = active_page.clone();
        let view_visible = active_page.clone();
        let translate = translate.clone();
        use_effect_with(view_visible_dep, move |_| {
            match *view_visible {
                UserlistPage::List => breadcrumbs.set(Rc::new(vec![translate.t("LABEL.USERS"), translate.t("LABEL.LIST")])),
                UserlistPage::Create => breadcrumbs.set(Rc::new(vec![translate.t("LABEL.USERS"), translate.t("LABEL.CREATE")])),
            }
        });
    };

    let users = use_memo(config_ctx.config.as_ref().and_then(|c| c.api_proxy.clone()),
                         |api_cfg_opt| {
                             if let Some(api_cfg) = api_cfg_opt {
                                 let mut users = Vec::new();
                                 for target in &api_cfg.user {
                                     for creds in &target.credentials {
                                         users.push(Rc::new(TargetUser {
                                             target: target.target.to_string(),
                                             credentials: Rc::new(creds.clone()),
                                         }));
                                     }
                                 }
                                 Some(Rc::new(users))
                             } else {
                                 None
                             }
                         });

    let handle_create = {
        Callback::from(move |cmd: String| {})
    };

    let context = UserlistContext {
        users,
        active_page: active_page.clone(),
    };

    html! {
        <ContextProvider<UserlistContext> context={context}>
            <div class="tp__userlist-view tp__list-view">
                <Breadcrumbs items={&*breadcrumbs} onclick={ handle_breadcrumb_select }/>
                <div class="tp__userlist-view__body tp__list-view__body">
                    <Panel value={UserlistPage::List.to_string()} active={active_page.to_string()}>
                        <UserlistList />
                    </Panel>
                    <Panel value={UserlistPage::Create.to_string()} active={active_page.to_string()}>
                        <UserlistCreate />
                    </Panel>
                </div>
            </div>
        </ContextProvider<UserlistContext>>
    }
}