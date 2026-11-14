/// The messages of the OpAMP specification, generated from `proto/opamp`.
/// Nothing outside the tests uses them yet.
#[allow(dead_code, clippy::all, clippy::pedantic)]
mod proto {
    include!(concat!(env!("OUT_DIR"), "/opamp.proto.v1.rs"));
}

fn main() {
    println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
}

#[cfg(test)]
mod tests {
    use super::proto::ServerToAgent;
    use prost::Message;

    #[test]
    fn server_to_agent_round_trips() {
        let message = ServerToAgent {
            instance_uid: vec![7; 16],
            flags: 1,
            capabilities: 3,
            ..Default::default()
        };
        let bytes = message.encode_to_vec();
        assert_eq!(ServerToAgent::decode(bytes.as_slice()).unwrap(), message);
    }
}
