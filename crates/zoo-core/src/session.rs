//! Bounded park-session recorder (#87, Atlas 4.1.1-4.1.5).
//!
//! The recorder keeps the starting-state identity, the ordered authoritative command log
//! with each result, recent domain events, work counters and detected simulation failures.
//! A session is reproduced from its starting identity plus the ordered log; checkpoints
//! wait for the versioned save format (#88). Memory stays bounded by the two rings.
use serde_json::{Value, json};
use std::collections::VecDeque;

pub(crate) const MAX_COMMANDS: usize = 2_000;
pub(crate) const MAX_EVENTS: usize = 500;
pub(crate) const MAX_EXPORT_BYTES: usize = 1_048_576;
/// The Rust bundle stays below this so the browser export (bundle plus at most 50 bounded
/// browser failure records) also fits inside `MAX_EXPORT_BYTES`.
const BUNDLE_BUDGET_BYTES: usize = 900_000;
const MAX_ARG_STRING_BYTES: usize = 256;
pub(crate) const FROZEN_MESSAGE: &str = "Simulation frozen after a detected failure; export diagnostics or start a new park";

#[derive(Clone, Debug)]
pub(crate) struct DomainEvent {
    pub tick: u64,
    pub kind: &'static str,
    pub entity_id: Option<u32>,
}

#[derive(Clone, Debug)]
struct CommandEntry {
    seq: u64,
    tick: u64,
    kind: &'static str,
    args: Value,
    ok: bool,
    message: String,
}

#[derive(Clone, Debug)]
struct EventEntry {
    seq: u64,
    event: DomainEvent,
}

#[derive(Clone, Debug)]
struct Failure {
    seq: u64,
    tick: u64,
    kind: &'static str,
    message: String,
    injected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PendingFault {
    Panic,
}

#[derive(Clone, Debug)]
pub(crate) struct Session {
    start: Value,
    commands: VecDeque<CommandEntry>,
    commands_total: u64,
    events: VecDeque<EventEntry>,
    events_total: u64,
    failures: Vec<Failure>,
    pub frozen: bool,
    pub pending_fault: Option<PendingFault>,
    pub injected_corruption: bool,
}

/// Deterministic FNV-1a 64-bit digest; identifies content, it is not a security hash.
fn fnv1a64(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("fnv1a64:{hash:016x}")
}

pub(crate) fn start_identity(content_json: &str, ruleset_id: &str, schema_version: u32) -> Value {
    json!({
        "scenario": "default",
        "ruleset_id": ruleset_id,
        "schema_version": schema_version,
        "content_digest": fnv1a64(content_json.as_bytes()),
    })
}

impl Session {
    pub fn new(start: Value) -> Self {
        Self {
            start,
            commands: VecDeque::new(),
            commands_total: 0,
            events: VecDeque::new(),
            events_total: 0,
            failures: Vec::new(),
            frozen: false,
            pending_fault: None,
            injected_corruption: false,
        }
    }

    pub fn start(&self) -> &Value {
        &self.start
    }

    /// Appends an authoritative command and returns its sequence number.
    pub fn record(&mut self, tick: u64, kind: &'static str, args: Value, ok: bool, message: &str) -> u64 {
        let seq = self.commands_total;
        self.commands_total += 1;
        self.commands.push_back(CommandEntry {
            seq,
            tick,
            kind,
            args,
            ok,
            message: message.to_owned(),
        });
        if self.commands.len() > MAX_COMMANDS {
            self.commands.pop_front();
        }
        seq
    }

    /// Consecutive successful advances are merged into one entry so the browser's 4 Hz
    /// tick cannot exhaust the command ring. Replaying the summed minutes is identical,
    /// because the simulation advances minute by minute and the invariant check passed at
    /// the start of every merged advance.
    pub fn record_advance(&mut self, tick: u64, minutes: u64) {
        if let Some(last) = self.commands.back_mut()
            && last.kind == "advance"
            && last.ok
            && last.seq + 1 == self.commands_total
        {
            let total = last.args["minutes"].as_u64().unwrap_or(0) + minutes;
            last.args = json!({ "minutes": total });
            last.message = advance_message(total);
            return;
        }
        self.record(tick, "advance", json!({ "minutes": minutes }), true, &advance_message(minutes));
    }

    pub fn fail(&mut self, seq: u64, tick: u64, kind: &'static str, message: String, injected: bool) {
        // Exactly one failure per session: a frozen session never re-detects.
        if self.frozen {
            return;
        }
        self.frozen = true;
        self.failures.push(Failure {
            seq,
            tick,
            kind,
            message,
            injected,
        });
    }

    pub fn push_events(&mut self, events: impl IntoIterator<Item = DomainEvent>) {
        for event in events {
            let seq = self.events_total;
            self.events_total += 1;
            self.events.push_back(EventEntry { seq, event });
            if self.events.len() > MAX_EVENTS {
                self.events.pop_front();
            }
        }
    }

