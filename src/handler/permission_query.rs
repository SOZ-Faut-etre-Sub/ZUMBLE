use crate::client::ClientRef;
use crate::error::MumbleError;
use crate::handler::Handler;
use crate::proto::mumble::PermissionQuery;
use crate::proto::MessageKind;
use crate::state::ServerStateRef;

// const PERM_NONE: u32 = 0x0;
// const PERM_WRITE: u32 = 0x1;
const PERM_TRAVERSE: u32 = 0x2;
const PERM_ENTER: u32 = 0x4;
const PERM_SPEAK: u32 = 0x8;
const PERM_MUTEDEAFEN: u32 = 0x10;
const PERM_MOVE: u32 = 0x20;
// const PERM_MAKECHANNEL: u32 = 0x40;
// const PERM_LINKCHANNEL: u32 = 0x80;
const PERM_WHISPER: u32 = 0x100;
const PERM_TEXTMESSAGE: u32 = 0x200;
const PERM_MAKETEMPCHANNEL: u32 = 0x400;
const PERM_LISTEN: u32 = 0x800;
const PERM_KICK: u32 = 0x10000;
const PERM_BAN: u32 = 0x20000;
// const PERM_REGISTER: u32 = 0x40000;
// const PERM_SELFREGISTER: u32 = 0x80000;
// const PERM_CACHED: u32 = 0x8000000;
// const PERM_ALL: u32 = 0xf0fff;

const PERM_DEFAULT: u32 = PERM_TRAVERSE | PERM_ENTER | PERM_SPEAK | PERM_WHISPER | PERM_TEXTMESSAGE | PERM_MAKETEMPCHANNEL | PERM_LISTEN;
const PERM_ADMIN: u32 = PERM_DEFAULT | PERM_MUTEDEAFEN | PERM_MOVE | PERM_KICK | PERM_BAN;

impl Handler for PermissionQuery {
    async fn handle(&self, _state: ServerStateRef, client: ClientRef) -> Result<(), MumbleError> {
        let mut pq = PermissionQuery::new();
        pq.set_channel_id(self.get_channel_id());
        pq.set_permissions(PERM_ADMIN);

        {
            client.send_message(MessageKind::PermissionQuery, &pq).await?;
        }

        Ok(())
    }
}
