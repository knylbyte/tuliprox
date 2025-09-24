use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{WebSocket, MessageEvent, Event, ErrorEvent, CloseEvent};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use web_sys::js_sys::{Uint8Array, ArrayBuffer};
use log::{debug, error, trace};
use shared::model::{ProtocolMessage, PROTOCOL_VERSION};
use shared::utils::{concat_path_leading_slash};
use crate::model::EventMessage;
use crate::services::{get_base_href, get_token, EventService, StatusService};

const WS_RECONNECT_MS:i32 = 2000;

pub struct WebSocketService {
    connected: Rc<AtomicBool>,
    ws: Rc<RefCell<Option<WebSocket>>>,
    status_service: Rc<StatusService>,
    event_service: Rc<EventService>,
    ws_path: String,
}

impl WebSocketService {
    pub fn new(status_service: Rc<StatusService>, event_service: Rc<EventService>) -> Self {
        let base_href = get_base_href();
        Self {
            connected: Rc::new(AtomicBool::new(false)),
            ws: Rc::new(RefCell::new(None)),
            status_service,
            event_service,
            ws_path: concat_path_leading_slash(&base_href, "ws"),
        }
    }
    /// Helper function to allow cloning the service into JS closures for reconnect
    fn clone_for_reconnect(&self) -> Self {
        Self {
            connected: self.connected.clone(),
            ws: self.ws.clone(),
            status_service: self.status_service.clone(),
            event_service: self.event_service.clone(),
            ws_path: self.ws_path.clone(),
        }
    }

    pub fn connect_ws(&self) {
        if self.connected.load(Ordering::SeqCst) {
            return;
        }
        match WebSocket::new(&self.ws_path) {
            Err(err) => error!("Failed to open websocket connection: {err:?}"),
            Ok(socket) => {
                socket.set_binary_type(web_sys::BinaryType::Arraybuffer);
                let ws_clone = self.ws.clone();
                *ws_clone.borrow_mut() = Some(socket.clone());
                let event_service = self.event_service.clone();

                let ws_onmessage_clone = Rc::clone(&ws_clone);
                // onmessage
                let onmessage_callback = Closure::<dyn FnMut(_)>::wrap(Box::new(move |event: MessageEvent| {
                    trace!("WebSocket received message: {event:?}");
                    if let Ok(buf) = event.data().dyn_into::<ArrayBuffer>() {
                         let array = Uint8Array::new(&buf);
                         let bytes = bytes::Bytes::from(array.to_vec());
                        match ProtocolMessage::from_bytes(bytes) {
                            Ok(message) => {
                                match message {
                                    ProtocolMessage::Unauthorized => {
                                        event_service.broadcast(EventMessage::Unauthorized);
                                    },
                                    ProtocolMessage::Error(err) => {
                                        error!("{err}");
                                    },
                                    ProtocolMessage::ActiveUserResponse(user_count, connections) => {
                                        event_service.broadcast(EventMessage::ActiveUser(user_count, connections));
                                    },
                                    ProtocolMessage::ActiveProviderResponse(user_count, connections) => {
                                        event_service.broadcast(EventMessage::ActiveProvider(user_count, connections));
                                    },
                                    ProtocolMessage::StatusResponse(status) => {
                                        let data = Rc::new(status);
                                        event_service.broadcast(EventMessage::ServerStatus(data));
                                    }
                                    ProtocolMessage::ConfigChangeResponse(config_type) => {
                                        if !event_service.is_config_change_message_blocked() {
                                           event_service.broadcast(EventMessage::ConfigChange(config_type));
                                        }
                                    }
                                    ProtocolMessage::ServerError(error) => {
                                        event_service.broadcast(EventMessage::ServerError(error));
                                    }
                                    ProtocolMessage::PlaylistUpdateResponse(update_state) => {
                                        event_service.broadcast(EventMessage::PlaylistUpdate(update_state));
                                    }
                                    ProtocolMessage::PlaylistUpdateProgressResponse(target, msg) => {
                                        event_service.broadcast(EventMessage::PlaylistUpdateProgress(target, msg));
                                    }
                                    ProtocolMessage::Version(_) => {
                                        if let Some(token) = get_token() {
                                            Self::try_send_message(ws_onmessage_clone.borrow().as_ref(), ProtocolMessage::Auth(token));
                                        }
                                    }
                                    ProtocolMessage::Auth(_)
                                    | ProtocolMessage::Authorized
                                    | ProtocolMessage::StatusRequest(_) => {}
                                }
                            }
                            Err(err) => error!("Failed to decode websocket message: {err}")
                        }
                    }
                }));
                socket.set_onmessage(Some(onmessage_callback.as_ref().unchecked_ref()));
                onmessage_callback.forget(); // Important: leak the closure to keep it alive

                let ws_open_clone = Rc::clone(&ws_clone);
                let connected_clone = self.connected.clone();
                let event_service_clone = Rc::clone(&self.event_service);
                // onopen
                let onopen_callback = Closure::<dyn FnMut(_)>::wrap(Box::new(move |_event: Event| {
                    trace!("WebSocket connection opened.");
                    connected_clone.store(true, Ordering::SeqCst);
                    Self::try_send_message(ws_open_clone.borrow().as_ref(), ProtocolMessage::Version(PROTOCOL_VERSION));
                    event_service_clone.broadcast(EventMessage::WebSocketStatus(true));
                }));
                socket.set_onopen(Some(onopen_callback.as_ref().unchecked_ref()));
                onopen_callback.forget();

                let ws_close_rc = self.ws.clone();
                let connected_clone = self.connected.clone();
                let ws_service_reconnect = Rc::new(self.clone_for_reconnect());
                let event_service_clone = Rc::clone(&self.event_service);
                let onclose_callback = Closure::<dyn FnMut(_)>::wrap(Box::new(move |e: CloseEvent| {
                    debug!("Websocket closed, Code: {}, Reason: {}, WasClean: {}", e.code(), e.reason(), e.was_clean());
                    *ws_close_rc.borrow_mut() = None;
                    connected_clone.store(false, Ordering::SeqCst);
                    event_service_clone.broadcast(EventMessage::WebSocketStatus(false));

                    // schedule reconnect after 3 seconds
                    let ws_service_inner = ws_service_reconnect.clone();
                    let timeout_cb = Closure::once_into_js(Box::new(move || {
                        ws_service_inner.connect_ws();
                    }) as Box<dyn FnOnce()>);

                    web_sys::window()
                        .unwrap()
                        .set_timeout_with_callback_and_timeout_and_arguments_0(
                            timeout_cb.unchecked_ref(),
                            WS_RECONNECT_MS,
                        )
                        .unwrap();
                }));
                socket.set_onclose(Some(onclose_callback.as_ref().unchecked_ref()));
                onclose_callback.forget();

                let connected_clone = self.connected.clone();
                let ws_service_reconnect = Rc::new(self.clone_for_reconnect());
                let event_service_clone = Rc::clone(&self.event_service);
                // onerror
                let onerror_callback = Closure::<dyn FnMut(_)>::wrap(Box::new(move |e: ErrorEvent| {
                    error!("WebSocket error");
                    connected_clone.store(false, Ordering::SeqCst);
                    event_service_clone.broadcast(EventMessage::WebSocketStatus(false));
                    web_sys::console::error_1(&e);

                    // schedule reconnect after 3 seconds
                    let ws_service_inner = ws_service_reconnect.clone();
                    let timeout_cb = Closure::once_into_js(Box::new(move || {
                        ws_service_inner.connect_ws();
                    }) as Box<dyn FnOnce()>);

                    web_sys::window()
                        .unwrap()
                        .set_timeout_with_callback_and_timeout_and_arguments_0(
                            timeout_cb.unchecked_ref(),
                            WS_RECONNECT_MS,
                        )
                        .unwrap();
                }));
                socket.set_onerror(Some(onerror_callback.as_ref().unchecked_ref()));
                onerror_callback.forget();
            }
        }
    }

