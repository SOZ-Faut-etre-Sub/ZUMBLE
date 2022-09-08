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
    }
}

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
