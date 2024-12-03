use crate::client::{Client, ClientRef};
use crate::error::MumbleError;
use crate::handler::Handler;
use crate::proto::mumble::ChannelState;
use crate::proto::MessageKind;
use crate::state::ServerStateRef;
use crate::ServerState;
use async_trait::async_trait;
use protobuf::reflect::ProtobufValue;
use std::sync::Arc;
use tokio::sync::RwLock;

#[async_trait]
impl Handler for ChannelState {
    async fn handle(&self, state: ServerStateRef, client: ClientRef) -> Result<(), MumbleError> {
        if self.has_channel_id() {
            tracing::warn!("editing channel is not supported");

            return Ok(());
        }

        if !self.has_parent() {
            tracing::warn!("cannot create channel: channel must have a parent");

            return Ok(());
        }

        if !self.has_name() {
            tracing::warn!("cannot create channel: channel must have a name");

            return Ok(());
        }

        if !self.get_temporary() {
            tracing::warn!("cannot create channel: channel must be temporary");

            return Ok(());
        }

        let name = self.get_name();

        if !{ state.channels.contains_key(&self.get_parent()) } {
            tracing::warn!("cannot create channel: parent channel does not exist");

            return Ok(());
        }

        let existing_channel = { state.get_channel_by_name(name).await? };

        let new_channel_id = if let Some(channel) = existing_channel {
            let channel_state = { channel.get_channel_state() };

            {
                client.send_message(MessageKind::ChannelState, channel_state.as_ref()).await?;
            }

            channel_state.get_channel_id()
        } else {
            let channel = { state.add_channel(self) };
            let channel_state = { channel.get_channel_state() };

            {
                state.broadcast_message(MessageKind::ChannelState, channel_state.as_ref()).await?;
            }

            channel_state.get_channel_id()
        };

        let leave_channel_id_result = { state.set_client_channel(client, new_channel_id).await };

        let leave_channel_id = match leave_channel_id_result {
            Ok(Some(leave_channel_id)) => leave_channel_id,
            Ok(None) => return Ok(()),
            Err(_) => return Ok(()),
        };

        {
            state.channels.remove(&leave_channel_id);
        }

        Ok(())
    }
}
