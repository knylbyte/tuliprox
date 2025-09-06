use crate::api::endpoints::v1_api::create_status_check;
use crate::api::model::AppState;
use crate::api::model::EventMessage;
use crate::auth::{verify_token_admin, verify_token_user};
use axum::extract::ws::CloseFrame;
use axum::{extract::ws::{Message, WebSocket, WebSocketUpgrade},response::IntoResponse};
use log::{error, info};
use shared::model::{ProtocolHandler, ProtocolHandlerMemory, ProtocolMessage, UserRole, WsCloseCode, PROTOCOL_VERSION};
use std::sync::Arc;
use shared::utils::{concat_path_leading_slash};

// WebSocket upgrade handler
async fn websocket_handler(
    axum::extract::State(app_state): axum::extract::State<Arc<AppState>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    info!("Websocket connected");
    ws.on_upgrade(move |socket| handle_socket(socket, app_state, false))
}

// WebSocket upgrade handler
async fn websocket_handler_auth(
    axum::extract::State(app_state): axum::extract::State<Arc<AppState>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    info!("Websocket connected");
    ws.on_upgrade(move |socket| handle_socket(socket, app_state, true))
}

pub fn ws_api_register(web_auth_enabled: bool, web_ui_path: &str) -> axum::Router<Arc<AppState>> {
    if web_auth_enabled {
        axum::Router::new().route(
            &concat_path_leading_slash(web_ui_path, "ws"),
            axum::routing::get(websocket_handler_auth),
        )
    } else {
        axum::Router::new().route(
            &concat_path_leading_slash(web_ui_path, "ws"),
            axum::routing::get(websocket_handler),
        )
    }
}

#[inline]
fn verify_auth_admin_token(auth_token: &str, secret_key: Option<&Vec<u8>>) -> bool {
    match secret_key.as_ref() {
        None => false,
        Some(key) => verify_token_admin(auth_token, key.as_slice()),
    }
}

#[inline]
fn verify_auth_user_token(auth_token: &str, secret_key: Option<&Vec<u8>>) -> bool {
    match secret_key.as_ref() {
        None => false,
        Some(key) => verify_token_user(auth_token, key.as_slice()),
    }
}

fn get_secret_key(app_state: &AppState, auth: bool) -> Option<Vec<u8>> {
    if !auth {
        return None;
    }

    app_state
        .app_config
        .config
        .load()
        .web_ui
        .as_ref()
        .and_then(|c| c.auth.as_ref())
        .map(|c| {
            let secret_key: &[u8] = c.secret.as_ref();
            secret_key.to_vec()
        })
}

async fn handle_handshake(msg: Message, socket: &mut WebSocket, version: u8) -> Result<(), String> {
    if let Message::Binary(bytes) = msg {
        if bytes.len() == 1 {
            let client_version = bytes[0];
            if client_version == version {
                socket
                    .send(Message::binary(bytes))
                    .await
                    .map_err(|e| e.to_string())?;
                return Ok(());
            }
            error!("Protocol Version mismatch: server={version}, client={client_version}");
        }
    }

    let _ = socket
        .send(Message::Close(Some(CloseFrame {
            code: WsCloseCode::Protocol.code(),
            reason: "Unsupported protocol".into(),
        })))
        .await;

    Err("Protocol version mismatch".into())
}

async fn handle_protocol_message(
    msg: Message,
    mem: &mut ProtocolHandlerMemory,
    app_state: &Arc<AppState>,
    auth: bool,
    secret_key: Option<&Vec<u8>>,
) -> Option<ProtocolMessage> {
    if let Message::Binary(bytes) = msg {
        match ProtocolMessage::from_bytes(bytes) {
            Ok(ProtocolMessage::Auth(auth_token)) => {
                 mem.token = None;
                 if !auth || verify_auth_admin_token(&auth_token, secret_key) {
                     mem.role = UserRole::Admin;
                     mem.token = Some(auth_token);
                     Some(ProtocolMessage::Authorized)
                 } else if verify_auth_user_token(&auth_token, secret_key) {
                     mem.role = UserRole::User;
                     mem.token = Some(auth_token);
                     Some(ProtocolMessage::Authorized)
                 } else {
                     Some(ProtocolMessage::Unauthorized)
                 }
            },
            Ok(ProtocolMessage::StatusRequest(auth_token)) => {
                if !auth || verify_auth_admin_token(&auth_token, secret_key) {
                    mem.role = UserRole::Admin;
                    mem.token = Some(auth_token);
                    let status = create_status_check(app_state).await;
                    Some(ProtocolMessage::StatusResponse(status))
                } else {
                    Some(ProtocolMessage::Unauthorized)
                }
            }
            Ok(_) => {
                error!("Unexpected protocol message after handshake");
                None
            }
            Err(e) => {
                error!("Invalid websocket message: {e}");
                Some(ProtocolMessage::Error(format!(
                    "Invalid websocket message: {e}"
                )))
            }
        }
    } else {
        None
    }
}

