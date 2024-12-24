use crate::server::constants::ConcurrentHashMap;

#[derive(Default, Debug)]
pub struct VoiceTarget {
    pub sessions: ConcurrentHashMap<u32, ()>,
    pub channels: ConcurrentHashMap<u32, ()>,
}
