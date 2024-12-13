use scc::HashMap;

#[derive(Default, Debug)]
pub struct VoiceTarget {
    pub sessions: HashMap<u32, ()>,
    pub channels: HashMap<u32, ()>,
}
