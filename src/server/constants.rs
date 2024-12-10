/// TODO: Add these to a server.toml file so its easier to configure
/// The amount of players the server can support
pub const MAX_CLIENTS: usize = 4096;

/// the bandwidth (in bits) that the client can use
/// This mimics FiveM's current maximum
pub const MAX_BANDWIDTH_IN_BITS: u32 = 144_000;

pub const MAX_BANDWIDTH_IN_BYTES: usize = MAX_BANDWIDTH_IN_BITS as usize / 8;
