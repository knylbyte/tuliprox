use log::{info, trace};
use shared::model::{ActiveUserConnectionChange, ConfigType, PlaylistUpdateState};
use crate::api::model::{ProviderConnectionChangeReceiver};

#[allow(clippy::large_enum_variant)]
#[derive(Clone, PartialEq)]
pub enum EventMessage {
    ServerError(String),
    ActiveUser(ActiveUserConnectionChange), // user_count, connection count
    ActiveProvider(String, usize), // provider name, connections
    ConfigChange(ConfigType),
    PlaylistUpdate(PlaylistUpdateState),
    PlaylistUpdateProgress(String, String),
}

pub struct EventManager {
    channel_tx: tokio::sync::broadcast::Sender<EventMessage>,
    // #[allow(dead_code)]
    //channel_rx: tokio::sync::broadcast::Receiver<EventMessage>,
}

impl EventManager {
    pub fn new(mut provider_change_rx: ProviderConnectionChangeReceiver,
    ) -> Self {
        let (channel_tx, _channel_rx) = tokio::sync::broadcast::channel(10);

        let channel_tx_clone = channel_tx.clone();
        tokio::spawn(async move {
            loop {
               tokio::select! {

                   Some((provider, connection_count)) = provider_change_rx.recv() => {
                        if let Err(e) = channel_tx_clone.send(EventMessage::ActiveProvider(provider, connection_count)) {
                            trace!("Failed to send active provider change event: {e}");
                        }
                   }
                   else => {
                        // Both channels are closed, exit gracefully
                        info!("All input channels closed, terminating event manager task");
                        break;
                   }
               }
           }
        });

        Self {
            channel_tx,
            //channel_rx,
        }
    }

    pub fn get_event_channel(&self) -> tokio::sync::broadcast::Receiver<EventMessage> {
        self.channel_tx.subscribe()
    }

    pub fn send_event(&self, event: EventMessage) {
        if let Err(err) = self.channel_tx.send(event) {
           trace!("Failed to send event: {err}");
        }
    }

}

