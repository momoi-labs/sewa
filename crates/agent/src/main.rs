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
    use super::proto::{AgentDescription, AgentToServer};
    use prost::Message;

    #[test]
    fn agent_to_server_round_trips() {
        let message = AgentToServer {
            instance_uid: vec![7; 16],
            sequence_num: 3,
            agent_description: Some(AgentDescription::default()),
            capabilities: 1,
            ..Default::default()
        };
        let bytes = message.encode_to_vec();
        assert_eq!(AgentToServer::decode(bytes.as_slice()).unwrap(), message);
    }
}