    pub fn bundle(&self, clock: Value, entities: Value, work: Value) -> String {
        let commands: Vec<Value> = self.commands.iter().map(command_value).collect();
        let events: Vec<Value> = self
            .events
            .iter()
            .map(|entry| {
                json!({
                    "seq": entry.seq,
                    "tick": entry.event.tick,
                    "kind": entry.event.kind,
                    "entity_id": entry.event.entity_id,
                })
            })
            .collect();
        let failures: Vec<Value> = self
            .failures
            .iter()
            .map(|failure| {
                json!({
                    "seq": failure.seq,
                    "tick": failure.tick,
                    "source": "simulation",
                    "kind": failure.kind,
                    "message": failure.message,
                    "injected": failure.injected,
                })
            })
            .collect();
        let mut bundle = json!({
            "format": "zoo-diagnostics/1",
            "build": { "zoo_core_version": env!("CARGO_PKG_VERSION") },
            "start": self.start,
            "status": if self.frozen { "frozen" } else { "running" },
            "clock": clock,
            "limits": {
                "max_commands": MAX_COMMANDS,
                "max_events": MAX_EVENTS,
                "max_export_bytes": MAX_EXPORT_BYTES,
            },
            "commands": commands,
            "commands_total": self.commands_total,
            "commands_dropped": self.commands_total - self.commands.len() as u64,
            "events": events,
            "events_total": self.events_total,
            "events_dropped": self.events_total - self.events.len() as u64,
            "entities": entities,
            "work": work,
            "failures": failures,
            "truncated": false,
        });
        fit_budget(&mut bundle);
        serde_json::to_string(&bundle).expect("diagnostics serialization is infallible")
    }

    /// The bundle's command log in replay order.
    pub fn parse_replay_log(bundle: &Value) -> Result<Vec<(String, Value, bool, String)>, String> {
        if bundle["format"] != "zoo-diagnostics/1" {
            return Err("not a zoo-diagnostics/1 bundle".to_owned());
        }
        if bundle["truncated"] != false {
            return Err("the bundle was truncated for export and cannot be replayed exactly".to_owned());
        }
        if bundle["commands_dropped"] != 0 {
            return Err("the oldest commands were dropped; the session cannot be replayed from its initial state".to_owned());
        }
        let commands = bundle["commands"].as_array().ok_or("bundle has no command log")?;
        let mut log = Vec::with_capacity(commands.len());
        for (index, entry) in commands.iter().enumerate() {
            if entry["seq"].as_u64() != Some(index as u64) {
                return Err(format!("command log is not contiguous from seq 0 at entry {index}"));
            }
            let kind = entry["kind"].as_str().ok_or("command without kind")?;
            let ok = entry["result"]["ok"].as_bool().ok_or("command without result")?;
            let message = entry["result"]["message"].as_str().ok_or("command without result message")?;
            log.push((kind.to_owned(), entry["args"].clone(), ok, message.to_owned()));
        }
        Ok(log)
    }
}

pub(crate) fn advance_message(minutes: u64) -> String {
    format!("Advanced {minutes} simulated minutes")
}

fn command_value(entry: &CommandEntry) -> Value {
    json!({
        "seq": entry.seq,
        "tick": entry.tick,
        "kind": entry.kind,
        "args": entry.args,
        "result": { "ok": entry.ok, "message": entry.message },
    })
}

fn encoded_len(value: &Value) -> usize {
    serde_json::to_string(value).map_or(usize::MAX, |text| text.len())
}

/// Keeps the export within budget: shorten long argument strings first, then drop the
/// oldest events and commands, always keeping the latest command. Any change is declared
/// with `truncated: true`.
fn fit_budget(bundle: &mut Value) {
    if encoded_len(bundle) <= BUNDLE_BUDGET_BYTES {
        return;
    }
    bundle["truncated"] = json!(true);
    if let Some(commands) = bundle["commands"].as_array_mut() {
        for command in commands {
            if let Some(args) = command["args"].as_object_mut() {
                for value in args.values_mut() {
                    if let Some(text) = value.as_str()
                        && text.len() > MAX_ARG_STRING_BYTES
                    {
                        let mut cut = MAX_ARG_STRING_BYTES;
                        while !text.is_char_boundary(cut) {
                            cut -= 1;
                        }
                        *value = json!(format!("{}…", &text[..cut]));
                    }
                }
            }
        }
    }
    while encoded_len(bundle) > BUNDLE_BUDGET_BYTES {
        let events = bundle["events"].as_array().map_or(0, Vec::len);
        let commands = bundle["commands"].as_array().map_or(0, Vec::len);
        if events > 0 {
            let drop = events.div_ceil(2);
            bundle["events"].as_array_mut().expect("events").drain(..drop);
        } else if commands > 1 {
            let drop = (commands - 1).div_ceil(2);
            bundle["commands"].as_array_mut().expect("commands").drain(..drop);
        } else {
            break;
        }
    }
    let kept_commands = bundle["commands"].as_array().map_or(0, Vec::len) as u64;
    let kept_events = bundle["events"].as_array().map_or(0, Vec::len) as u64;
    let commands_total = bundle["commands_total"].as_u64().unwrap_or(0);
    let events_total = bundle["events_total"].as_u64().unwrap_or(0);
    bundle["commands_dropped"] = json!(commands_total - kept_commands);
    bundle["events_dropped"] = json!(events_total - kept_events);
}
