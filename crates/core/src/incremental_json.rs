//! Streaming incremental JSON parser for tool call arguments.
//!
//! The `StreamingArgsParser` is a character-level state machine that parses
//! LLM-streamed tool-call JSON as it arrives byte-by-byte. It emits two kinds
//! of events:
//!
//! - `ArgsEvent::Parsed` — a parameter value is complete (e.g. `"id": "abc"`)
//! - `ArgsEvent::Progress` — a parameter is still arriving, carrying the latest
//!   content fragment so the UI can render partial progress.
//!
//! # Performance
//!
//! The parser maintains a `scan_pos` cursor. Each `poll()` call resumes scanning
//! from the last position, so every byte is visited exactly once → **O(n) total**.
//! No re-parsing, no back-tracking, no `serde_json::from_str` retry loops.

use std::collections::HashSet;

use serde_json::Value;

// ── Public Event type ────────────────────────────────────────────────

/// An event produced by the streaming parser.
#[derive(Debug, Clone)]
pub enum ArgsEvent {
    /// A parameter value has been fully received and parsed.
    Parsed {
        id: String,
        name: String,
        value: Value,
    },
    /// A string parameter is still arriving; `value` contains the latest
    /// content fragment, `received` is the cumulative byte count (after the
    /// opening quote).
    Progress {
        id: String,
        name: String,
        received: usize,
        value: Value,
    },
}

// ── Parser state ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Waiting for `{` or the next key `"`.
    ExpectKey,
    /// Inside a key string (reading until the closing `"`).
    InKey,
    /// Key is complete, waiting for `:`.
    ExpectColon,
    /// Waiting for the start of a value.
    ExpectValue,
    /// Inside a string value.
    InString,
    /// Inside a number value.
    InNumber,
    /// Inside a literal (`true`, `false`, `null`).
    InLiteral,
    /// Inside a nested object `{...}`.
    InObject,
    /// Inside a nested array `[...]`.
    InArray,
    /// A key-value pair is done; waiting for `,` or `}`.
    ExpectComma,
}

// ── Parser ───────────────────────────────────────────────────────────

/// Character-level streaming JSON parser for tool-call arguments.
///
/// # Usage
///
/// ```ignore
/// let mut parser = StreamingArgsParser::new();
/// parser.push_bytes(delta);
/// for event in parser.poll(tool_call_id) {
///     // forward event to UI / channel
/// }
/// ```
pub struct StreamingArgsParser {
    /// The full accumulated JSON buffer (only appended to, never truncated).
    buffer: Vec<u8>,
    /// Next byte position to scan. Resumes from here on each `poll()`.
    scan_pos: usize,
    /// Current parser state.
    state: State,
    /// Current parameter key name (fully parsed).
    current_key: Option<String>,
    /// Byte offset of the opening `"` of the current key (or value start for non-strings).
    value_start: usize,
    /// The brace depth at which we entered an InObject / InArray value.
    /// When brace_depth returns to this level the nested value is complete.
    entry_depth: usize,
    /// Current brace (`{`/`}`) + bracket (`[`/`]`) nesting depth.
    brace_depth: usize,
    /// Current bracket-only depth inside InArray. Tracks when array closes.
    bracket_depth: usize,
    /// Whether the previous character was a backslash (string escape).
    escape: bool,
    /// Parameter keys that have already been emitted as `Parsed`. Prevents
    /// duplicate emissions for the same key.
    emitted_keys: HashSet<String>,
    /// The cumulative byte count of the last `Progress` event that was emitted
    /// for the current string value. Used to skip redundant emissions when no
    /// new bytes have arrived.
    last_progress_received: usize,
}

