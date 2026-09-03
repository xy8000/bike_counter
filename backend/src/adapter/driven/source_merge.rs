//! Reusable whole-source (channel-interleaved) measurement reader used by the
//! driven adapters' [`DataProvider::get_measurements_source`] implementations.
//!
//! A single shared `imported_until` watermark can only advance safely while
//! every channel has been read past it, so the scanner pages its channels
//! **fairly** — one page per channel, round-robin — and reports a watermark
//! equal to the minimum real cursor over the channels that still have data.
//! Synthetic gap-skip cursors never become the watermark.
//!
//! Cursors live for the duration of one run and are re-seeded from the passed
//! `from` (the persisted `imported_until`) whenever the run changes, so a
//! process restart resumes from the persisted watermark and only re-reads the
//! un-checkpointed tail.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::core::domain::data_source::provider_port::{
    ProviderError, SourceMeasurement, SourceMeasurementBatch,
};

/// A single channel page produced by an adapter's pager.
#[derive(Debug)]
pub struct ChannelPage {
    /// Real measurements of the channel, ascending, all `> from` (exclusive).
    pub measurements: Vec<SourceMeasurement>,
    /// Highest real measurement returned by this page (`None` when empty).
    pub last_real: Option<DateTime<Utc>>,
    /// Exclusive cursor for the channel's next page: the last real timestamp
    /// when rows were returned, else the synthetic gap-skip cursor (never a
    /// watermark). `None` when the channel is exhausted.
    pub next_from: Option<DateTime<Utc>>,
    /// `true` when no more data follows this channel.
    pub done: bool,
}

#[derive(Clone)]
struct Cursor {
    /// Exclusive lower bound of the next fetch.
    next: Option<DateTime<Utc>>,
    /// Highest real measurement read so far; caps the safe watermark.
    last_real: Option<DateTime<Utc>>,
    done: bool,
}

/// Fair whole-source reader state for one run (anchored at a `from`).
pub struct SourceScanner {
    anchor: Option<DateTime<Utc>>,
    ids: Vec<String>,
    cursors: HashMap<String, Cursor>,
    /// Round-robin index of the next channel to page.
    round_robin: usize,
}

impl SourceScanner {
    /// Builds a fresh scanner over `channel_external_ids`, all starting at
    /// `from`.
    pub fn new(from: Option<DateTime<Utc>>, channel_external_ids: &[String]) -> Self {
        let cursors = channel_external_ids
            .iter()
            .map(|id| {
                (
                    id.clone(),
                    Cursor {
                        next: from,
                        last_real: None,
                        done: false,
                    },
                )
            })
            .collect();
        Self {
            anchor: from,
            ids: channel_external_ids.to_vec(),
            cursors,
            round_robin: 0,
        }
    }

    /// Whether this scanner is already seeded for `from` / `channel_external_ids`.
    pub fn matches(&self, from: Option<DateTime<Utc>>, channel_external_ids: &[String]) -> bool {
        self.anchor == from && self.ids == channel_external_ids
    }

    /// Re-seeds this scanner for a new run / channel set.
    pub fn reset(&mut self, from: Option<DateTime<Utc>>, channel_external_ids: &[String]) {
        *self = SourceScanner::new(from, channel_external_ids);
    }

    /// Picks the next not-done channel to page (round-robin), returning its
    /// external id and the exclusive lower bound of its next fetch. `None` when
    /// every channel is done.
    pub fn next_channel(&mut self) -> Option<(String, Option<DateTime<Utc>>)> {
        let n = self.ids.len();
        for _ in 0..n {
            let index = self.round_robin % n;
            self.round_robin = (self.round_robin + 1) % n;
            let id = self.ids[index].clone();
            let cursor = self.cursors.get(&id).expect("scanner cursor per channel");
            if !cursor.done {
                return Some((id, cursor.next));
            }
        }
        None
    }

