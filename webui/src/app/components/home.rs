use crate::app::components::{AppIcon, DashboardView, IconButton, InputRow, Panel, PlaylistEditorView, PlaylistExplorerView, Sidebar, StatsView, UserlistView};
use crate::app::context::{ConfigContext, PlaylistContext, StatusContext};
use crate::hooks::{use_server_status, use_service_context};
use crate::model::ViewType;
use crate::services::WsMessage;
use shared::model::{AppConfigDto, StatusCheck};
use std::future;
use std::rc::Rc;
use yew::prelude::*;
use yew::suspense::use_future;

#[function_component]
pub fn Home() -> Html {
    let services = use_service_context();
    let config = use_state(|| None::<Rc<AppConfigDto>>);
    let status = use_state(|| None::<Rc<StatusCheck>>);

    let view_visible = use_state(|| ViewType::Users);

    let handle_logout = {
        let services_ctx = services.clone();
        Callback::from(move |_| services_ctx.auth.logout())
    };

    {
        let services_ctx = services.clone();
        use_effect_with((), move |_| {
            let services_ctx = services_ctx.clone();
            let services_ctx_clone = services_ctx.clone();
            let subid = services_ctx.websocket.subscribe(move |msg| {
                if matches!(msg, WsMessage::Unauthorized) {
                    services_ctx_clone.auth.logout()
                }
            });
            move || services_ctx.websocket.unsubscribe(subid)
        });
    }

    let handle_view_change = {
        let view_vis = view_visible.clone();
        Callback::from(move |view| view_vis.set(view))
    };

    let _ = use_server_status(status.clone());

    {
        // first register for config update
        let services_ctx = services.clone();
        let config_state = config.clone();
        let _ = use_future(|| async move {
            services_ctx.config.config_subscribe(
                &mut |cfg| {
                    config_state.set(cfg.clone());
                    future::ready(())
                }
            ).await
        });
    }

    {
        let services_ctx = services.clone();
        let _ = use_future(|| async move {
            let _cfg = services_ctx.config.get_server_config().await;
        });
    }

    let sources = use_memo(config.clone(), |config_ctx| {
        if let Some(cfg) = config_ctx.as_ref() {
            let mut sources = vec![];
            for source in &cfg.sources.sources {
                let mut inputs = vec![];
                for input_cfg in &source.inputs {
                    let input = Rc::new(input_cfg.clone());
                    inputs.push(Rc::new(InputRow::Input(Rc::clone(&input))));
                    if let Some(aliases) = input_cfg.aliases.as_ref() {
                        for alias in aliases {
                            inputs.push(Rc::new(InputRow::Alias(Rc::new(alias.clone()), Rc::clone(&input))));
                        }
                    }
                }
                let mut targets = vec![];
                for target in &source.targets {
                    targets.push(Rc::new(target.clone()));
                }
                sources.push((inputs, targets));
            }
            Some(Rc::new(sources))
        } else {
            None
        }
    });

    // {
    //     let services_ctx = services.clone();
    //     let status_signal = status.clone();
    //     let status_holder_signal = status_holder.clone();
    //
    //     use_effect_with((), move |_| {
    //         let subid = services_ctx.websocket.subscribe(move |msg| {
    //             match msg {
    //                 WsMessage::ServerStatus(server_status) => {
    //                     *status_holder_signal.borrow_mut() = Some(Rc::clone(&server_status));
    //                     status_signal.set(Some(server_status));
    //                 }
    //                 WsMessage::ActiveUser(user_count, connections) => {
    //                     let mut server_status = {
    //                         if let Some(old_status) = status_holder_signal.borrow().as_ref() {
    //                             (**old_status).clone()
    //                         } else {
    //                             StatusCheck::default()
    //                         }
    //                     };
    //                     server_status.active_users = user_count;
    //                     server_status.active_user_connections = connections;
    //                     let new_status = Rc::new(server_status);
    //                     *status_holder_signal.borrow_mut() = Some(Rc::clone(&new_status));
    //                     status_signal.set(Some(new_status));
    //                 }
    //                 WsMessage::ActiveProvider(provider, connections) => {
    //                     let mut server_status = {
    //                         if let Some(old_status) = status_holder_signal.borrow().as_ref() {
    //                             (**old_status).clone()
    //                         } else {
    //                             StatusCheck::default()
    //                         }
    //                     };
    //                     if let Some(treemap) = server_status.active_provider_connections.as_mut() {
    //                         if connections == 0 {
    //                             treemap.remove(&provider);
    //                         } else {
    //                             treemap.insert(provider, connections);
    //                         }
    //                     } else if connections > 0 {
    //                         let mut treemap = BTreeMap::new();
    //                         treemap.insert(provider, connections);
    //                         server_status.active_provider_connections = Some(treemap);
    //                     }
    //                     let new_status = Rc::new(server_status);
    //                     *status_holder_signal.borrow_mut() = Some(Rc::clone(&new_status));
    //                     status_signal.set(Some(new_status));
    //                 }
    //             }
    //         });
    //         let services_clone = services_ctx.clone();
    //         spawn_local(async move {
    //             services_clone.websocket.get_server_status().await;
    //         });
    //         let services_clone = services_ctx.clone();
    //         move || services_clone.websocket.unsubscribe(subid)
    //     });
    // }

    let config_context = ConfigContext {
        config: (*config).clone(),
    };

    let status_context = StatusContext {
        status: (*status).clone(),
    };
    let playlist_context = PlaylistContext {
        sources: sources.clone(),
    };

    //<div class={"app-header__toolbar"}><select onchange={handle_language} defaultValue={i18next.language}>{services.config().getUiConfig().languages.map(l => <option key={l} value={l}>{l}</option>)}</select></div>
    // <div class={"app-header__toolbar"}><button data-tooltip={preferencesVisible ? "LABEL.PLAYLIST_BROWSER" : "LABEL.CONFIGURATION"} onClick={handlePreferences}>{getIconByName(preferencesVisible ? "Live" : "Config")}</button></div>

    html! {
        <ContextProvider<ConfigContext> context={config_context}>
        <ContextProvider<StatusContext> context={status_context}>
        <ContextProvider<PlaylistContext> context={playlist_context}>
            <div class="tp__app">
               <Sidebar onview={handle_view_change}/>

              <div class="tp__app-main">
                    <div class="tp__app-main__header tp__app-header">
                        {
                            if let Some(ref title) = services.config.ui_config.app_title {
                                html! { title.as_str() }
                            } else {
                                html! { <AppIcon name="AppTitle" /> }
                            }
                        }
                        <div class={"tp__app-header-toolbar"}>
                            // <select onchange={handle_language} defaultValue={i18next.language}>{
                            //     services.config().getUiConfig().languages.map(l => <option key={l} value={l}>{l}</option>)
                            // }</select>
                           // <button data-tooltip={ if *preferences_visible { "LABEL.PLAYLIST_BROWSER" } else { "LABEL.CONFIGURATION" }}
                           //     onclick={handle_view_change}>
                           //          { if *preferences_visible {
                           //                  html! { <AppIcon name="Live" /> }
                           //              } else {
                           //                  html! { <AppIcon name="Config" /> }
                           //              }
                           //          }
                           //   </button>
                           //
                            <IconButton name="Logout" icon="Logout" onclick={handle_logout} />
                        </div>
                    </div>
                    <div class="tp__app-main__body">
                       <Panel class="tp__full-width" value={ViewType::Dashboard.to_string()} active={view_visible.to_string()}>
                        <DashboardView/>
                       </Panel>
                       <Panel class="tp__full-width" value={ViewType::Stats.to_string()} active={view_visible.to_string()}>
                        <StatsView/>
                       </Panel>
                       <Panel class="tp__full-width" value={ViewType::PlaylistEditor.to_string()} active={view_visible.to_string()}>
                        <PlaylistEditorView/>
                       </Panel>
                       <Panel class="tp__full-width" value={ViewType::PlaylistExplorer.to_string()} active={view_visible.to_string()}>
                        <PlaylistExplorerView/>
                       </Panel>
                       <Panel class="tp__full-width" value={ViewType::Users.to_string()} active={view_visible.to_string()}>
                          <UserlistView/>
                       </Panel>
                    </div>
              </div>
            </div>
        </ContextProvider<PlaylistContext>>
        </ContextProvider<StatusContext>>
        </ContextProvider<ConfigContext>>
    }
}
