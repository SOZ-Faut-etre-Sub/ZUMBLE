/// TODO: Add these to a server.toml file so its easier to configure
/// The amount of players the server can support
pub const MAX_CLIENTS: usize = 4096;

/// the bandwidth (in bytes) that the client can use
/// This mimics FiveM's current maximum
pub const MAX_BANDWIDTH: u32 = 128_000;
