use log::info;
use crate::app::components::preferences::Preferences;
use crate::app::components::svg_icon::AppIcon;
use crate::hooks::use_service_context;
use yew::prelude::*;
use yew_i18n::use_translation;
use crate::app::components::IconButton;
use crate::app::components::menu_item::MenuItem;

#[function_component]
pub fn Home() -> Html {
    let services = use_service_context();
    let translate = use_translation();
    let app_title = services.config.ui_config.app_title.as_ref().map_or("tuliprox", |v| v.as_str());

    let app_logo = if let Some(logo) = services.config.ui_config.app_logo.as_ref() {
        html! { <img src={logo.to_string()} alt="logo"/> }
    } else {
        html! { <AppIcon name="Logo"  width={"48"} height={"48"}/> }
    };

    let preferences_visible = use_state(|| true);

    let handle_logout = {
        let services_ctx = services.clone();
        Callback::from(move |_| services_ctx.auth.logout())
    };
    let handle_view_change = {
        let prefs_vis = preferences_visible.clone();
        Callback::from(move |()| prefs_vis.set(!&*prefs_vis))
    };

    let handle_menu_click = {
        Callback::from(move |name| info!("change page to {name}"))
    };


    //<div class={"app-header__toolbar"}><select onchange={handle_language} defaultValue={i18next.language}>{services.config().getUiConfig().languages.map(l => <option key={l} value={l}>{l}</option>)}</select></div>
    // <div class={"app-header__toolbar"}><button data-tooltip={preferencesVisible ? "LABEL.PLAYLIST_BROWSER" : "LABEL.CONFIGURATION"} onClick={handlePreferences}>{getIconByName(preferencesVisible ? "Live" : "Config")}</button></div>

    html! {
        <div class="app">
           <div class="app-sidebar">
             <div class="app-sidebar__header">
                 <span class="app-header__logo">{app_logo}</span>
                 <AppIcon name={"ChevronLeft"}></AppIcon>
            </div>
             <div class="app-sidebar__content">
              <MenuItem icon="Dashboard" name="dashboard" label={translate.t("LABEL.DASHBOARD")}
                    onclick={&handle_menu_click}></MenuItem>
              <MenuItem icon="Settings" name="settings" label={translate.t("LABEL.SETTINGS")}
                    onclick={&handle_menu_click}></MenuItem>
            </div>
           </div>

          <div class="app-main">
                <div class="app-main__header">
                    {app_title}
                    <div class={"app-main__header-toolbar"}>
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
                // <div class="app-main__content">
                //     {  if *preferences_visible {
                //                html! { <div class="app-content"><Preferences /></div> }
                //             } else {
                //             html! { <div>{"Plylist"}</div> }
                //                 //html! { <PlaylistBrowser config={server_config} /> }
                //             }
                //         }
                // </div>
          </div>
        </div>
    }
}