    fn try_send_message(ws_opt: Option<&WebSocket>, msg: ProtocolMessage) {
        if let Some(ws) = ws_opt {
            match msg.to_bytes() {
                Ok(bytes) => {
                    if let Err(err) = ws.send_with_u8_array(bytes.as_ref()) {
                        error!("Failed to send a websocket message: {err:?}");
                    }
                },
                Err(err) => error!("Failed to create WebSocket protocol version message: {err}"),
            }
        }
    }

    pub fn send_message(&self, msg: ProtocolMessage) {
        Self::try_send_message(self.ws.borrow().as_ref(), msg);
    }

    pub async fn get_server_status(&self) {
        if self.connected.load(Ordering::SeqCst) {
            if let Some(token) = get_token() {
                self.send_message(ProtocolMessage::StatusRequest(token));
            }
        } else {
            match self.status_service.get_server_status().await {
                Ok(status) => {
                    self.event_service.broadcast(EventMessage::ServerStatus(status));
                }
                Err(err) => {error!("Failed to get server status: {err:?}");}
            }
        }


        // TODO
        // on no wesocket connection

        //         let fetch_status = {
        //             let status = status_signal.clone();
        //             let services_ctx = services_ctx.clone();
        //             move || {
        //                 let status = status.clone();
        //                 let services_ctx = services_ctx.clone();
        //                 spawn_local(async move {
        //                     status.set(services_ctx.status.get_server_status().await.ok());
        //                 });
        //             }
        //         };
        //
        //         fetch_status();
        //         // all 5 seconds
        //         let interval = Interval::new(5000, move || {
        //             fetch_status();
        //         });
        //
        //         // Cleanup function
        //         || drop(interval)
    }
}