//! What the application is doing, as one value.
//!
//! Nothing here is a Tauri type, for the reason [`crate::job`]'s own header
//! gives: the shell carries these values across the boundary, and a state that
//! can only be observed through a webview cannot be observed by the tray, by a
//! second window, or by a test.
//!
//! **One snapshot, not a stream of edges.** Before this type there was a
//! boolean and a channel, and everything that wanted to draw the scan had to
//! reconstruct the state from events it may not have heard: a window that
//! reloaded mid-job, a tray built after the job started, and a settings strip
//! opened half-way through all had different answers, and none of them could
//! be told apart from a job that had never run. A consumer reads [`ScanState`]
//! whenever it likes and draws exactly what it finds, and [`ScanState::
//! revision`] is what lets it know a read is worth repeating.

use serde::Serialize;

/// Everything a surface needs to draw the scan, in one read.
///
/// Every field outside [`ScanState::snapshot`] outlives the job that wrote it,
/// and that is the point of their being here rather than inside the snapshot:
/// how many files the index holds and what the last reading pass concluded are
/// still the truth after the job that established them has gone, and a window
/// opened afterwards has nowhere else to get them.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScanState {
    /// Bumped by every write to this struct, and never reset.
    ///
    /// It exists so that a consumer can tell "nothing has changed since I last
    /// looked" from "it changed and changed back": two reads that are equal
    /// field for field are not evidence that nothing happened in between, and a
    /// surface that redraws on difference alone misses a job that started and
    /// ended between two of its polls.
    pub revision: u64,
    /// How many files the index holds, as of the last count.
    ///
    /// A property of the index and not of any job: it survives the job that
    /// counted it, the next job's claim, and an ending that counted nothing.
    /// `i64` rather than `u64` because that is what the index answers with.
    pub files: i64,
    /// How many reading passes have ENDED in this process, ever.
    ///
    /// Moved by [`JobSlot::mark_reading_done`] and by nothing else. A consumer
    /// that re-reads the index when a scan has read new documents watches this
    /// rather than `revision`, which moves on every progress tick, and rather
    /// than the snapshot going idle, which also happens when a probe ends.
    ///
    /// [`JobSlot::mark_reading_done`]: crate::state::JobSlot::mark_reading_done
    pub read_seq: u64,
    /// What the last reading pass concluded, or `None` before the first one in
    /// this process. Survives the pass, the job and the next claim, for the
    /// same reason `files` does: it is what a window opened afterwards reads.
    pub last_reading: Option<ReadingOutcome>,
    /// What is happening right now — the one field that is about the job rather
    /// than about what the jobs have left behind.
    pub snapshot: ScanSnapshot,
}

/// The three states a job slot can be in.
///
/// `Ended` is a state and not an event, which is the whole difference from the
/// channel this replaces: a job that finished stays finished until the next one
/// starts, so a window opened a minute later still finds out how the scan went
/// rather than an idle application that looks like one that never ran.
///
/// 🔴 `rename_all_fields`, and not only `rename_all`. On an enum the second
/// renames the VARIANTS and leaves every struct-variant field in snake_case,
/// which compiles, serialises, and reaches the window as `root_index` beside a
/// `kind` that is spelled correctly. `scan_state::tests::
/// every_snapshot_has_its_wire_shape_pinned` is what caught it.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ScanSnapshot {
    /// Nothing is running and nothing has ended that anyone still needs to be
    /// told about. The state a fresh process starts in.
    #[default]
    Idle,
    Running {
        phase: Phase,
        /// Whether Stop is a thing a person may press. Fixed for the life of
        /// the job — a phase change does not make a job that could not be
        /// interrupted interruptible — which is why it sits beside the phase
        /// rather than inside it.
        cancellable: bool,
    },
    Ended {
        report: ScanReport,
    },
}

