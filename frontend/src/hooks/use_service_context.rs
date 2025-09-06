use std::rc::Rc;
use yew::prelude::*;
use crate::model::WebConfig;
use crate::services::{AuthService, ConfigService, EventService, PlaylistService, StatusService, ToastrService,
                      UserService, WebSocketService};

pub struct Services {
    pub auth: Rc<AuthService>,
    pub config: Rc<ConfigService>,
    pub user: Rc<UserService>,
    pub status: Rc<StatusService>,
    pub event: Rc<EventService>,
    pub playlist: Rc<PlaylistService>,
    pub toastr: Rc<ToastrService>,
    pub websocket: Rc<WebSocketService>,
}

impl Services {
    pub fn new(web_config: &WebConfig) -> Self {
        let event = Rc::new(EventService::new());
        let config = Rc::new(ConfigService::new(web_config, Rc::clone(&event)));
        let auth = Rc::new(AuthService::new());
        let status = Rc::new(StatusService::new());
        let playlist = Rc::new(PlaylistService::new());
        let toastr = Rc::new(ToastrService::new());
        let user = Rc::new(UserService::new(Rc::clone(&event)));
        let websocket = Rc::new(WebSocketService::new(Rc::clone(&status), Rc::clone(&event)));
        Self {
            auth,
            config,
            status,
            event,
            playlist,
            user,
            toastr,
            websocket
        }
    }
}

impl PartialEq for Services {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for Services {}

#[derive(PartialEq, Eq, Clone)]
pub struct ServiceContext {
    services: Rc<Services>,
}

impl ServiceContext {
    pub fn new(config: &WebConfig) -> Self {
        Self {
            services: Rc::new(Services::new(config))
        }
    }

    pub fn services(&self) ->  Rc<Services> {
            self.services.clone()
    }
}

#[hook]
pub fn use_service_context() -> Rc<Services> {
    use_context::<UseStateHandle<ServiceContext>>().expect("Services context not found").services()
}