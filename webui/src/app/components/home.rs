use yew::prelude::*;
use crate::app::components::{IconButton, Sidebar, DashboardView, PlaylistView, Panel};
use crate::model::ViewType;
use crate::hooks::use_service_context;

#[function_component]
pub fn Home() -> Html {
    let services = use_service_context();
    let app_title = services.config.ui_config.app_title.as_ref().map_or("tuliprox", |v| v.as_str());

    let view_visible = use_state(|| ViewType::Playlists);

    let handle_logout = {
        let services_ctx = services.clone();
        Callback::from(move |_| services_ctx.auth.logout())
    };

    let handle_view_change = {
        let view_vis = view_visible.clone();
        Callback::from(move |view| view_vis.set(view))
    };

    //<div class={"app-header__toolbar"}><select onchange={handle_language} defaultValue={i18next.language}>{services.config().getUiConfig().languages.map(l => <option key={l} value={l}>{l}</option>)}</select></div>
    // <div class={"app-header__toolbar"}><button data-tooltip={preferencesVisible ? "LABEL.PLAYLIST_BROWSER" : "LABEL.CONFIGURATION"} onClick={handlePreferences}>{getIconByName(preferencesVisible ? "Live" : "Config")}</button></div>

    html! {
        <div class="tp__app">
           <Sidebar onview={handle_view_change}/>

          <div class="tp__app-main">
                <div class="tp__app-main__header tp__app-header">
                    {app_title}
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
                   <Panel value={ViewType::Dashboard.to_string()} active={view_visible.to_string()}>
                    <DashboardView/>
                   </Panel>
                   <Panel class="tp__full-width" value={ViewType::Playlists.to_string()} active={view_visible.to_string()}>
                    <PlaylistView/>
                   </Panel>
                   <Panel class="tp__full-width" value={ViewType::Users.to_string()} active={view_visible.to_string()}>
                      {"Users"}
                   </Panel>
                </div>
          </div>
        </div>
    }
}