impl StreamingArgsParser {
    /// Create a fresh parser for a new tool call.
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            scan_pos: 0,
            state: State::ExpectKey,
            current_key: None,
            value_start: 0,
            entry_depth: 0,
            brace_depth: 0,
            bracket_depth: 0,
            escape: false,
            emitted_keys: HashSet::new(),
            last_progress_received: 0,
        }
    }

    /// Append raw bytes that arrived from the LLM stream.
    pub fn push_bytes(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    /// Poll the parser for events. Call this after every `push_bytes()`.
    ///
    /// Returns all newly-discovered events (empty if nothing changed).
    pub fn poll(&mut self, tool_call_id: &str) -> Vec<ArgsEvent> {
        let mut events = Vec::new();

        while self.scan_pos < self.buffer.len() {
            let ch = self.buffer[self.scan_pos];

            // Fast-path: skip whitespace regardless of state (except InString
            // where whitespace is content).
            let skip_ws = !matches!(self.state, State::InString);
            if skip_ws && ch.is_ascii_whitespace() {
                self.scan_pos += 1;
                continue;
            }

            match self.state {
                // ── ExpectKey ────────────────────────────────────────
                State::ExpectKey => match ch {
                    b'{' => {
                        self.brace_depth += 1;
                        self.scan_pos += 1;
                    }
                    b'}' => {
                        self.brace_depth = self.brace_depth.saturating_sub(1);
                        self.state = State::ExpectComma;
                        self.scan_pos += 1;
                    }
                    b'"' => {
                        self.value_start = self.scan_pos + 1;
                        self.state = State::InKey;
                        self.scan_pos += 1;
                    }
                    _ => {
                        self.scan_pos += 1;
                    }
                },

                // ── InKey ────────────────────────────────────────────
                State::InKey => {
                    if ch == b'\\' {
                        self.escape = !self.escape;
                    } else if ch == b'"' && !self.escape {
                        let key_bytes = &self.buffer[self.value_start..self.scan_pos];
                        self.current_key =
                            Some(String::from_utf8_lossy(key_bytes).into_owned());
                        self.state = State::ExpectColon;
                    } else {
                        self.escape = false;
                    }
                    self.scan_pos += 1;
                }

                // ── ExpectColon ──────────────────────────────────────
                State::ExpectColon => {
                    if ch == b':' {
                        self.state = State::ExpectValue;
                    }
                    self.scan_pos += 1;
                }

                // ── ExpectValue ──────────────────────────────────────
                State::ExpectValue => match ch {
                    b'"' => {
                        self.value_start = self.scan_pos + 1;
                        self.last_progress_received = 0;
                        self.escape = false;
                        self.state = State::InString;
                        self.scan_pos += 1;
                    }
                    b'{' => {
                        self.value_start = self.scan_pos;
                        self.entry_depth = self.brace_depth;
                        self.brace_depth += 1;
                        self.state = State::InObject;
                        self.scan_pos += 1;
                    }
                    b'[' => {
                        self.value_start = self.scan_pos;
                        self.entry_depth = self.brace_depth;
                        self.bracket_depth = 1;
                        self.state = State::InArray;
                        self.scan_pos += 1;
                    }
                    b'-' | b'0'..=b'9' => {
                        self.value_start = self.scan_pos;
                        self.state = State::InNumber;
                        self.scan_pos += 1;
                    }
                    b't' | b'f' | b'n' => {
                        self.value_start = self.scan_pos;
                        self.state = State::InLiteral;
                        self.scan_pos += 1;
                    }
                    _ => {
                        self.scan_pos += 1;
                    }
                },

                // ── InString (the hot path for long content) ─────────
                State::InString => {
                    if ch == b'\\' {
                        self.escape = !self.escape;
                        self.scan_pos += 1;
                        continue;
                    }
                    self.scan_pos += 1;

                    if ch == b'"' && !self.escape {
                        Self::emit_parsed_str(
                            &self.current_key,
                            &mut self.emitted_keys,
                            &self.buffer[self.value_start..self.scan_pos - 1],
                            tool_call_id,
                            &mut events,
                        );
                        self.last_progress_received = 0;
                        self.state = State::ExpectComma;
                        continue;
                    }
                    self.escape = false;

                    Self::emit_progress(
                        &self.buffer,
                        &self.current_key,
                        &self.emitted_keys,
                        self.value_start,
                        self.scan_pos,
                        &mut self.last_progress_received,
                        tool_call_id,
                        &mut events,
                    );
                }

                // ── InNumber ─────────────────────────────────────────
                State::InNumber => {
                    if ch == b',' || ch == b'}' || ch.is_ascii_whitespace() {
                        let raw = &self.buffer[self.value_start..self.scan_pos];
                        Self::emit_parsed_literal(
                            &self.current_key,
                            &mut self.emitted_keys,
                            raw,
                            tool_call_id,
                            &mut events,
                        );
                        self.transition_after(ch);
                    } else {
                        self.scan_pos += 1;
                    }
                }

                // ── InLiteral ────────────────────────────────────────
                State::InLiteral => {
                    if ch == b',' || ch == b'}' || ch.is_ascii_whitespace() {
                        let raw = &self.buffer[self.value_start..self.scan_pos];
                        Self::emit_parsed_literal(
                            &self.current_key,
                            &mut self.emitted_keys,
                            raw,
                            tool_call_id,
                            &mut events,
                        );
                        self.transition_after(ch);
                    } else {
                        self.scan_pos += 1;
                    }
                }

                // ── InObject ─────────────────────────────────────────
                State::InObject => match ch {
                    b'{' => {
                        self.brace_depth += 1;
                        self.scan_pos += 1;
                    }
                    b'}' => {
                        self.brace_depth = self.brace_depth.saturating_sub(1);
                        if self.brace_depth == self.entry_depth {
                            Self::emit_nested_parsed(
                                &self.buffer,
                                &self.current_key,
                                &mut self.emitted_keys,
                                self.value_start,
                                self.scan_pos,
                                tool_call_id,
                                &mut events,
                            );
                            self.state = State::ExpectComma;
                        }
                        self.scan_pos += 1;
                    }
                    _ => {
                        self.scan_pos += 1;
                    }
                },

                // ── InArray ──────────────────────────────────────────
                State::InArray => match ch {
                    b'[' => {
                        self.bracket_depth += 1;
                        self.scan_pos += 1;
                    }
                    b']' => {
                        self.bracket_depth = self.bracket_depth.saturating_sub(1);
                        if self.bracket_depth == 0 {
                            Self::emit_nested_parsed(
                                &self.buffer,
                                &self.current_key,
                                &mut self.emitted_keys,
                                self.value_start,
                                self.scan_pos,
                                tool_call_id,
                                &mut events,
                            );
                            self.state = State::ExpectComma;
                        }
                        self.scan_pos += 1;
                    }
                    _ => {
                        self.scan_pos += 1;
                    }
                },

                // ── ExpectComma ──────────────────────────────────────
                State::ExpectComma => match ch {
                    b',' => {
                        self.current_key = None;
                        self.state = State::ExpectKey;
                        self.scan_pos += 1;
                    }
                    b'}' => {
                        self.scan_pos += 1;
                    }
                    _ => {
                        self.scan_pos += 1;
                    }
                },
            }
        }

        events
    }

    // ── helpers ──────────────────────────────────────────────────────

    fn emit_parsed_str(
        key: &Option<String>,
        emitted_keys: &mut HashSet<String>,
        value_bytes: &[u8],
        tool_call_id: &str,
        events: &mut Vec<ArgsEvent>,
    ) {
        let k = match key.as_ref() {
            Some(k) => k,
            None => return,
        };
        if emitted_keys.contains(k) {
            return;
        }
        let mut full = Vec::with_capacity(value_bytes.len() + 2);
        full.push(b'"');
        full.extend_from_slice(value_bytes);
        full.push(b'"');
        let value = serde_json::from_slice(&full)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(value_bytes).into_owned()));
        emitted_keys.insert(k.clone());
        events.push(ArgsEvent::Parsed {
            id: tool_call_id.to_string(),
            name: k.clone(),
            value,
        });
    }

    fn emit_parsed_literal(
        key: &Option<String>,
        emitted_keys: &mut HashSet<String>,
        raw: &[u8],
        tool_call_id: &str,
        events: &mut Vec<ArgsEvent>,
    ) {
        let k = match key.as_ref() {
            Some(k) => k,
            None => return,
        };
        if emitted_keys.contains(k) {
            return;
        }
        let value: Value = serde_json::from_slice(raw)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(raw).into_owned()));
        emitted_keys.insert(k.clone());
        events.push(ArgsEvent::Parsed {
            id: tool_call_id.to_string(),
            name: k.clone(),
            value,
        });
    }

    fn emit_nested_parsed(
        buffer: &[u8],
        key: &Option<String>,
        emitted_keys: &mut HashSet<String>,
        value_start: usize,
        end: usize,
        tool_call_id: &str,
        events: &mut Vec<ArgsEvent>,
    ) {
        let k = match key.as_ref() {
            Some(k) => k,
            None => return,
        };
        if emitted_keys.contains(k) {
            return;
        }
        let raw = &buffer[value_start..end + 1];
        let value: Value = serde_json::from_slice(raw)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(raw).into_owned()));
        emitted_keys.insert(k.clone());
        events.push(ArgsEvent::Parsed {
            id: tool_call_id.to_string(),
            name: k.clone(),
            value,
        });
    }

    fn emit_progress(
        buffer: &[u8],
        key: &Option<String>,
        emitted_keys: &HashSet<String>,
        value_start: usize,
        scan_pos: usize,
        last_progress: &mut usize,
        tool_call_id: &str,
        events: &mut Vec<ArgsEvent>,
    ) {
        let k = match key.as_ref() {
            Some(k) => k,
            None => return,
        };
        if emitted_keys.contains(k) {
            return;
        }
        let received = scan_pos.saturating_sub(value_start);
        if received <= *last_progress {
            return;
        }
        *last_progress = received;

        let fragment = &buffer[value_start..scan_pos];
        let fragment_str = String::from_utf8_lossy(fragment).into_owned();
        events.push(ArgsEvent::Progress {
            id: tool_call_id.to_string(),
            name: k.clone(),
            received,
            value: Value::String(fragment_str),
        });
    }

    fn transition_after(&mut self, ch: u8) {
        self.state = match ch {
            b'}' => {
                self.brace_depth = self.brace_depth.saturating_sub(1);
                State::ExpectComma
            }
            b',' => State::ExpectKey,
            _ => State::ExpectComma,
        };
        self.scan_pos += 1;
    }
}

