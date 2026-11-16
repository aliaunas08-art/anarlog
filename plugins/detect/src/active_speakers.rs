//! Records who the meeting platform marks as speaking while a capture runs.
//!
//! The timeline is a plain list of intervals in Unix milliseconds. It is
//! returned to the caller when tracking stops and stored with the session;
//! the post-capture speaker pass lines it up with the transcript to name
//! remote voices (attribution source `platform_tag`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anlg_detect::{MeetingActiveSpeakerSample, MeetingPlatform};

pub const SAMPLE_INTERVAL_MS: i64 = 1_000;
/// Two equal samples further apart than this are separate intervals.
const MAX_MERGE_GAP_MS: i64 = 2_500;
/// Bounds memory and the stored size (about 5.5 hours of constant changes).
const MAX_INTERVALS: usize = 20_000;
pub const TIMELINE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ActiveSpeakerInterval {
    pub start_ms: i64,
    pub end_ms: i64,
    /// Remote participants the platform marked as speaking.
    pub speakers: Vec<String>,
    pub self_speaking: bool,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ActiveSpeakerTimeline {
    pub version: u32,
    pub platform: Option<MeetingPlatform>,
    pub app_id: Option<String>,
    /// Samples taken while a call was in progress.
    pub samples: u32,
    /// Of those, samples where the platform marked someone as speaking.
    pub tagged_samples: u32,
    pub intervals: Vec<ActiveSpeakerInterval>,
}

#[derive(Debug, Default)]
pub(crate) struct TimelineBuilder {
    timeline: ActiveSpeakerTimeline,
    previous: Option<(Vec<String>, bool)>,
}

impl TimelineBuilder {
    pub(crate) fn new() -> Self {
        Self {
            timeline: ActiveSpeakerTimeline {
                version: TIMELINE_VERSION,
                ..Default::default()
            },
            previous: None,
        }
    }

    /// The meeting app this capture follows once a call was seen.
    pub(crate) fn followed_app(&self) -> Option<String> {
        self.timeline.app_id.clone()
    }

    pub(crate) fn observe(&mut self, at_ms: i64, samples: &[MeetingActiveSpeakerSample]) {
        let chosen = match self.timeline.app_id.as_deref() {
            Some(app_id) => samples.iter().find(|sample| sample.app.id == app_id),
            None => {
                let mut active = samples.iter().filter(|sample| sample.active_call);
                match (active.next(), active.next()) {
                    // Two apps in a call at once: no way to tell which one
                    // the recording hears, so wait until only one is.
                    (Some(sample), None) => Some(sample),
                    _ => None,
                }
            }
        };
        let Some(sample) = chosen.filter(|sample| sample.active_call) else {
            self.previous = None;
            return;
        };
        if self.timeline.app_id.is_none() {
            self.timeline.app_id = Some(sample.app.id.clone());
            self.timeline.platform = Some(sample.platform.clone());
        }
        self.timeline.samples += 1;
        let key = (sample.speakers.clone(), sample.self_speaking);
        if key.0.is_empty() && !key.1 {
            self.previous = None;
            return;
        }
        self.timeline.tagged_samples += 1;
        let continues = self.previous.as_ref() == Some(&key);
        self.previous = Some(key.clone());
        if continues
            && let Some(last) = self.timeline.intervals.last_mut()
            && at_ms - last.end_ms <= MAX_MERGE_GAP_MS
        {
            last.end_ms = last.end_ms.max(at_ms + SAMPLE_INTERVAL_MS / 2);
            return;
        }
        if self.timeline.intervals.len() >= MAX_INTERVALS {
            return;
        }
        self.timeline.intervals.push(ActiveSpeakerInterval {
            start_ms: at_ms - SAMPLE_INTERVAL_MS / 2,
            end_ms: at_ms + SAMPLE_INTERVAL_MS / 2,
            speakers: key.0,
            self_speaking: key.1,
        });
    }

    pub(crate) fn finish(self) -> ActiveSpeakerTimeline {
        self.timeline
    }
}

struct Run {
    generation: u64,
    stop: Arc<AtomicBool>,
    builder: TimelineBuilder,
}

#[derive(Default)]
pub(crate) struct ActiveSpeakerTracker {
    run: Mutex<Option<Run>>,
    next_generation: std::sync::atomic::AtomicU64,
}

pub(crate) type ActiveSpeakerTrackerState = Arc<ActiveSpeakerTracker>;

impl ActiveSpeakerTracker {
    /// Starts a fresh timeline, discarding any unfinished one. Returns the
    /// generation and its stop flag for the sampling loop.
    pub(crate) fn start(&self) -> (u64, Arc<AtomicBool>) {
        let generation = self.next_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let stop = Arc::new(AtomicBool::new(false));
        let mut run = lock(&self.run);
        if let Some(previous) = run.take() {
            previous.stop.store(true, Ordering::SeqCst);
        }
        *run = Some(Run {
            generation,
            stop: stop.clone(),
            builder: TimelineBuilder::new(),
        });
        (generation, stop)
    }

    pub(crate) fn followed_app(&self, generation: u64) -> Option<Option<String>> {
        lock(&self.run)
            .as_ref()
            .filter(|run| run.generation == generation)
            .map(|run| run.builder.followed_app())
    }

    pub(crate) fn observe(
        &self,
        generation: u64,
        at_ms: i64,
        samples: &[MeetingActiveSpeakerSample],
    ) {
        if let Some(run) = lock(&self.run)
            .as_mut()
            .filter(|run| run.generation == generation)
        {
            run.builder.observe(at_ms, samples);
        }
    }

    pub(crate) fn stop(&self) -> Option<ActiveSpeakerTimeline> {
        let run = lock(&self.run).take()?;
        run.stop.store(true, Ordering::SeqCst);
        Some(run.builder.finish())
    }
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

pub(crate) const SAMPLE_INTERVAL: Duration = Duration::from_millis(SAMPLE_INTERVAL_MS as u64);

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anlg_detect::MeetingApp;

    fn sample(app: &str, call: bool, speakers: &[&str], me: bool) -> MeetingActiveSpeakerSample {
        MeetingActiveSpeakerSample {
            app: MeetingApp {
                id: app.into(),
                name: app.into(),
            },
            platform: MeetingPlatform::Zoom,
            active_call: call,
            speakers: speakers.iter().map(|s| s.to_string()).collect(),
            self_speaking: me,
        }
    }

    #[test]
    fn merges_consecutive_equal_samples_into_intervals() {
        let mut builder = TimelineBuilder::new();
        let zoom = "us.zoom.xos";
        builder.observe(10_000, &[sample(zoom, true, &["Sam"], false)]);
        builder.observe(11_000, &[sample(zoom, true, &["Sam"], false)]);
        builder.observe(12_000, &[sample(zoom, true, &[], false)]);
        builder.observe(13_000, &[sample(zoom, true, &["Sam"], false)]);
        builder.observe(14_000, &[sample(zoom, true, &["Priya"], false)]);
        builder.observe(15_000, &[sample(zoom, true, &[], true)]);
        let timeline = builder.finish();
        assert_eq!(timeline.app_id.as_deref(), Some(zoom));
        assert_eq!(timeline.samples, 6);
        assert_eq!(timeline.tagged_samples, 5);
        let spans: Vec<_> = timeline
            .intervals
            .iter()
            .map(|i| (i.start_ms, i.end_ms, i.speakers.join("+"), i.self_speaking))
            .collect();
        assert_eq!(
            spans,
            vec![
                (9_500, 11_500, "Sam".into(), false),
                (12_500, 13_500, "Sam".into(), false),
                (13_500, 14_500, "Priya".into(), false),
                (14_500, 15_500, "".into(), true),
            ]
        );
    }

    #[test]
    fn waits_for_a_single_call_then_follows_that_app() {
        let mut builder = TimelineBuilder::new();
        builder.observe(
            1_000,
            &[
                sample("us.zoom.xos", true, &["Sam"], false),
                sample("com.google.Chrome", true, &["Priya"], false),
            ],
        );
        assert_eq!(builder.followed_app(), None);
        builder.observe(
            2_000,
            &[sample("com.google.Chrome", true, &["Priya"], false)],
        );
        builder.observe(3_000, &[sample("us.zoom.xos", true, &["Sam"], false)]);
        let timeline = builder.finish();
        assert_eq!(timeline.app_id.as_deref(), Some("com.google.Chrome"));
        assert_eq!(timeline.intervals.len(), 1);
        assert_eq!(timeline.intervals[0].speakers, vec!["Priya".to_string()]);
    }

    #[test]
    fn stop_returns_the_timeline_once_and_restart_discards_the_old_run() {
        let tracker = ActiveSpeakerTracker::default();
        let (first, first_stop) = tracker.start();
        tracker.observe(
            first,
            1_000,
            &[sample("us.zoom.xos", true, &["Sam"], false)],
        );
        let (second, _) = tracker.start();
        assert!(first_stop.load(Ordering::SeqCst));
        tracker.observe(
            first,
            2_000,
            &[sample("us.zoom.xos", true, &["Late"], false)],
        );
        tracker.observe(
            second,
            3_000,
            &[sample("us.zoom.xos", true, &["Priya"], false)],
        );
        let timeline = tracker.stop().unwrap();
        assert_eq!(timeline.intervals.len(), 1);
        assert_eq!(timeline.intervals[0].speakers, vec!["Priya".to_string()]);
        assert!(tracker.stop().is_none());
    }
}
