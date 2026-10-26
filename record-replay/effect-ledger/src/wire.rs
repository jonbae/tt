//! What the supervisor and its child say to each other over two pipes.
//! One frame = kind:u8, len:u32 LE, body. Every child frame names its request.

use std::io::{self, Read, Write};

use crate::outcome::Death;

pub enum Frame {
    // child -> supervisor
    Enter { rid: u64, entry: String, request: Vec<u8> },
    Begin { rid: u64, seq: u64, symbol: String, request: Vec<u8> },
    End { rid: u64, seq: u64, response: Vec<u8> },
    Leave { rid: u64, response: Vec<u8> },
    Crashed { rid: u64, message: String },
    Expect { rid: u64, message: String },
    /// A Rust panic: a host or ledger bug.
    Panic { rid: u64, message: String },
    // supervisor -> child, replay only
    Run { rid: u64, entry: String, request: Vec<u8> },
    Respond { death: Option<Death>, response: Vec<u8> },
    Die { death: Death },
}

#[derive(Default)]
struct Body(Vec<u8>);

impl Body {
    fn u64(mut self, v: u64) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn bytes(mut self, b: &[u8]) -> Self {
        self.0.extend_from_slice(&(b.len() as u32).to_le_bytes());
        self.0.extend_from_slice(b);
        self
    }
    fn death(mut self, d: Option<Death>) -> Self {
        let (kind, value) = match d {
            None => (0u8, 0i32),
            Some(Death::Signal(s)) => (1, s),
            Some(Death::Exit(c)) => (2, c),
        };
        self.0.push(kind);
        self.0.extend_from_slice(&value.to_le_bytes());
        self
    }
}

struct Cursor<'a>(&'a [u8]);

impl Cursor<'_> {
    fn take(&mut self, n: usize) -> io::Result<&[u8]> {
        if self.0.len() < n {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }
    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn bytes(&mut self) -> io::Result<Vec<u8>> {
        let n = u32::from_le_bytes(self.take(4)?.try_into().unwrap()) as usize;
        Ok(self.take(n)?.to_vec())
    }
    fn string(&mut self) -> io::Result<String> {
        Ok(String::from_utf8_lossy(&self.bytes()?).into_owned())
    }
    fn death(&mut self) -> io::Result<Option<Death>> {
        let b = self.take(5)?;
        let value = i32::from_le_bytes(b[1..5].try_into().unwrap());
        Ok(match b[0] {
            0 => None,
            1 => Some(Death::Signal(value)),
            _ => Some(Death::Exit(value)),
        })
    }
}

impl Frame {
    pub fn write(&self, w: &mut impl Write) -> io::Result<()> {
        let (kind, body) = match self {
            Frame::Enter { rid, entry, request } => (1, Body::default().u64(*rid).bytes(entry.as_bytes()).bytes(request)),
            Frame::Begin { rid, seq, symbol, request } => (2, Body::default().u64(*rid).u64(*seq).bytes(symbol.as_bytes()).bytes(request)),
            Frame::End { rid, seq, response } => (3, Body::default().u64(*rid).u64(*seq).bytes(response)),
            Frame::Leave { rid, response } => (4, Body::default().u64(*rid).bytes(response)),
            Frame::Crashed { rid, message } => (5, Body::default().u64(*rid).bytes(message.as_bytes())),
            Frame::Expect { rid, message } => (6, Body::default().u64(*rid).bytes(message.as_bytes())),
            Frame::Panic { rid, message } => (7, Body::default().u64(*rid).bytes(message.as_bytes())),
            Frame::Run { rid, entry, request } => (101, Body::default().u64(*rid).bytes(entry.as_bytes()).bytes(request)),
            Frame::Respond { death, response } => (102, Body::default().death(*death).bytes(response)),
            Frame::Die { death } => (103, Body::default().death(Some(*death))),
        };
        // One write per frame, so frames from concurrent requests never
        // interleave (the caller holds the pipe's lock).
        let mut buf = Vec::with_capacity(5 + body.0.len());
        buf.push(kind);
        buf.extend_from_slice(&(body.0.len() as u32).to_le_bytes());
        buf.extend_from_slice(&body.0);
        w.write_all(&buf)
    }

    /// `Ok(None)` at a clean EOF. A frame torn by the writer's death reads as EOF.
    pub fn read(r: &mut impl Read) -> io::Result<Option<Frame>> {
        let mut head = [0u8; 5];
        match r.read_exact(&mut head) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e),
        }
        let len = u32::from_le_bytes(head[1..5].try_into().unwrap()) as usize;
        let mut body = vec![0u8; len];
        match r.read_exact(&mut body) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e),
        }
        let mut c = Cursor(&body);
        Ok(Some(match head[0] {
            1 => Frame::Enter { rid: c.u64()?, entry: c.string()?, request: c.bytes()? },
            2 => Frame::Begin { rid: c.u64()?, seq: c.u64()?, symbol: c.string()?, request: c.bytes()? },
            3 => Frame::End { rid: c.u64()?, seq: c.u64()?, response: c.bytes()? },
            4 => Frame::Leave { rid: c.u64()?, response: c.bytes()? },
            5 => Frame::Crashed { rid: c.u64()?, message: c.string()? },
            6 => Frame::Expect { rid: c.u64()?, message: c.string()? },
            7 => Frame::Panic { rid: c.u64()?, message: c.string()? },
            101 => Frame::Run { rid: c.u64()?, entry: c.string()?, request: c.bytes()? },
            102 => Frame::Respond { death: c.death()?, response: c.bytes()? },
            103 => Frame::Die { death: c.death()?.ok_or(io::ErrorKind::InvalidData)? },
            _ => return Err(io::ErrorKind::InvalidData.into()),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(f: Frame) -> Frame {
        let mut buf = Vec::new();
        f.write(&mut buf).unwrap();
        Frame::read(&mut buf.as_slice()).unwrap().unwrap()
    }

    #[test]
    fn frames_keep_their_request() {
        match round_trip(Frame::Begin { rid: 7, seq: 3, symbol: "hosted_utc_now".into(), request: vec![1, 2] }) {
            Frame::Begin { rid, seq, symbol, request } => assert_eq!((rid, seq, symbol.as_str(), request), (7, 3, "hosted_utc_now", vec![1, 2])),
            _ => panic!("not a BEGIN"),
        }
        match round_trip(Frame::Respond { death: Some(Death::Exit(1)), response: b"ok".to_vec() }) {
            Frame::Respond { death, response } => assert_eq!((death, response), (Some(Death::Exit(1)), b"ok".to_vec())),
            _ => panic!("not a RESPOND"),
        }
    }

    #[test]
    fn a_torn_frame_reads_as_eof() {
        let mut buf = Vec::new();
        Frame::Leave { rid: 1, response: vec![9; 100] }.write(&mut buf).unwrap();
        buf.truncate(50);
        assert!(Frame::read(&mut buf.as_slice()).unwrap().is_none());
    }
}
