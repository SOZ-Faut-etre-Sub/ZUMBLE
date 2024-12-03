use dashmap::DashSet;

#[derive(Default, Debug)]
pub struct VoiceTarget {
    pub sessions: DashSet<u32>,
    pub channels: DashSet<u32>,
}