/// What the running job is doing, in the terms a person reads.
///
/// The four variants are not four job types: they are four things a surface has
/// to draw differently. A reading pass has a folder and a position within a set
/// of folders; an embedding pass has neither; a removal has a folder and no
/// counts at all; and the jobs a person did not ask for have nothing to draw
/// but the fact that the slot is busy.
///
/// `rename_all_fields` for the reason [`ScanSnapshot`]'s own attribute records.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Phase {
    Reading {
        /// Which folder of `root_count` this is, counting from zero. A scan
        /// over several watched folders is one job, and "3 of 7" is what says
        /// so; without it a progress bar restarts and reads as a new scan.
        root_index: u64,
        root_count: u64,
        root_path: String,
        counts: crate::job::Progress,
    },
    Embedding {
        counts: crate::job::Progress,
    },
    Removing {
        root_path: String,
    },
    /// A job the person did not start and is not shown: the start-up probe and
    /// the adoption of an embedding model. It holds the same single slot as a
    /// scan — that is why it is here at all — but it owes no report when it
    /// ends, which is what [`crate::state::JobSlot::drop`] reads this for.
    Other {
        job: OtherJob,
    },
}

/// The jobs that are not a scan. A closed enumeration on purpose: a job that is
/// neither of these and is still not a scan has to be added here by name, and
/// whoever adds it is the one who decides what a vanished one leaves behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OtherJob {
    Probe,
    ModelAdoption,
}

/// How a job ended, as [`crate::state::JobSlot::finish`] takes it.
///
/// Not on the wire and deliberately not [`ScanSnapshot`] itself: it is the
/// half of that enum a finishing job may choose between, and `Running` is not
/// one of the choices. A `finish` that could write `Running` would leave a
/// snapshot claiming a job is going with no slot behind it and nothing left to
/// correct it.
#[derive(Debug, Clone, PartialEq)]
pub enum Terminal {
    Idle,
    Ended { report: ScanReport },
}

/// What a finished scan has to say for itself.
///
/// Two fields today, and the rest is Task 2's: `reason` is the same
/// [`crate::job::EndReason`] the job events already carry, so a scan that
/// stopped because a volume went missing says so here in the same words it
/// would have said on the channel, and `message` is the diagnostic sentence for
/// the endings that have one.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub reason: crate::job::EndReason,
    pub message: Option<String>,
}

/// `Completed` and no message: the shape of an ending nobody had anything to
/// add to.
///
/// Hand-written rather than derived, because the derive would need a `Default`
/// on [`crate::job::EndReason`] itself — and a defaulted end reason is a
/// success claim that any new writer of that enum would inherit without
/// choosing it. Here the choice is visible and belongs to this struct alone.
impl Default for ScanReport {
    fn default() -> Self {
        Self {
            reason: crate::job::EndReason::Completed,
            message: None,
        }
    }
}

