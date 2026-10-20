// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sato
// SPDX-License-Identifier: GPL-2.0-or-later

//! The log an extension may write to, and the budget it gets.
//!
//! `rx_log` is the one custom import of API version 1 (see
//! `docs/extensions/API.md` section 4). It is a debugging aid for people
//! writing extensions, so it is rate-limited rather than trusted: a module that
//! logs in a loop loses its later messages instead of filling the host's
//! memory, and a message that is not valid UTF-8 is dropped.

use crate::limits::ProcessLimits;
use rustacity_ext_api::abi;

/// One message an extension wrote through `rx_log`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRecord {
    /// 0 trace, 1 debug, 2 info, 3 warn, 4 error.
    pub level: i32,
    pub message: String,
}

impl LogRecord {
    /// The level as the word the documentation uses.
    pub fn level_name(&self) -> &'static str {
        match self.level {
            0 => "trace",
            1 => "debug",
            2 => "info",
            3 => "warn",
            _ => "error",
        }
    }
}

/// The budget and the collection, per instance.
pub(crate) struct LogCollector {
    capacity: usize,
    max_messages: usize,
    max_bytes: usize,
    max_message_bytes: usize,
    records: Vec<LogRecord>,
    messages_this_call: usize,
    bytes_this_call: usize,
    dropped: u64,
    truncated: u64,
}

impl LogCollector {
    pub(crate) fn new(capacity: usize, limits: &ProcessLimits) -> Self {
        Self {
            capacity,
            max_messages: limits.max_log_messages_per_call,
            max_bytes: limits.max_log_bytes_per_call,
            max_message_bytes: limits.max_log_message_bytes,
            records: Vec::new(),
            messages_this_call: 0,
            bytes_this_call: 0,
            dropped: 0,
            truncated: 0,
        }
    }

    /// Refills the per-call budget. The records themselves stay until they are
    /// collected, so the user can read what happened during the whole run.
    pub(crate) fn begin_call(&mut self) {
        self.messages_this_call = 0;
        self.bytes_this_call = 0;
    }

    /// Takes one message from the extension, dropping what it cannot keep.
    pub(crate) fn push(&mut self, level: i32, bytes: &[u8]) {
        if self.messages_this_call >= self.max_messages || self.bytes_this_call >= self.max_bytes {
            self.dropped += 1;
            return;
        }
        let Ok(text) = std::str::from_utf8(bytes) else {
            // Not text: the host will not guess what it was.
            self.dropped += 1;
            return;
        };
        self.messages_this_call += 1;
        self.bytes_this_call += text.len();

        let text = if text.len() > self.max_message_bytes {
            self.truncated += 1;
            &text[..floor_char_boundary(text, self.max_message_bytes)]
        } else {
            text
        };

        if self.records.len() >= self.capacity {
            self.dropped += 1;
            return;
        }
        self.records.push(LogRecord {
            level: if (0..=4).contains(&level) {
                level
            } else {
                abi::LOG_LEVEL_DEFAULT
            },
            message: text.to_string(),
        });
    }

    /// The records collected so far, emptying the collection.
    pub(crate) fn drain(&mut self) -> Vec<LogRecord> {
        std::mem::take(&mut self.records)
    }

    pub(crate) fn dropped(&self) -> u64 {
        self.dropped
    }

    pub(crate) fn truncated(&self) -> u64 {
        self.truncated
    }
}

/// The largest byte index at or below `limit` that does not split a character.
fn floor_char_boundary(text: &str, limit: usize) -> usize {
    let mut end = limit.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collector() -> LogCollector {
        LogCollector::new(8, &ProcessLimits::default())
    }

    #[test]
    fn a_message_is_kept_with_its_level() {
        let mut log = collector();
        log.begin_call();
        log.push(3, b"careful");
        assert_eq!(
            log.drain(),
            [LogRecord {
                level: 3,
                message: "careful".to_string()
            }]
        );
    }

    #[test]
    fn a_level_out_of_range_becomes_info() {
        let mut log = collector();
        log.begin_call();
        log.push(99, b"hello");
        assert_eq!(log.drain()[0].level, abi::LOG_LEVEL_DEFAULT);
    }

    #[test]
    fn text_that_is_not_utf8_is_dropped() {
        let mut log = collector();
        log.begin_call();
        log.push(2, &[0xff, 0xfe]);
        assert!(log.drain().is_empty());
        assert_eq!(log.dropped(), 1);
    }

    #[test]
    fn the_budget_is_per_call_and_counts_messages() {
        // Room for the whole budget, so that what is dropped is dropped for
        // want of budget and not for want of room.
        let mut log = LogCollector::new(abi::MAX_LOG_MESSAGES_PER_CALL, &ProcessLimits::default());
        for _ in 0..(abi::MAX_LOG_MESSAGES_PER_CALL + 10) {
            log.push(2, b"one");
        }
        assert_eq!(log.drain().len(), abi::MAX_LOG_MESSAGES_PER_CALL);
        assert_eq!(log.dropped(), 10);

        // A new call gets a new budget, so the run as a whole keeps logging.
        log.begin_call();
        log.push(2, b"one");
        assert_eq!(log.drain().len(), 1);
    }

    #[test]
    fn a_long_message_is_truncated_at_a_character_boundary() {
        let mut log = collector();
        log.begin_call();
        // Three-byte characters, so the limit lands inside one.
        let message = "\u{4e00}".repeat(abi::MAX_LOG_MESSAGE_BYTES);
        log.push(2, message.as_bytes());
        let kept = log.drain();
        assert!(kept[0].message.len() <= abi::MAX_LOG_MESSAGE_BYTES);
        assert!(kept[0].message.chars().all(|c| c == '\u{4e00}'));
        assert_eq!(log.truncated(), 1);
    }

    #[test]
    fn the_collection_stops_at_its_capacity() {
        let mut log = LogCollector::new(2, &ProcessLimits::default());
        log.begin_call();
        for _ in 0..5 {
            log.push(2, b"hello");
        }
        assert_eq!(log.drain().len(), 2);
        assert_eq!(log.dropped(), 3);
    }
}
