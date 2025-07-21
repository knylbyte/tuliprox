use std::sync::Arc;
use axum::{
    extract::ws::{WebSocketUpgrade, WebSocket, Message},
    response::IntoResponse,
};
use axum::extract::ws::CloseFrame;
use log::{error, info};
use shared::model::{ProtocolHandler, ProtocolMessage, WsCloseCode, PROTOCOL_VERSION};
use crate::api::endpoints::v1_api::create_status_check;
use crate::api::model::app_state::AppState;
use crate::auth::verify_token;

// WebSocket upgrade handler
async fn websocket_handler(
    axum::extract::State(app_state): axum::extract::State<Arc<AppState>>,
    ws: WebSocketUpgrade) -> impl IntoResponse {
    info!("Websocket connected");
    ws.on_upgrade(move |socket| handle_socket(socket, app_state, false))
}

// WebSocket upgrade handler
async fn websocket_handler_auth(
    axum::extract::State(app_state): axum::extract::State<Arc<AppState>>,
    ws: WebSocketUpgrade) -> impl IntoResponse {
    info!("Websocket connected");
    ws.on_upgrade(move |socket| handle_socket(socket, app_state, true))
}

pub fn ws_api_register(web_auth_enabled: bool, web_ui_path: &str) -> axum::Router<Arc<AppState>> {
    if web_auth_enabled {
        axum::Router::new().route(&format!("{web_ui_path}/ws"), axum::routing::get(websocket_handler_auth))
    } else {
        axum::Router::new().route(&format!("{web_ui_path}/ws"), axum::routing::get(websocket_handler))
    }
}


// WebSocket communication logic
async fn handle_socket(mut socket: WebSocket, app_state: Arc<AppState>, auth: bool) {
    let secret_key = if auth {
        if let Some(web_auth_config) = &app_state.app_config.config.load().web_ui.as_ref().and_then(|c| c.auth.as_ref()) {
            let secret_key: &[u8] = web_auth_config.secret.as_ref();
            Some(secret_key.to_vec())
        } else {
            None
        }
    } else {
        None
    };

    let verify_auth_token = |auth_token: &str| {
        secret_key.as_ref().map(|key| verify_token(auth_token, key.as_slice()))
    };

    let mut active_user_change_rx =  app_state.active_users.get_active_user_change_channel();
    let mut active_provider_change_rx =  app_state.active_provider.get_active_provider_change_channel();


    let mut handler = ProtocolHandler::Version(PROTOCOL_VERSION);

    loop {
        tokio::select! {
            maybe_msg = socket.recv() => {
                match maybe_msg {
                    Some(Ok(msg)) => {
                        match handler {
                            ProtocolHandler::Version(version) => {
                                let mut version_error = true;
                                if let Message::Binary(bytes) = msg {
                                    if bytes.len() == 1 {
                                        let client_version = bytes[0];
                                        if version == client_version {
                                            if socket.send(Message::binary(bytes)).await.is_err() {
                                                error!("Error sending websocket message");
                                            } else {
                                                version_error = false;
                                                handler = ProtocolHandler::Default;
                                            }
                                        } else {
                                            error!("Version mismatch: server={version}, client={client_version}");
                                        }
                                    }
                                }
                                if version_error {
                                    let _ = socket.send(Message::Close(Some(CloseFrame {
                                        code: WsCloseCode::Protocol.code(),
                                        reason: "Unsupported protocol".into(),
                                    }))).await;
                                    break;
                                }
                            }

                            ProtocolHandler::Default => {
                                if let Message::Binary(bytes) = msg {
                                    match ProtocolMessage::from_bytes(bytes) {
                                        Ok(ProtocolMessage::StatusRequest(auth_token)) => {
                                            if !auth || verify_auth_token(&auth_token).is_some() {
                                                let status = create_status_check(&app_state).await;
                                                if let Ok(response) = ProtocolMessage::StatusResponse(status).to_bytes() {
                                                    if socket.send(Message::Binary(response)).await.is_err() {
                                                        error!("Failed to send websocket status response");
                                                    }
                                                }
                                            }
                                        }
                                        Ok(_) => {
                                            error!("Unexpected protocol message after handshake");
                                        }
                                        Err(err) => {
                                            error!("Invalid websocket message: {err}");
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Some(Err(err)) => {
                        error!("WebSocket error: {err}");
                        break;
                    }
                    None => {
                        // WebSocket closed
                        break;
                    }
                }
            }

            Ok((user_count, connection_count)) = active_user_change_rx.recv() => {
                if let Ok(payload) = ProtocolMessage::ActiveUserResponse(user_count, connection_count).to_bytes() {
                    if let Err(e) = socket.send(Message::Binary(payload)).await {
                        error!("Failed to send active user change: {e}");
                        break;
                    }
                }
            }

            Ok((provider, connection_count)) = active_provider_change_rx.recv() => {
                if let Ok(payload) = ProtocolMessage::ActiveProviderResponse(provider, connection_count).to_bytes() {
                    if let Err(e) = socket.send(Message::Binary(payload)).await {
                        error!("Failed to send active user change: {e}");
                        break;
                    }
                }
            }
        }
    }
}