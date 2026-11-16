//! FIX session layer as a sans-IO state machine.
//!
//! [`Session`] consumes raw messages and timer ticks and returns [`Output`]s
//! (bytes to send, application messages, session events). It implements
//! logon, heartbeats, test requests, sequence number checks, resend requests
//! with gap fill, sequence resets and logout. Incoming CompIDs and
//! SendingTime are checked (Reject 35=3; Logout when they are wrong) and
//! application message types outside [`SessionConfig::app_msg_types`] get a
//! BusinessMessageReject (35=j). The same engine runs as initiator or
//! acceptor, so tests can connect two sessions back to back.

use crate::message::{FrameSplitter, Message, MessageView, parse_utc_timestamp, utc_timestamp};
use crate::tags::{self, msg_type};
use std::collections::BTreeMap;
use std::time::{Duration, Instant, SystemTime};

/// Settings of one FIX session.
#[derive(Clone, Debug)]
pub struct SessionConfig {
    pub begin_string: String,
    pub sender_comp_id: String,
    pub target_comp_id: String,
    pub sender_sub_id: Option<String>,
    pub target_sub_id: Option<String>,
    pub on_behalf_of_comp_id: Option<String>,
    pub on_behalf_of_sub_id: Option<String>,
    pub heartbeat_interval: Duration,
    /// Send 141=Y on logon and start both sequences at 1.
    pub reset_on_logon: bool,
    /// Extra tags for the Logon message (for example 553/554 credentials).
    pub logon_fields: Vec<(u32, String)>,
    /// Accept logons instead of initiating them.
    pub acceptor: bool,
    /// Largest accepted difference between an incoming SendingTime (52) and
    /// the local clock; `None` skips the check.
    pub max_latency: Option<Duration>,
    /// Application message types the application handles; others are
    /// answered with BusinessMessageReject (380=3). `None` accepts all.
    pub app_msg_types: Option<Vec<String>>,
    /// Send outgoing messages as FIXCOMP (zlib) blocks. IB enables
    /// compression per session on request; incoming FIXCOMP blocks are
    /// always accepted.
    pub compression: bool,
}

impl SessionConfig {
    pub fn initiator(sender: &str, target: &str) -> Self {
        Self {
            begin_string: "FIX.4.2".into(),
            sender_comp_id: sender.into(),
            target_comp_id: target.into(),
            sender_sub_id: None,
            target_sub_id: None,
            on_behalf_of_comp_id: None,
            on_behalf_of_sub_id: None,
            heartbeat_interval: Duration::from_secs(30),
            reset_on_logon: false,
            logon_fields: Vec::new(),
            acceptor: false,
            max_latency: Some(Duration::from_secs(120)),
            app_msg_types: None,
            compression: false,
        }
    }

    pub fn acceptor(sender: &str, target: &str) -> Self {
        Self {
            acceptor: true,
            ..Self::initiator(sender, target)
        }
    }
}

/// Persists sequence numbers (and, optionally, sent messages for resends)
/// between connections.
pub trait SeqStore: Send {
    /// (next outgoing, next expected incoming)
    fn load(&mut self) -> (u64, u64);
    fn save(&mut self, next_out: u64, next_in: u64);
    /// Keep a sent application message (complete encoded bytes).
    fn save_message(&mut self, _raw: &[u8]) {}
    /// Messages kept by `save_message`, oldest first.
    fn load_messages(&mut self) -> Vec<Vec<u8>> {
        Vec::new()
    }
    /// Forget kept messages (both sequences restart at 1).
    fn reset(&mut self) {}
}

/// Sequence numbers kept in memory only.
#[derive(Debug, Clone, Copy)]
pub struct MemoryStore(pub u64, pub u64);

impl Default for MemoryStore {
    fn default() -> Self {
        Self(1, 1)
    }
}

impl SeqStore for MemoryStore {
    fn load(&mut self) -> (u64, u64) {
        (self.0, self.1)
    }
    fn save(&mut self, next_out: u64, next_in: u64) {
        self.0 = next_out;
        self.1 = next_in;
    }
}

/// Sequence numbers stored as `"<next out> <next in>"` in a file, and sent
/// application messages appended to `<file>.msgs`, so a restarted session can
/// answer a ResendRequest with the original messages.
#[derive(Debug, Clone)]
pub struct FileStore(pub std::path::PathBuf);

