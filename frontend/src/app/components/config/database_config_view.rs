use crate::app::ConfigContext;
use crate::{
    config_field, config_field_bool, config_field_bool_empty, config_field_empty,
    config_field_hide, config_field_optional,
};
use yew::prelude::*;
use yew_i18n::use_translation;

const LABEL_ENABLED: &str = "LABEL.ENABLED";
const LABEL_KIND: &str = "LABEL.KIND";
const LABEL_HOST: &str = "LABEL.HOST";
const LABEL_PORT: &str = "LABEL.PORT";
const LABEL_USER: &str = "LABEL.USER";
const LABEL_PASSWORD: &str = "LABEL.PASSWORD";
const LABEL_DATABASE: &str = "LABEL.DATABASE";
const LABEL_SSL_MODE: &str = "LABEL.SSL_MODE";
const LABEL_MAX_CONNECTIONS: &str = "LABEL.MAX_CONNECTIONS";

#[function_component]
pub fn DatabaseConfigView() -> Html {
    let translate = use_translation();
    let config_ctx = use_context::<ConfigContext>().expect("Config context not found");

    let render_empty = || {
        html! {
            <div class="tp__database-config-view__body tp__config-view-page__body">
                { config_field_bool_empty!(translate.t(LABEL_ENABLED)) }
                { config_field_empty!(translate.t(LABEL_KIND)) }
                { config_field_empty!(translate.t(LABEL_HOST)) }
                { config_field_empty!(translate.t(LABEL_PORT)) }
                { config_field_empty!(translate.t(LABEL_USER)) }
                { config_field_empty!(translate.t(LABEL_PASSWORD)) }
                { config_field_empty!(translate.t(LABEL_DATABASE)) }
                { config_field_empty!(translate.t(LABEL_SSL_MODE)) }
                { config_field_empty!(translate.t(LABEL_MAX_CONNECTIONS)) }
            </div>
        }
    };

    html! {
        <div class="tp__database-config-view tp__config-view-page">
            {
                if let Some(cfg) = &config_ctx.config {
                    let db = cfg.config.database.as_ref();
                    let pg = cfg.config.postgresql.as_ref();
                    html! {
                        <div class="tp__database-config-view__body tp__config-view-page__body">
                            {
                                if let Some(db) = db {
                                    config_field_bool!(db, translate.t(LABEL_ENABLED), enabled)
                                } else {
                                    config_field_bool_empty!(translate.t(LABEL_ENABLED))
                                }
                            }
                            {
                                if let Some(db) = db {
                                    config_field!(db, translate.t(LABEL_KIND), kind)
                                } else {
                                    config_field_empty!(translate.t(LABEL_KIND))
                                }
                            }
                            {
                                if let Some(pg) = pg {
                                    config_field!(pg, translate.t(LABEL_HOST), host)
                                } else {
                                    config_field_empty!(translate.t(LABEL_HOST))
                                }
                            }
                            {
                                if let Some(pg) = pg {
                                    config_field!(pg, translate.t(LABEL_PORT), port)
                                } else {
                                    config_field_empty!(translate.t(LABEL_PORT))
                                }
                            }
                            {
                                if let Some(pg) = pg {
                                    config_field!(pg, translate.t(LABEL_USER), user)
                                } else {
                                    config_field_empty!(translate.t(LABEL_USER))
                                }
                            }
                            {
                                if let Some(pg) = pg {
                                    config_field_hide!(pg, translate.t(LABEL_PASSWORD), password)
                                } else {
                                    config_field_empty!(translate.t(LABEL_PASSWORD))
                                }
                            }
                            {
                                if let Some(pg) = pg {
                                    config_field!(pg, translate.t(LABEL_DATABASE), database)
                                } else {
                                    config_field_empty!(translate.t(LABEL_DATABASE))
                                }
                            }
                            {
                                if let Some(pg) = pg {
                                    config_field!(pg, translate.t(LABEL_SSL_MODE), sslmode)
                                } else {
                                    config_field_empty!(translate.t(LABEL_SSL_MODE))
                                }
                            }
                            {
                                if let Some(pg) = pg {
                                    config_field_optional!(pg, translate.t(LABEL_MAX_CONNECTIONS), max_connections)
                                } else {
                                    config_field_empty!(translate.t(LABEL_MAX_CONNECTIONS))
                                }
                            }
                        </div>
                    }
                } else {
                    { render_empty() }
                }
            }
        </div>
    }
}
