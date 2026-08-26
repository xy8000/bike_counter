//! Core policy for provider-emitted messages: severity filtering and a
//! per-data-source cap.
//!
//! The driven adapter hands providers a scoped
//! [`ProviderMessageSink`](super::provider_port::ProviderMessageSink) that
//! writes straight through to the store. The core wraps that sink in
//! [`FilteringProviderMessageSink`] so it — not the adapter or the database —
//! decides which messages are worth persisting:
//!
//! - messages below the provider's configured `log_level` are dropped, and
//! - at most [`MAX_PROVIDER_MESSAGES`] events are persisted per data source;
//!   the next event is dropped and a single truncation `WARNING` is recorded
//!   (and printed to stdout) so the limit is observable.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::provider_message::ProviderMessageSeverity;
use super::provider_port::{ProviderError, ProviderMessageSink};

/// Maximum number of provider messages persisted per data source. Once reached,
/// further events are dropped and a single truncation warning is recorded, so at
/// most `MAX_PROVIDER_MESSAGES + 1` rows exist per data source (the `+1` is the
/// truncation warning itself).
pub const MAX_PROVIDER_MESSAGES: usize = 1000;

/// Wraps a scoped sink to enforce the provider `log_level` and the per-data-
/// source message cap. Thread-safe: the atomic counters make the level/cap
/// policy hold under concurrent emits.
pub struct FilteringProviderMessageSink {
    inner: Arc<dyn ProviderMessageSink + Send + Sync>,
    min_level: ProviderMessageSeverity,
    max_messages: usize,
    /// Number of events admitted past the level filter (including dropped
    /// over-cap events), so the cap is global per sink/data source.
    recorded: AtomicUsize,
    /// Whether the truncation warning has already been reported.
    truncated_reported: AtomicBool,
}

impl FilteringProviderMessageSink {
    pub fn new(
        inner: Arc<dyn ProviderMessageSink + Send + Sync>,
        min_level: ProviderMessageSeverity,
        max_messages: usize,
    ) -> Self {
        Self {
            inner,
            min_level,
            max_messages,
            recorded: AtomicUsize::new(0),
            truncated_reported: AtomicBool::new(false),
        }
    }
}

impl ProviderMessageSink for FilteringProviderMessageSink {
    fn provider_event_occurred(
        &self,
        severity: ProviderMessageSeverity,
        message: &str,
    ) -> Result<(), ProviderError> {
        // Drop events below the configured log level.
        if !severity.at_or_above(self.min_level) {
            return Ok(());
        }

        // Cap: persist the first `max_messages` events.
        let slot = self.recorded.fetch_add(1, Ordering::SeqCst);
        if slot < self.max_messages {
            return self.inner.provider_event_occurred(severity, message);
        }

        // Over the cap: drop the event and report the truncation exactly once.
        // The warning is recorded directly through the inner sink (not counted
        // against the cap), so the data source ends up with `max_messages + 1`
        // rows at most.
        if self
            .truncated_reported
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            let notice = format!(
                "provider messages truncated after {max} events; further events were dropped",
                max = self.max_messages
            );
            println!("{notice}");
            self.inner
                .provider_event_occurred(ProviderMessageSeverity::Warning, &notice)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::core::domain::data_source::provider_message::ProviderMessageSeverity;

    /// In-memory sink recording every event it receives (the "store" side).
    #[derive(Default)]
    struct RecordingSink {
        events: Mutex<Vec<(ProviderMessageSeverity, String)>>,
    }

    impl ProviderMessageSink for RecordingSink {
        fn provider_event_occurred(
            &self,
            severity: ProviderMessageSeverity,
            message: &str,
        ) -> Result<(), ProviderError> {
            self.events
                .lock()
                .unwrap()
                .push((severity, message.to_string()));
            Ok(())
        }
    }

    fn sink(
        min_level: ProviderMessageSeverity,
        max_messages: usize,
    ) -> (FilteringProviderMessageSink, Arc<RecordingSink>) {
        let inner = Arc::new(RecordingSink::default());
        let filter = FilteringProviderMessageSink::new(inner.clone(), min_level, max_messages);
        (filter, inner)
    }

    #[test]
    fn drops_events_below_the_log_level() {
        let (filter, inner) = sink(ProviderMessageSeverity::Warning, MAX_PROVIDER_MESSAGES);

        filter
            .provider_event_occurred(ProviderMessageSeverity::Debug, "missing column")
            .unwrap();
        filter
            .provider_event_occurred(ProviderMessageSeverity::Info, "archive downloaded")
            .unwrap();
        assert!(
            inner.events.lock().unwrap().is_empty(),
            "DEBUG/INFO must be dropped at WARNING level"
        );

        filter
            .provider_event_occurred(ProviderMessageSeverity::Warning, "real problem")
            .unwrap();
        filter
            .provider_event_occurred(ProviderMessageSeverity::Error, "worse problem")
            .unwrap();

        let events = inner.events.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].0, ProviderMessageSeverity::Warning);
        assert_eq!(events[1].0, ProviderMessageSeverity::Error);
    }

    #[test]
    fn caps_messages_and_reports_truncation_once() {
        let (filter, inner) = sink(ProviderMessageSeverity::Trace, 3);

        // The first three events are persisted.
        for i in 0..3 {
            filter
                .provider_event_occurred(ProviderMessageSeverity::Info, &format!("event {i}"))
                .unwrap();
        }
        // The fourth and fifth events are dropped; the fourth triggers exactly
        // one truncation warning.
        filter
            .provider_event_occurred(ProviderMessageSeverity::Info, "event 3")
            .unwrap();
        filter
            .provider_event_occurred(ProviderMessageSeverity::Info, "event 4")
            .unwrap();

        let events = inner.events.lock().unwrap();
        // 3 normal events + exactly 1 truncation warning.
        assert_eq!(events.len(), 4);
        let normal: Vec<_> = events
            .iter()
            .filter(|(severity, message)| {
                *severity != ProviderMessageSeverity::Warning || !message.contains("truncated")
            })
            .collect();
        assert_eq!(normal.len(), 3);
        let truncation: Vec<_> = events
            .iter()
            .filter(|(severity, message)| {
                *severity == ProviderMessageSeverity::Warning && message.contains("truncated")
            })
            .collect();
        assert_eq!(truncation.len(), 1, "truncation reported exactly once");
        assert!(truncation[0].1.contains("after 3 events"));
    }

    #[test]
    fn keeps_max_messages_plus_one_truncation_warning() {
        // With the real cap, a data source must end up with at most
        // MAX_PROVIDER_MESSAGES + 1 rows (1000 normal + 1 truncation warning).
        let (filter, inner) = sink(ProviderMessageSeverity::Trace, MAX_PROVIDER_MESSAGES);
        for i in 0..(MAX_PROVIDER_MESSAGES + 5) {
            filter
                .provider_event_occurred(ProviderMessageSeverity::Info, &format!("event {i}"))
                .unwrap();
        }

        let events = inner.events.lock().unwrap();
        assert_eq!(events.len(), MAX_PROVIDER_MESSAGES + 1);
        let truncations = events
            .iter()
            .filter(|(severity, message)| {
                *severity == ProviderMessageSeverity::Warning && message.contains("truncated")
            })
            .count();
        assert_eq!(truncations, 1);
    }
}