async fn handle_incoming_message(
    result: Result<Message, axum::Error>,
    socket: &mut WebSocket,
    handler: &mut ProtocolHandler,
    app_state: &Arc<AppState>,
    auth: bool,
    secret_key: Option<&Vec<u8>>,
) -> Result<(), String> {
    let msg = result.map_err(|e| e.to_string())?;

    match handler {
        ProtocolHandler::Version(version) => {
            handle_handshake(msg, socket, *version).await?;
            *handler = ProtocolHandler::Default(ProtocolHandlerMemory::default());
            Ok(())
        }
        ProtocolHandler::Default(mem) => {
            let msg = handle_protocol_message(msg, mem, app_state, auth, secret_key).await;
            match msg {
                None => Ok(()),
                Some(protocol_msg) => {
                    let bytes = match protocol_msg.to_bytes() {
                        Ok(bytes) => bytes,
                        Err(err) => ProtocolMessage::Error(err.to_string())
                            .to_bytes()
                            .map_err(|e| e.to_string())?,
                    };
                    Ok(socket
                        .send(Message::Binary(bytes))
                        .await
                        .map_err(|e| e.to_string())?)
                }
            }
        }
    }
}

async fn handle_event_message(socket: &mut WebSocket, event: EventMessage, handler: &ProtocolHandler) -> Result<(), String> {
    match handler {
        ProtocolHandler::Version(_) => {},
        ProtocolHandler::Default(mem) => {
            if mem.role.is_admin() {
                match event {
                    EventMessage::ServerError(error) => {
                        let msg = ProtocolMessage::ServerError(error)
                            .to_bytes()
                            .map_err(|e| e.to_string())?;
                        socket
                            .send(Message::Binary(msg))
                            .await
                            .map_err(|e| format!("Server Error event: {e} "))?;
                    }
                    EventMessage::ActiveUser(users, connections) => {
                        let msg = ProtocolMessage::ActiveUserResponse(users, connections)
                            .to_bytes()
                            .map_err(|e| e.to_string())?;
                        socket
                            .send(Message::Binary(msg))
                            .await
                            .map_err(|e| format!("Active user connection change event: {e} "))?;
                    }
                    EventMessage::ActiveProvider(provider, connections) => {
                        let msg = ProtocolMessage::ActiveProviderResponse(provider, connections)
                            .to_bytes()
                            .map_err(|e| e.to_string())?;
                        socket
                            .send(Message::Binary(msg))
                            .await
                            .map_err(|e| format!("Provider connection change event: {e} "))?;
                    }
                    EventMessage::ConfigChange(config) => {
                        let msg = ProtocolMessage::ConfigChangeResponse(config)
                            .to_bytes()
                            .map_err(|e| e.to_string())?;
                        socket
                            .send(Message::Binary(msg))
                            .await
                            .map_err(|e| format!("Configuration files change event: {e} "))?;
                    }
                    EventMessage::PlaylistUpdate(state) => {
                        let msg = ProtocolMessage::PlaylistUpdateResponse(state)
                            .to_bytes()
                            .map_err(|e| e.to_string())?;
                        socket
                            .send(Message::Binary(msg))
                            .await
                            .map_err(|e| format!("Playlist update event: {e} "))?;
                    }
                    EventMessage::PlaylistUpdateProgress(target, msg) => {
                        let msg = ProtocolMessage::PlaylistUpdateProgressResponse(target, msg)
                            .to_bytes()
                            .map_err(|e| e.to_string())?;
                        socket
                            .send(Message::Binary(msg))
                            .await
                            .map_err(|e| format!("Playlist update progress event: {e} "))?;
                    }
                }
            }
        }
    }
    Ok(())
}

// WebSocket communication logic
async fn handle_socket(mut socket: WebSocket, app_state: Arc<AppState>, auth: bool) {
    let secret_key = get_secret_key(&app_state, auth);

    let mut event_rx = app_state.event_manager.get_event_channel();

    let mut handler = ProtocolHandler::Version(PROTOCOL_VERSION);

    loop {
        tokio::select! {
            maybe_msg = socket.recv() => {
                if let Some(msg) = maybe_msg {
                    if let Err(e) = handle_incoming_message(msg, &mut socket, &mut handler, &app_state, auth, secret_key.as_ref()).await {
                        error!("WebSocket message handling error: {e}");
                        break;
                    }
                } else {
                    break;
                }
            }

            Ok(event) = event_rx.recv() => {
                if let Err(e) = handle_event_message(&mut socket, event, &handler).await {
                    error!("Failed to send event: {e}");
                    break;
                }
            }
        }
    }
}
