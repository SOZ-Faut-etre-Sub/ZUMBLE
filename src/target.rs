use scc::HashSet;

#[derive(Default, Debug)]
pub struct VoiceTarget {
    pub sessions: HashSet<u32>,
    pub channels: HashSet<u32>,
}
