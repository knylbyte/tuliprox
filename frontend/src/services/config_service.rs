use crate::model::WebConfig;
use crate::services::{get_base_href, request_get};
use shared::model::{AppConfigDto, ConfigInputDto, IpCheckDto};
use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use log::error;
use futures_signals::signal::Mutable;
use futures_signals::signal::SignalExt;
use shared::foundation::filter::{get_filter, prepare_templates};
use shared::foundation::mapper::MapperScript;
use shared::utils::{concat_path, concat_path_leading_slash};

pub struct ConfigService {
    pub ui_config: Rc<WebConfig>,
    pub server_config: RefCell<Option<Rc<AppConfigDto>>>,
    config_channel: Mutable<Option<Rc<AppConfigDto>>>,
    is_fetching: AtomicBool,
    config_path: String,
    ip_check_path: String,
    batch_input_content_path: String,
}

impl ConfigService {
    pub fn new(config: &WebConfig) -> Self {
        let base_href = get_base_href();
        Self {
            ui_config: Rc::new(config.clone()),
            server_config: RefCell::new(None),
            config_channel: Mutable::new(None),
            is_fetching: AtomicBool::new(false),
            config_path: concat_path_leading_slash(&base_href, "api/v1/config"),
            ip_check_path: concat_path_leading_slash(&base_href, "api/v1/ipinfo"),
            batch_input_content_path: concat_path_leading_slash(&base_href, "api/v1/config/batchContent"),
        }
    }

    pub async fn config_subscribe<F, U>(&self, callback: &mut F)
    where
        U: Future<Output=()>,
        F: FnMut(Option<Rc<AppConfigDto>>) -> U,
    {
        let fut = self.config_channel.signal_cloned().for_each(callback);
        fut.await
    }

    pub async fn get_server_config(&self) -> Option<Rc<AppConfigDto>> {
        self.fetch_server_config().await;
        self.server_config.borrow().clone()
    }

    async fn fetch_server_config(&self) {
        if self.is_fetching.swap(true, Ordering::SeqCst) {
            return;
        }
        let result = match request_get::<AppConfigDto>(&self.config_path, None).await {
            Ok(mut app_config) => {
                let templates = {
                    if let Some(templ) = app_config.sources.templates.as_mut() {
                        prepare_templates(templ).ok()
                    }  else {
                        None
                    }
                };

                for source in app_config.sources.sources.iter_mut() {
                    for target in source.targets.iter_mut() {
                        target.t_filter = get_filter(target.filter.as_str(), templates.as_ref()).ok();
                    }
                }

                if let Some(mappings) = app_config.mappings.as_mut() {
                    for mapping in mappings.mappings.mapping.iter_mut() {
                        let templates = mapping.templates.as_ref();
                        if let Some(mappers) = mapping.mapper.as_mut() {
                            for mapper in mappers.iter_mut() {
                                mapper.t_filter = get_filter(mapper.filter.as_str(), templates).ok();
                                mapper.t_script =  MapperScript::parse(&mapper.script, templates).ok();
                            }
                        }
                    }

                }

                Some(Rc::new(app_config))
            },
            Err(err) => {
                error!("{err}");
                None
            }
        };
        self.server_config.replace(result.clone());
        self.config_channel.set(result);
        self.is_fetching.store(false, Ordering::SeqCst);
    }

    pub async fn get_ip_info(&self) -> Option<IpCheckDto> {
        match request_get::<IpCheckDto>(&self.ip_check_path, None).await {
            Ok(cfg) => Some(cfg),
            Err(err) => {
                error!("{err}");
                None
            }
        }
    }

    pub async fn get_batch_input_content(&self, input: &ConfigInputDto) -> Option<String> {
        let id = input.id.to_string();
        let path = concat_path(&self.batch_input_content_path, &id);
        match request_get::<String>(&path, Some("text/plain".to_owned())).await {
            Ok(cfg) => Some(cfg),
            Err(err) => {
                error!("{err}");
                None
            }
        }
    }
}
