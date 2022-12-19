use crate::client::Client;
use crate::proto::mumble::ChannelState;
use crate::ServerState;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug)]
pub struct Channel {
    pub id: u32,
    pub parent_id: Option<u32>,
    pub name: String,
    pub description: String,
    pub temporary: bool,
    pub listeners: HashSet<u32>,
}

impl Channel {
    pub fn new(id: u32, parent_id: Option<u32>, name: String, description: String, temporary: bool) -> Self {
        Self {
            id,
            parent_id,
            name,
            description,
            temporary,
            listeners: HashSet::new(),
        }
    }

    pub fn get_channel_state(&self) -> ChannelState {
        let mut state = ChannelState::new();

        state.set_channel_id(self.id);
        state.set_name(self.name.clone());
        state.set_description(self.description.clone());

        if let Some(parent_id) = self.parent_id {
            state.set_parent(parent_id);
        }

        state.set_temporary(self.temporary);
        state.set_position(self.id as i32);

        state
    }

    pub async fn get_listeners(&self, state: Arc<RwLock<ServerState>>) -> HashMap<u32, Arc<RwLock<Client>>> {
        let mut listening_clients = HashMap::new();
        let state_read = state.read().await;

        for client in state_read.clients.values() {
            {
                let client_read = client.read().await;

                if client_read.channel_id == self.id {
                    listening_clients.insert(client_read.session_id, client.clone());
                }
            }
        }

        for client_id in &self.listeners {
            if let Some(client) = state_read.clients.get(client_id) {
                listening_clients.insert(*client_id, client.clone());
            }
        }

        listening_clients
    }
}
