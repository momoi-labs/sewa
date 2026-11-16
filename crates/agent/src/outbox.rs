use std::collections::VecDeque;
use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;

use prost::Message;

use crate::opamp::PLAN_CAPABILITY;
use crate::opamp::proto::CustomMessage;
use crate::state::write_private;

pub const LIMIT: usize = 8 * 1024 * 1024;

pub struct Outbox {
    dir: PathBuf,
    limit: usize,
    entries: VecDeque<(u64, CustomMessage)>,
    next: u64,
    size: usize,
}

impl Outbox {
    pub fn open(dir: PathBuf, limit: usize) -> Self {
        let mut found: Vec<(u64, CustomMessage)> = Vec::new();
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let sequence = path
                    .extension()
                    .filter(|extension| *extension == "pb")
                    .and_then(|_| path.file_stem()?.to_str()?.parse::<u64>().ok());
                let message = sequence.and_then(|sequence| {
                    let bytes = fs::read(&path).ok()?;
                    Some((sequence, CustomMessage::decode(&bytes[..]).ok()?))
                });
                match message {
                    Some(message) => found.push(message),
                    None => {
                        eprintln!("sewa-agent: dropped {} from the outbox", path.display());
                        let _ = fs::remove_file(&path);
                    }
                }
            }
        }
        found.sort_by_key(|(sequence, _)| *sequence);
        let mut outbox = Self {
            next: found.last().map_or(0, |(sequence, _)| sequence + 1),
            size: found.iter().map(|(_, message)| message.encoded_len()).sum(),
            entries: found.into(),
            dir,
            limit,
        };
        outbox.trim();
        outbox
    }

    pub fn push(&mut self, message: CustomMessage) -> bool {
        let sequence = self.next;
        self.next += 1;
        let written = self.write(sequence, &message);
        if let Err(error) = &written {
            eprintln!("sewa-agent: cannot keep a report on disk: {error}");
        }
        self.size += message.encoded_len();
        self.entries.push_back((sequence, message));
        self.trim();
        written.is_ok()
    }

    fn write(&self, sequence: u64, message: &CustomMessage) -> io::Result<()> {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)?;
        write_private(&self.path(sequence), &message.encode_to_vec())
    }

    fn path(&self, sequence: u64) -> PathBuf {
        self.dir.join(format!("{sequence:020}.pb"))
    }

    pub fn front(&self) -> Option<&CustomMessage> {
        self.entries.front().map(|(_, message)| message)
    }

    pub fn pop(&mut self) {
        self.remove(0);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[cfg(test)]
    pub fn iter(&self) -> impl Iterator<Item = &CustomMessage> {
        self.entries.iter().map(|(_, message)| message)
    }

    #[cfg(test)]
    pub fn clear(&mut self) {
        while !self.is_empty() {
            self.pop();
        }
    }

    fn remove(&mut self, index: usize) {
        if let Some((sequence, message)) = self.entries.remove(index) {
            self.size -= message.encoded_len();
            let _ = fs::remove_file(self.path(sequence));
        }
    }

    fn trim(&mut self) {
        let mut dropped = 0;
        while self.size > self.limit && self.entries.len() > 1 {
            let last = self.entries.len() - 1;
            let index = self
                .entries
                .iter()
                .take(last)
                .position(|(_, message)| message.capability == PLAN_CAPABILITY)
                .unwrap_or(0);
            self.remove(index);
            dropped += 1;
        }
        if dropped > 0 {
            eprintln!("sewa-agent: the outbox is full; dropped {dropped} old report(s)");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(capability: &str, data: &str) -> CustomMessage {
        CustomMessage {
            capability: capability.to_owned(),
            r#type: "t".to_owned(),
            data: data.as_bytes().to_vec(),
        }
    }

    fn data(outbox: &Outbox) -> Vec<String> {
        outbox
            .iter()
            .map(|message| String::from_utf8(message.data.clone()).unwrap())
            .collect()
    }

    fn files(dir: &std::path::Path) -> usize {
        fs::read_dir(dir).map_or(0, |entries| entries.count())
    }

    #[test]
    fn keeps_the_order_across_restarts() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("outbox");
        let mut outbox = Outbox::open(dir.clone(), LIMIT);
        for index in 0..12 {
            assert!(outbox.push(message(PLAN_CAPABILITY, &format!("m{index}"))));
        }
        outbox.pop();
        drop(outbox);
        let mut outbox = Outbox::open(dir.clone(), LIMIT);
        let expected: Vec<String> = (1..12).map(|index| format!("m{index}")).collect();
        assert_eq!(data(&outbox), expected);
        outbox.push(message(PLAN_CAPABILITY, "m12"));
        drop(outbox);
        let outbox = Outbox::open(dir.clone(), LIMIT);
        assert_eq!(data(&outbox).last().unwrap(), "m12");
        assert_eq!(files(&dir), 12);
    }

    const RESULT: &str = "io.sewa.rslt";

    #[test]
    fn drops_the_oldest_plans_first_past_the_limit() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("outbox");
        let one = message(PLAN_CAPABILITY, "p0").encoded_len();
        let mut outbox = Outbox::open(dir.clone(), one * 4);
        outbox.push(message(PLAN_CAPABILITY, "p0"));
        outbox.push(message(RESULT, "r0"));
        outbox.push(message(PLAN_CAPABILITY, "p1"));
        outbox.push(message(PLAN_CAPABILITY, "p2"));
        assert_eq!(data(&outbox), ["p0", "r0", "p1", "p2"]);
        outbox.push(message(PLAN_CAPABILITY, "p3"));
        outbox.push(message(PLAN_CAPABILITY, "p4"));
        assert_eq!(data(&outbox), ["r0", "p2", "p3", "p4"]);
        assert_eq!(files(&dir), 4);
        outbox.push(message(RESULT, "r1"));
        outbox.push(message(RESULT, "r2"));
        outbox.push(message(RESULT, "r3"));
        outbox.push(message(RESULT, "r4"));
        assert_eq!(data(&outbox), ["r1", "r2", "r3", "r4"]);
        drop(outbox);
        let outbox = Outbox::open(dir.clone(), one * 2);
        assert_eq!(data(&outbox), ["r3", "r4"]);
        assert_eq!(files(&dir), 2);
    }

    #[test]
    fn a_file_that_does_not_read_is_dropped() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("outbox");
        let mut outbox = Outbox::open(dir.clone(), LIMIT);
        outbox.push(message(PLAN_CAPABILITY, "m0"));
        fs::write(dir.join(format!("{:020}.pb", 1)), b"\xff\xff\xff").unwrap();
        let outbox = Outbox::open(dir.clone(), LIMIT);
        assert_eq!(data(&outbox), ["m0"]);
        assert_eq!(files(&dir), 1);
    }
}
