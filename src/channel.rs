
use scc::HashMap;

use crate::client::{ClientRef};
use crate::proto::mumble::ChannelState;
use std::sync::Arc;

pub type ChannelRef = Arc<Channel>;

pub struct Channel {
    pub id: u32,
    pub parent_id: Option<u32>,
    pub name: String,
    // unused, the client will get this via ChannelState anyways
    // pub description: String,
    pub temporary: bool,
    pub listeners: HashMap<u32, ClientRef>,
    pub clients: HashMap<u32, ClientRef>,
    channel_state_cache: Arc<ChannelState>,
}

impl Channel {
    pub fn new(id: u32, parent_id: Option<u32>, name: String, description: String, temporary: bool) -> Self {
        let mut state = ChannelState::new();

        state.set_channel_id(id);
        state.set_name(name.clone());
        state.set_description(description.clone());

        if let Some(parent_id) = parent_id {
            state.set_parent(parent_id);
        }

        state.set_temporary(temporary);
        state.set_position(id as i32);

        Self {
            id,
            channel_state_cache: Arc::new(state),
            parent_id,
            name,
            // description,
            temporary,
            clients: HashMap::new(),
            listeners: HashMap::new(),
        }
    }

    pub fn get_channel_state(&self) -> Arc<ChannelState> {
        self.channel_state_cache.clone()
    }

    pub fn get_listeners(&self) -> &HashMap<u32, ClientRef> {
        &self.listeners
    }

    pub fn get_clients(&self) -> &HashMap<u32, ClientRef> {
        &self.clients
    }
}
