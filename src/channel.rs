use dashmap::DashMap;

use crate::client::{ClientRef};
use crate::proto::mumble::ChannelState;
use std::sync::Arc;

pub type ChannelRef = Arc<Channel>;

pub struct Channel {
    pub id: u32,
    pub parent_id: Option<u32>,
    pub name: String,
    pub description: String,
    pub temporary: bool,
    pub listeners: Arc<DashMap<u32, ClientRef>>,
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
            description,
            temporary,
            listeners: Arc::new(DashMap::new()),
        }
    }

    pub fn get_channel_state(&self) -> Arc<ChannelState> {
        return self.channel_state_cache.clone();
    }

    pub fn get_listeners(&self) -> Arc<DashMap<u32, ClientRef>> {
        return self.listeners.clone();
    }
}