impl Default for StreamingArgsParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect_parsed(parser: &mut StreamingArgsParser, id: &str) -> Vec<(String, Value)> {
        parser
            .poll(id)
            .into_iter()
            .filter_map(|e| match e {
                ArgsEvent::Parsed { name, value, .. } => Some((name, value)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn full_object_single_param() {
        let mut p = StreamingArgsParser::new();
        p.push_bytes(b"{\"path\":\"/tmp/a.txt\"}");
        let parsed = collect_parsed(&mut p, "c1");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].0, "path");
        assert_eq!(parsed[0].1, Value::String("/tmp/a.txt".into()));
    }

    #[test]
    fn full_object_multi_params() {
        let mut p = StreamingArgsParser::new();
        p.push_bytes(b"{\"a\":42,\"b\":true,\"c\":\"hello\"}");
        let parsed = collect_parsed(&mut p, "c1");
        assert_eq!(parsed.len(), 3);
    }

    #[test]
    fn streaming_progress() {
        let mut p = StreamingArgsParser::new();
        p.push_bytes(b"{\"text\":\"Hel");
        let events = p.poll("c1");
        let has_progress = events.iter().any(|e| matches!(e, ArgsEvent::Progress { .. }));
        assert!(has_progress, "should emit progress for partial string");

        p.push_bytes(b"lo World\"}");
        let parsed = collect_parsed(&mut p, "c1");
        assert_eq!(parsed[0].1, Value::String("Hello World".into()));
    }

    #[test]
    fn streaming_interleaved() {
        let mut p = StreamingArgsParser::new();
        p.push_bytes(b"{\"code\":\"fn main(");
        let _ = p.poll("c1");
        p.push_bytes(b") {}\"}");
        let parsed = collect_parsed(&mut p, "c1");
        assert_eq!(parsed[0].1, Value::String("fn main() {}".into()));
    }
}
