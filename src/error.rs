use crate::proto::MessageKind;

quick_error! {
    #[derive(Debug)]
    pub enum MumbleError {
        UnsupportedMessageKind(kind: MessageKind) {
            display("unsupported message kind: {:?}", kind)
        }
        UnexpectedMessageKind(kind: u16) {
            display("unexpected message kind: {}", kind)
        }
        Io(err: tokio::io::Error) {
            from()
        }
        Parse(err: protobuf::ProtobufError) {
            from()
        }
        Decrypt(err: DecryptError) {
            from()
        }
        ForceDisconnect {
            display("force disconnecting client")
        }
        LockError(err: crate::sync::Error) {
            from()
        }
    }
}

impl actix_web::error::ResponseError for MumbleError {}

quick_error! {
    #[derive(Debug)]
    pub enum DecryptError {
        Io (err: tokio::io::Error) {
            from()
        }
        Eof {
            display("unexpected eof")
        }
        Repeat {
            display("repeat")
        }
        Late {
            display("late")
        }
        Mac {
            display("mac")
        }
    }
}