/// What a reading pass concluded, kept apart from the report because it outlives
/// the job: `ScanReport` is about the job that ended, this is about the state of
/// the index the pass left behind. Task 2 gives it its fields.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReadingOutcome {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::{EndReason, Progress};

    /// The wire shape of every snapshot a window can be handed, pinned as JSON.
    ///
    /// The pair of states it separates is "the window and the core agree on the
    /// spelling" from "they agree on the Rust type and disagree on the JSON" —
    /// a rename, an added `#[serde(tag)]`, or a field that stops being
    /// camelCase compiles perfectly and reaches the window as a snapshot it
    /// cannot match on. `kind` is the discriminant every consumer switches on,
    /// so it is the one name that cannot drift quietly.
    ///
    /// The two stub structs are asserted only as the containers they are, never
    /// by their fields: Task 2 fills them, and a test that pinned an empty
    /// `ReadingOutcome` would have to be rewritten by the task that gives it
    /// something to say.
    #[test]
    fn every_snapshot_has_its_wire_shape_pinned() {
        let value = |snapshot: ScanSnapshot| serde_json::to_value(snapshot).unwrap();

        assert_eq!(
            value(ScanSnapshot::Idle),
            serde_json::json!({ "kind": "idle" })
        );

        assert_eq!(
            value(ScanSnapshot::Running {
                phase: Phase::Reading {
                    root_index: 2,
                    root_count: 7,
                    root_path: "/Users/somebody/Documents".to_string(),
                    counts: Progress {
                        done: 3,
                        total: 40,
                        skipped: 1,
                        refused: 0,
                        contended: 0,
                        seconds_left: Some(12),
                    },
                },
                cancellable: true,
            }),
            serde_json::json!({
                "kind": "running",
                "cancellable": true,
                "phase": {
                    "kind": "reading",
                    "rootIndex": 2,
                    "rootCount": 7,
                    "rootPath": "/Users/somebody/Documents",
                    "counts": {
                        "done": 3,
                        "total": 40,
                        "skipped": 1,
                        "refused": 0,
                        "contended": 0,
                        "secondsLeft": 12,
                    },
                },
            })
        );

        assert_eq!(
            value(ScanSnapshot::Running {
                phase: Phase::Embedding {
                    counts: Progress::default(),
                },
                cancellable: false,
            }),
            serde_json::json!({
                "kind": "running",
                "cancellable": false,
                "phase": {
                    "kind": "embedding",
                    "counts": {
                        "done": 0,
                        "total": 0,
                        "skipped": 0,
                        "refused": 0,
                        "contended": 0,
                        "secondsLeft": null,
                    },
                },
            })
        );

        assert_eq!(
            value(ScanSnapshot::Running {
                phase: Phase::Removing {
                    root_path: "/Users/somebody/Archive".to_string(),
                },
                cancellable: true,
            }),
            serde_json::json!({
                "kind": "running",
                "cancellable": true,
                "phase": { "kind": "removing", "rootPath": "/Users/somebody/Archive" },
            })
        );

        for (job, spelling) in [
            (OtherJob::Probe, "probe"),
            (OtherJob::ModelAdoption, "modelAdoption"),
        ] {
            assert_eq!(
                value(ScanSnapshot::Running {
                    phase: Phase::Other { job },
                    cancellable: true,
                }),
                serde_json::json!({
                    "kind": "running",
                    "cancellable": true,
                    "phase": { "kind": "other", "job": spelling },
                }),
                "{job:?} serialized differently than this test's own spelling of it"
            );
        }

        assert_eq!(
            value(ScanSnapshot::Ended {
                report: ScanReport {
                    reason: EndReason::VolumeMissing,
                    message: Some("the volume is not mounted".to_string()),
                },
            }),
            serde_json::json!({
                "kind": "ended",
                "report": {
                    "reason": "volumeMissing",
                    "message": "the volume is not mounted",
                },
            })
        );
    }

    /// A fresh state is idle, has counted nothing and has read nothing — and it
    /// says so in the fields, not by being absent.
    ///
    /// The pair it separates is "no scan has run in this process" from "a scan
    /// ran and found an empty folder": both draw zero files, and only
    /// `lastReading` tells them apart, which is why `None` here is asserted
    /// rather than left to whatever `Default` happens to produce.
    #[test]
    fn a_state_nothing_has_happened_to_yet_is_idle_and_says_so() {
        let fresh = ScanState::default();
        assert_eq!(fresh.revision, 0);
        assert_eq!(fresh.files, 0);
        assert_eq!(fresh.read_seq, 0);
        assert_eq!(fresh.last_reading, None);
        assert_eq!(fresh.snapshot, ScanSnapshot::Idle);
        assert_eq!(
            serde_json::to_value(&fresh).unwrap(),
            serde_json::json!({
                "revision": 0,
                "files": 0,
                "readSeq": 0,
                "lastReading": null,
                "snapshot": { "kind": "idle" },
            })
        );
    }
}