impl FileStore {
    fn messages_path(&self) -> std::path::PathBuf {
        let mut p = self.0.clone().into_os_string();
        p.push(".msgs");
        p.into()
    }
}

impl SeqStore for FileStore {
    fn save_message(&mut self, raw: &[u8]) {
        use std::io::Write;
        let path = self.messages_path();
        let written = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| f.write_all(raw));
        if let Err(e) = written {
            tracing::error!(path = %path.display(), error = %e, "could not save a sent FIX message");
        }
    }
    fn load_messages(&mut self) -> Vec<Vec<u8>> {
        let mut splitter = FrameSplitter::default();
        splitter.extend(&std::fs::read(self.messages_path()).unwrap_or_default());
        std::iter::from_fn(|| splitter.next_message()).collect()
    }
    fn reset(&mut self) {
        let _ = std::fs::remove_file(self.messages_path());
    }
    fn load(&mut self) -> (u64, u64) {
        let text = std::fs::read_to_string(&self.0).unwrap_or_default();
        let mut it = text.split_whitespace().filter_map(|s| s.parse().ok());
        (it.next().unwrap_or(1), it.next().unwrap_or(1))
    }
    fn save(&mut self, next_out: u64, next_in: u64) {
        // Replace the file in one step: a crash during a plain write could
        // leave it empty, and an empty file loads as sequence 1.
        let mut staged = self.0.clone().into_os_string();
        staged.push(".tmp");
        let written = std::fs::write(&staged, format!("{next_out} {next_in}"))
            .and_then(|()| std::fs::rename(&staged, &self.0));
        if let Err(e) = written {
            tracing::error!(path = %self.0.display(), error = %e, "could not save FIX sequence numbers");
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Disconnected,
    LogonSent,
    Active,
    LogoutSent,
    Closed,
}

/// Highest MsgSeqNum IB accepts by default (IBKR FIX manual, tag 34).
pub const IB_MAX_SEQ_NUM: u64 = 999_999;

/// Header tags the session writes itself; removed from stored messages before a resend.
const HEADER_TAGS: [u32; 11] = [
    tags::SENDER_COMP_ID,
    tags::TARGET_COMP_ID,
    tags::MSG_SEQ_NUM,
    tags::SENDING_TIME,
    tags::SENDER_SUB_ID,
    tags::TARGET_SUB_ID,
    tags::ON_BEHALF_OF_COMP_ID,
    tags::ON_BEHALF_OF_SUB_ID,
    tags::POSS_DUP_FLAG,
    tags::ORIG_SENDING_TIME,
    tags::DELIVER_TO_COMP_ID,
];

/// SessionRejectReason (373) values this session sends.
pub mod reject_reason {
    pub const REQUIRED_TAG_MISSING: u32 = 1;
    pub const VALUE_INCORRECT: u32 = 5;
    pub const INCORRECT_DATA_FORMAT: u32 = 6;
    pub const COMP_ID_PROBLEM: u32 = 9;
    pub const SENDING_TIME_ACCURACY: u32 = 10;
}

/// Session-level happenings reported to the application.
#[derive(Clone, Debug, PartialEq)]
pub enum SessionEvent {
    LoggedOn,
    LoggedOut(String),
    /// A gap was found; a ResendRequest was sent.
    Gap {
        expected: u64,
        received: u64,
    },
    /// The counterparty rejected a message (35=3).
    Reject(Message),
    SequenceReset(u64),
}

/// What the caller must do after feeding the session.
#[derive(Clone, Debug, PartialEq)]
pub enum Output {
    Send(Vec<u8>),
    App(Message),
    Event(SessionEvent),
    Disconnect(String),
}

/// One FIX session.
pub struct Session {
    pub cfg: SessionConfig,
    state: State,
    next_out: u64,
    next_in: u64,
    sent: BTreeMap<u64, (Message, String)>,
    /// Messages that arrived ahead of a gap; non-empty while a resend is pending.
    queued: BTreeMap<u64, Vec<u8>>,
    /// While a resend is pending: when it last made progress.
    resend_progress: Option<Instant>,
    /// When the Logon or Logout went out; sends do not move this deadline.
    state_since: Instant,
    last_sent: Instant,
    last_recv: Instant,
    test_request: Option<(String, Instant)>,
    test_counter: u64,
    store: Box<dyn SeqStore>,
}

impl Session {
    pub fn new(cfg: SessionConfig, mut store: Box<dyn SeqStore>) -> Self {
        let (next_out, next_in) = store.load();
        let mut sent = BTreeMap::new();
        for raw in store.load_messages() {
            let Ok(v) = MessageView::parse(&raw) else {
                continue;
            };
            let mut m = v.to_message();
            for tag in HEADER_TAGS {
                m.remove(tag);
            }
            sent.insert(
                v.seq_num(),
                (m, v.get(tags::SENDING_TIME).unwrap_or("").to_string()),
            );
        }
        let now = Instant::now();
        Self {
            cfg,
            state: State::Disconnected,
            next_out,
            next_in,
            sent,
            queued: BTreeMap::new(),
            resend_progress: None,
            state_since: now,
            last_sent: now,
            last_recv: now,
            test_request: None,
            test_counter: 0,
            store,
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn next_out(&self) -> u64 {
        self.next_out
    }

    pub fn next_in(&self) -> u64 {
        self.next_in
    }

    fn persist(&mut self) {
        self.store.save(self.next_out, self.next_in);
    }

    fn encode(
        &self,
        msg: &Message,
        seq: u64,
        sending_time: &str,
        poss_dup: Option<&str>,
    ) -> Vec<u8> {
        let seq_s = seq.to_string();
        let mut header: Vec<(u32, &str)> = vec![
            (tags::SENDER_COMP_ID, &self.cfg.sender_comp_id),
            (tags::TARGET_COMP_ID, &self.cfg.target_comp_id),
            (tags::MSG_SEQ_NUM, &seq_s),
        ];
        if let Some(s) = &self.cfg.sender_sub_id {
            header.push((tags::SENDER_SUB_ID, s));
        }
        if let Some(s) = &self.cfg.target_sub_id {
            header.push((tags::TARGET_SUB_ID, s));
        }
        if let Some(s) = &self.cfg.on_behalf_of_comp_id {
            header.push((tags::ON_BEHALF_OF_COMP_ID, s));
        }
        if let Some(s) = &self.cfg.on_behalf_of_sub_id {
            header.push((tags::ON_BEHALF_OF_SUB_ID, s));
        }
        if let Some(orig) = poss_dup {
            header.push((tags::POSS_DUP_FLAG, "Y"));
            header.push((tags::SENDING_TIME, sending_time));
            header.push((tags::ORIG_SENDING_TIME, orig));
        } else {
            header.push((tags::SENDING_TIME, sending_time));
        }
        let mut out = Vec::with_capacity(256);
        msg.encode(&self.cfg.begin_string, &header, &mut out);
        out
    }

    /// Assign the next sequence number, keep application messages for
    /// resends, and return the bytes to send.
    pub fn send(&mut self, msg: Message, now: Instant) -> Vec<u8> {
        let seq = self.next_out;
        if seq == IB_MAX_SEQ_NUM {
            tracing::warn!(
                "outgoing MsgSeqNum reached {IB_MAX_SEQ_NUM}, the highest IB accepts by default; log on with reset_on_logon or agree a daily or weekly reset with IB"
            );
        }
        self.next_out += 1;
        let ts = utc_timestamp(SystemTime::now());
        let bytes = self.encode(&msg, seq, &ts, None);
        if !msg_type::is_admin(&msg.msg_type) {
            self.store.save_message(&bytes);
            self.sent.insert(seq, (msg, ts));
        }
        self.last_sent = now;
        self.persist();
        bytes
    }

    fn logon_message(&self) -> Message {
        let mut m = Message::new(msg_type::LOGON);
        m.push(tags::ENCRYPT_METHOD, 0)
            .push(tags::HEART_BT_INT, self.cfg.heartbeat_interval.as_secs());
        if self.cfg.reset_on_logon {
            m.push(tags::RESET_SEQ_NUM_FLAG, "Y");
        }
        for (t, v) in &self.cfg.logon_fields {
            m.push(*t, v);
        }
        m
    }

    /// Start the session (initiator).
    pub fn logon(&mut self, now: Instant) -> Vec<u8> {
        if self.cfg.reset_on_logon {
            self.next_out = 1;
            self.next_in = 1;
            self.sent.clear();
            self.store.reset();
        }
        self.state = State::LogonSent;
        self.state_since = now;
        self.last_recv = now;
        let m = self.logon_message();
        self.send(m, now)
    }

    /// Start an orderly logout.
    pub fn logout(&mut self, text: &str, now: Instant) -> Vec<u8> {
        let mut m = Message::new(msg_type::LOGOUT);
        if !text.is_empty() {
            m.push(tags::TEXT, text);
        }
        self.state = State::LogoutSent;
        self.state_since = now;
        self.send(m, now)
    }

    /// Feed one raw message received from the counterparty.
    pub fn on_raw(&mut self, raw: &[u8], now: Instant) -> Vec<Output> {
        let mut out = Vec::new();
        self.last_recv = now;
        let expected = self.next_in;
        let view = match MessageView::parse(raw) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "garbled FIX message ignored");
                return out;
            }
        };
        self.handle(&view, now, &mut out);
        // Process queued messages that are now in sequence.
        while let Some(raw) = self.queued.remove(&self.next_in) {
            if let Ok(v) = MessageView::parse(&raw) {
                self.handle(&v, now, &mut out);
            }
        }
        self.queued.retain(|k, _| *k >= self.next_in);
        self.resend_progress = match self.resend_progress {
            _ if self.queued.is_empty() => None,
            Some(at) if self.next_in == expected => Some(at),
            _ => Some(now),
        };
        out
    }

    /// Check the header of an incoming message: `Some((SessionRejectReason,
    /// RefTagID, text))` when it must be rejected.
    fn header_problem(&self, v: &MessageView<'_>) -> Option<(u32, u32, String)> {
        for (tag, expected) in [
            (tags::SENDER_COMP_ID, &self.cfg.target_comp_id),
            (tags::TARGET_COMP_ID, &self.cfg.sender_comp_id),
        ] {
            let got = v.get(tag).unwrap_or("");
            if got != expected {
                return Some((
                    reject_reason::COMP_ID_PROBLEM,
                    tag,
                    format!("CompID problem: tag {tag} is {got:?}, expected {expected:?}"),
                ));
            }
        }
        let Some(text) = v.get(tags::SENDING_TIME) else {
            return Some((
                reject_reason::REQUIRED_TAG_MISSING,
                tags::SENDING_TIME,
                "Required tag missing: SendingTime (52)".into(),
            ));
        };
        let Some(sent) = parse_utc_timestamp(text) else {
            return Some((
                reject_reason::INCORRECT_DATA_FORMAT,
                tags::SENDING_TIME,
                format!("SendingTime {text:?} is not YYYYMMDD-HH:MM:SS[.sss]"),
            ));
        };
        let limit = self.cfg.max_latency?;
        let now = SystemTime::now();
        let skew = now
            .duration_since(sent)
            .or_else(|_| sent.duration_since(now))
            .unwrap_or_default();
        (skew > limit).then(|| {
            (
                reject_reason::SENDING_TIME_ACCURACY,
                tags::SENDING_TIME,
                format!("SendingTime accuracy problem: {text} is {skew:?} from the local clock (limit {limit:?})"),
            )
        })
    }

    fn reject(
        &mut self,
        v: &MessageView<'_>,
        reason: u32,
        ref_tag: u32,
        text: &str,
        now: Instant,
    ) -> Vec<u8> {
        let mut m = Message::new(msg_type::REJECT);
        m.push(tags::REF_SEQ_NUM, v.seq_num())
            .push(tags::REF_TAG_ID, ref_tag)
            .push(tags::REF_MSG_TYPE, v.msg_type())
            .push(tags::SESSION_REJECT_REASON, reason)
            .push(tags::TEXT, text);
        self.send(m, now)
    }

    fn handle(&mut self, v: &MessageView<'_>, now: Instant, out: &mut Vec<Output>) {
        let seq = v.seq_num();
        let t = v.msg_type();
        if let Some((reason, ref_tag, text)) = self.header_problem(v) {
            // A rejected message still uses up its sequence number.
            if seq == self.next_in {
                self.next_in += 1;
                self.persist();
            }
            out.push(Output::Send(self.reject(v, reason, ref_tag, &text, now)));
            if matches!(
                reason,
                reject_reason::COMP_ID_PROBLEM | reject_reason::SENDING_TIME_ACCURACY
            ) {
                out.push(Output::Send(self.logout(&text, now)));
                out.push(Output::Disconnect(text));
                self.state = State::Closed;
            }
            return;
        }
        if self.cfg.acceptor && self.state == State::Disconnected && t == msg_type::LOGON {
            if v.get(tags::RESET_SEQ_NUM_FLAG) == Some("Y") {
                self.next_in = 1;
                self.next_out = 1;
                self.sent.clear();
                self.store.reset();
            }
            if let Some(hb) = v.get_u64(tags::HEART_BT_INT) {
                self.cfg.heartbeat_interval = Duration::from_secs(hb.max(1));
            }
            let mut m = self.logon_message();
            if v.get(tags::RESET_SEQ_NUM_FLAG) == Some("Y") {
                m.set(tags::RESET_SEQ_NUM_FLAG, "Y");
            }
            out.push(Output::Send(self.send(m, now)));
            self.state = State::Active;
            out.push(Output::Event(SessionEvent::LoggedOn));
        } else if t == msg_type::LOGON && self.state == State::LogonSent {
            // The acceptor (IB) may override the heartbeat interval.
            if let Some(hb) = v.get_u64(tags::HEART_BT_INT) {
                self.cfg.heartbeat_interval = Duration::from_secs(hb.max(1));
            }
            if v.get(tags::RESET_SEQ_NUM_FLAG) == Some("Y") {
                self.next_in = 1;
            }
        }

        // Sequence reset (non gap fill) ignores the sequence number.
        if t == msg_type::SEQUENCE_RESET && v.get(tags::GAP_FILL_FLAG) != Some("Y") {
            match v.get_u64(tags::NEW_SEQ_NO) {
                Some(new) if new > self.next_in => {
                    self.next_in = new;
                    self.persist();
                    out.push(Output::Event(SessionEvent::SequenceReset(new)));
                }
                Some(new) if new < self.next_in => {
                    let text = format!(
                        "Attempt to decrease receive sequence number: NewSeqNo {new} is below the expected {}",
                        self.next_in
                    );
                    out.push(Output::Send(self.reject(
                        v,
                        reject_reason::VALUE_INCORRECT,
                        tags::NEW_SEQ_NO,
                        &text,
                        now,
                    )));
                }
                _ => {}
            }
            return;
        }

        if seq > self.next_in {
            let first_gap = self.queued.is_empty();
            self.queued.insert(seq, v.raw.to_vec());
            if t == msg_type::LOGON && self.state == State::LogonSent {
                self.state = State::Active;
                out.push(Output::Event(SessionEvent::LoggedOn));
            }
            if first_gap {
                out.push(Output::Event(SessionEvent::Gap {
                    expected: self.next_in,
                    received: seq,
                }));
                let mut m = Message::new(msg_type::RESEND_REQUEST);
                m.push(tags::BEGIN_SEQ_NO, self.next_in)
                    .push(tags::END_SEQ_NO, 0);
                out.push(Output::Send(self.send(m, now)));
            }
            return;
        }
        if seq < self.next_in {
            if v.is_poss_dup() {
                return;
            }
            let text = format!(
                "MsgSeqNum too low, expecting {} but received {seq}",
                self.next_in
            );
            out.push(Output::Send(self.logout(&text, now)));
            out.push(Output::Disconnect(text));
            self.state = State::Closed;
            return;
        }
        self.next_in += 1;
        self.persist();

        match t {
            msg_type::LOGON => {
                if self.state == State::LogonSent {
                    self.state = State::Active;
                    out.push(Output::Event(SessionEvent::LoggedOn));
                }
            }
            msg_type::HEARTBEAT => {
                if let Some((id, _)) = &self.test_request
                    && v.get(tags::TEST_REQ_ID) == Some(id.as_str())
                {
                    self.test_request = None;
                }
            }
            msg_type::TEST_REQUEST => {
                let mut m = Message::new(msg_type::HEARTBEAT);
                if let Some(id) = v.get(tags::TEST_REQ_ID) {
                    m.push(tags::TEST_REQ_ID, id);
                }
                out.push(Output::Send(self.send(m, now)));
            }
            msg_type::RESEND_REQUEST => {
                let begin = v.get_u64(tags::BEGIN_SEQ_NO).unwrap_or(1);
                let end = match v.get_u64(tags::END_SEQ_NO).unwrap_or(0) {
                    0 => self.next_out - 1,
                    e => e.min(self.next_out - 1),
                };
                self.resend(begin, end, now, out);
            }
            msg_type::SEQUENCE_RESET => {
                if let Some(new) = v.get_u64(tags::NEW_SEQ_NO)
                    && new > self.next_in
                {
                    self.next_in = new;
                    self.persist();
                }
            }
            msg_type::REJECT => out.push(Output::Event(SessionEvent::Reject(v.to_message()))),
            msg_type::LOGOUT => {
                let text = v.get(tags::TEXT).unwrap_or("").to_string();
                if self.state != State::LogoutSent {
                    out.push(Output::Send(self.logout("", now)));
                }
                self.state = State::Closed;
                out.push(Output::Event(SessionEvent::LoggedOut(text.clone())));
                out.push(Output::Disconnect(if text.is_empty() {
                    "logout".into()
                } else {
                    text
                }));
            }
            _ => match &self.cfg.app_msg_types {
                Some(known) if !known.iter().any(|k| k == t) => {
                    let mut m = Message::new(msg_type::BUSINESS_MESSAGE_REJECT);
                    m.push(tags::REF_SEQ_NUM, seq)
                        .push(tags::REF_MSG_TYPE, t)
                        .push(tags::BUSINESS_REJECT_REASON, 3)
                        .push(tags::TEXT, format!("Unsupported Message Type {t}"));
                    out.push(Output::Send(self.send(m, now)));
                }
                _ => out.push(Output::App(v.to_message())),
            },
        }
    }

    fn resend(&mut self, begin: u64, end: u64, now: Instant, out: &mut Vec<Output>) {
        let ts = utc_timestamp(SystemTime::now());
        let mut gap_start: Option<u64> = None;
        let mut seq = begin;
        while seq <= end {
            match self.sent.get(&seq) {
                Some((msg, orig)) => {
                    if let Some(g) = gap_start.take() {
                        out.push(Output::Send(self.gap_fill(g, seq, &ts)));
                    }
                    let bytes = self.encode(msg, seq, &ts, Some(orig));
                    out.push(Output::Send(bytes));
                }
                None => {
                    gap_start.get_or_insert(seq);
                }
            }
            seq += 1;
        }
        if let Some(g) = gap_start {
            out.push(Output::Send(self.gap_fill(g, end + 1, &ts)));
        }
        self.last_sent = now;
    }

    fn gap_fill(&self, seq: u64, new_seq: u64, ts: &str) -> Vec<u8> {
        let mut m = Message::new(msg_type::SEQUENCE_RESET);
        m.push(tags::GAP_FILL_FLAG, "Y")
            .push(tags::NEW_SEQ_NO, new_seq);
        self.encode(&m, seq, ts, Some(ts))
    }

    /// Call about once a second: sends heartbeats and test requests and
    /// detects a silent counterparty.
    pub fn on_timer(&mut self, now: Instant) -> Vec<Output> {
        let mut out = Vec::new();
        let hb = self.cfg.heartbeat_interval;
        match self.state {
            State::Active => {}
            // A Logon or Logout that gets no reply within one heartbeat
            // interval ends the session; otherwise it would wait for ever.
            State::LogonSent | State::LogoutSent if now.duration_since(self.state_since) >= hb => {
                let what = if self.state == State::LogonSent {
                    "Logon"
                } else {
                    "Logout"
                };
                self.state = State::Closed;
                out.push(Output::Disconnect(format!(
                    "no reply to {what} within {hb:?}"
                )));
                return out;
            }
            _ => return out,
        }
        // Without this, every later message would wait in `queued` for ever.
        if self
            .resend_progress
            .is_some_and(|at| now.duration_since(at) >= hb)
        {
            self.state = State::Closed;
            out.push(Output::Disconnect(format!(
                "no answer to ResendRequest within {hb:?}"
            )));
            return out;
        }
        if let Some((_, sent_at)) = &self.test_request {
            if now.duration_since(*sent_at) >= hb {
                self.state = State::Closed;
                out.push(Output::Disconnect(format!(
                    "no answer to TestRequest within {hb:?}"
                )));
                return out;
            }
        } else if now.duration_since(self.last_recv) >= hb + hb / 5 {
            self.test_counter += 1;
            let id = format!("TEST{}", self.test_counter);
            let mut m = Message::new(msg_type::TEST_REQUEST);
            m.push(tags::TEST_REQ_ID, &id);
            out.push(Output::Send(self.send(m, now)));
            self.test_request = Some((id, now));
        }
        if now.duration_since(self.last_sent) >= hb {
            out.push(Output::Send(
                self.send(Message::new(msg_type::HEARTBEAT), now),
            ));
        }
        out
    }
}