    /// Records a fetched page for one channel and returns the resulting batch.
    pub fn record(
        &mut self,
        id: &str,
        page: ChannelPage,
    ) -> Result<SourceMeasurementBatch, ProviderError> {
        let cursor = self
            .cursors
            .get_mut(id)
            .ok_or_else(|| ProviderError::InvalidData(format!("unknown channel '{id}'")))?;
        if page.last_real.is_some() {
            cursor.last_real = page.last_real;
        }
        cursor.next = page.next_from;
        cursor.done = page.done;

        let next_from = self.watermark();
        let more = self.cursors.values().any(|c| !c.done);
        Ok(SourceMeasurementBatch {
            measurements: page.measurements,
            next_from,
            more,
        })
    }

    /// The largest timestamp through which every channel has definitely been
    /// read: the minimum `last_real` over all channels that hold any data. An
    /// active channel without a real measurement yet blocks the watermark;
    /// exhausted channels contribute their final real cursor (a channel with no
    /// data at all does not constrain it).
    fn watermark(&self) -> Option<DateTime<Utc>> {
        let mut min: Option<DateTime<Utc>> = None;
        for cursor in self.cursors.values() {
            if cursor.done {
                if let Some(real) = cursor.last_real {
                    min = Some(match min {
                        Some(current) if current < real => current,
                        Some(current) => current,
                        None => real,
                    });
                }
                continue;
            }
            // An active channel with no real data yet: do not advance past it.
            let real = cursor.last_real?;
            min = Some(match min {
                Some(current) if current < real => current,
                Some(current) => current,
                None => real,
            });
        }
        min
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn meas(id: &str, t: DateTime<Utc>) -> SourceMeasurement {
        SourceMeasurement {
            channel_external_id: id.to_string(),
            record: crate::core::domain::data_source::provider_port::MeasurementRecord {
                value: 1,
                timestamp: t,
                resolution_seconds: 300,
                interval_end: None,
            },
        }
    }

    fn page(id: &str, from: Option<DateTime<Utc>>) -> ChannelPage {
        let next = from
            .map(|t| t + chrono::Duration::minutes(5))
            .unwrap_or_else(|| utc("2026-01-01T00:00:00Z"));
        ChannelPage {
            measurements: vec![meas(id, next)],
            last_real: Some(next),
            next_from: Some(next),
            done: false,
        }
    }

    #[test]
    fn pages_channels_fairly_and_blocks_the_watermark_until_all_have_data() {
        let ids = vec!["a".to_string(), "b".to_string()];
        let mut scanner = SourceScanner::new(None, &ids);

        // Call 1 pages a only; b has no real data -> watermark blocked.
        let (a_id, a_from) = scanner.next_channel().unwrap();
        assert_eq!(a_id, "a");
        let batch_a = scanner.record(&a_id, page(&a_id, a_from)).unwrap();
        assert_eq!(batch_a.measurements.len(), 1);
        assert!(
            batch_a.next_from.is_none(),
            "b not read yet -> no watermark"
        );
        assert!(batch_a.more);

        // Call 2 pages b.
        let (b_id, b_from) = scanner.next_channel().unwrap();
        assert_eq!(b_id, "b");
        let batch_b = scanner.record(&b_id, page(&b_id, b_from)).unwrap();
        assert!(
            batch_b.next_from.is_some(),
            "both channels now have real data"
        );
        assert!(batch_b.more);
    }

    #[test]
    fn exhausted_channels_are_skipped_and_signal_no_more() {
        let ids = vec!["a".to_string()];
        let from = Some(utc("2026-01-01T00:00:00Z"));
        let mut scanner = SourceScanner::new(from, &ids);
        assert!(scanner.matches(from, &ids));
        assert!(!scanner.matches(None, &ids));

        let (id, _next) = scanner.next_channel().unwrap();
        let batch = scanner
            .record(
                &id,
                ChannelPage {
                    measurements: vec![],
                    last_real: None,
                    next_from: None,
                    done: true,
                },
            )
            .unwrap();
        assert!(!batch.more);
        assert!(scanner.next_channel().is_none());
    }
}
