use crate::client::ClientArc;
use crate::proto::mumble::ChannelState;
use crate::server::constants::ConcurrentHashMap;
use std::sync::{Arc, Weak};

pub type WeakChannelRef = Weak<Channel>;
pub type ChannelRef = Arc<Channel>;

pub struct Channel {
    pub id: u32,
    pub parent_id: Option<u32>,
    pub name: String,
    // unused, the client will get this via ChannelState anyways
    // pub description: String,
    pub temporary: bool,
    pub listeners: ConcurrentHashMap<u32, ClientArc>,
    pub clients: ConcurrentHashMap<u32, ClientArc>,
    channel_state_cache: Arc<ChannelState>,
}

impl Channel {
    pub fn new(id: u32, parent_id: Option<u32>, name: String, description: String, temporary: bool) -> Arc<Self> {
        let mut state = ChannelState::new();

        state.set_channel_id(id);
        state.set_name(name.clone());
        state.set_description(description.clone());

        if let Some(parent_id) = parent_id {
            state.set_parent(parent_id);
        }

        state.set_temporary(temporary);
        state.set_position(id as i32);

        Arc::new(Self {
            id,
            channel_state_cache: Arc::new(state),
            parent_id,
            name,
            // description,
            temporary,
            clients: ConcurrentHashMap::new(),
            listeners: ConcurrentHashMap::new(),
        })
    }

    pub fn get_channel_state(&self) -> Arc<ChannelState> {
        Arc::clone(&self.channel_state_cache)
    }

    pub fn get_listeners(&self) -> &ConcurrentHashMap<u32, ClientArc> {
        &self.listeners
    }

    pub fn get_clients(&self) -> &ConcurrentHashMap<u32, ClientArc> {
        &self.clients
    }
}
