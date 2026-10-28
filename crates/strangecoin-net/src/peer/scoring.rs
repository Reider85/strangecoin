//! S2-P17 — ADR-0022 peer scoring: weighted quality/abuse formula + the
//! four-state ban decision ladder.
//!
//! Score = `clamp01(quality − abuse)` where the **grading** part
//! (`latency`, `uptime`) is normalized to `[0, 1]` and the **counter**
//! part (`invalid_blocks_txs_sent`, `protocol_violations`) is an
//! unclamped linear penalty (ADR-0022 §Scoring formula, including the
//! four recorded deviations from the prompt's literal sketch: counters
//! are subtractive, quality is scaled by its own weight maximum, counter
//! norms have no upper clamp so the ban thresholds stay reachable, and
//! uptime is a responsive fraction rather than cumulative contact time).
//!
//! The structural false-positive guarantee (ROADMAP3 risk «scoring может
//! ban legitimate peers»): latency and uptime only *grade* — a fully
//! responsive peer with zero violations floors at
//! `(w_uptime)/(w_latency + w_uptime) = 0.1/0.3 ≈ 0.333` (Warn band),
//! mathematically above the 0.2 temp threshold whatever its latency. Only
//! protocol-level counters can cross ban thresholds.
//!
//! **Determinism** (Stage 2 checklist — S2-P17 is one of the five prompts
//! where the review is mandatory): [`compute_score`] is a pure function
//! of the record's factor values and the config — no `SystemTime`, no
//! `Instant`, no RNG, no I/O. Counters/EWMA update only from observed
//! protocol events. Wall time enters only at the edges: the injected
//! `now` that stamps `BanDecision::TempBan/PersistentBan { until }` and
//! the `tick_at` ban-expiry comparison (ban **policy**, never consensus
//! state). Peer iteration order never influences results — every score
//! depends on its own record alone.
//!
//! S2-P17 deviations from the prompt sketch (recorded in the commit):
//! `update_score` takes an injected `now` (the sketch's signature cannot
//! compute `until` for a ban decision); the config additionally carries
//! the ADR-0022 full-weight constants (`invalid_full_weight`,
//! `violation_full_weight` — tuning surface, absent from the prompt's
//! struct sketch); uptime uses
//! the ADR's fraction semantics (`responsive/observed`, deviation #4)
//! instead of the prompt's cumulative-seconds/30-day-cap wording.

use super::store::{BanReason, PeerRecord};
use std::time::{Duration, Instant};

/// Latency EWMA smoothing (ADR-0022: `ewma = 0.9 * ewma + 0.1 * sample`).
pub const LATENCY_EWMA_ALPHA: f32 = 0.1;
/// Latency normalization scale: `latency_norm = clamp(ms / 100, 0, 1)` —
/// ≥100 ms scores zero on the latency factor (ADR-0022 table).
pub const LATENCY_NORM_MS: f32 = 100.0;
/// Band-comparison tolerance: f32 rounding can place a mathematically
/// exact boundary score (ADR-0022 milestone: 16 violations → exactly
/// 0.200) a hair *below* its threshold (`0.2f32 * 4 = 0.80000001`,
/// `1 − that < 0.2`). The epsilon keeps such knife-edge cases in the
/// lower-severity band — false-positive-first, per the ROADMAP3
/// mitigation («scoring может ban legitimate peers»).
const SCORE_EPSILON: f32 = 1e-6;

/// Default latency weight (ADR-0022 initial table).
pub const DEFAULT_W_LATENCY: f32 = 0.2;
/// Default invalid-blocks/txs weight (dominant penalty, threat V-15).
pub const DEFAULT_W_INVALID: f32 = 0.5;
/// Default protocol-violations weight (rate-limit + misuse feed, V-17).
pub const DEFAULT_W_VIOLATION: f32 = 0.2;
/// Default uptime weight.
pub const DEFAULT_W_UPTIME: f32 = 0.1;
/// Score below which a peer is warned (stays connected).
pub const DEFAULT_WARN_THRESHOLD: f32 = 0.4;
/// Score below which a peer is temp-banned 1 h (`config.toml:
/// peer_ban_threshold`, the S2-P12 key kept for back-compat).
pub const DEFAULT_BAN_THRESHOLD: f32 = 0.2;
/// Score below which a peer is persistently banned 30 d (network.json).
pub const DEFAULT_PERSISTENT_THRESHOLD: f32 = 0.05;
/// Temp-ban duration (ADR-0022).
pub const DEFAULT_TEMP_BAN_DURATION: Duration = Duration::from_secs(60 * 60);
/// Persistent-ban duration (ADR-0022: 30 days).
pub const DEFAULT_PERSISTENT_BAN_DURATION: Duration = Duration::from_secs(60 * 60 * 24 * 30);
/// Invalid events that cost the full `w_invalid` penalty (ADR-0022:
/// `INVALID_FULL_WEIGHT` — 12 invalids reach score 0 on a healthy peer).
pub const DEFAULT_INVALID_FULL_WEIGHT: u32 = 6;
/// Violations that cost the full `w_violation` penalty (ADR-0022:
/// `VIOLATION_FULL_WEIGHT` — 20 violations reach score 0).
pub const DEFAULT_VIOLATION_FULL_WEIGHT: u32 = 4;

/// Operator-facing scoring tunables (`config.toml: peer_scoring_*`,
/// ADR-0022 "Tuning surface"). Defaults reproduce the ADR-0022 initial
/// tables; the temp-ban threshold keeps its historical S2-P12 key
/// `peer_ban_threshold` in `config.toml` (back-compat deviation).
#[derive(Debug, Clone, PartialEq)]
pub struct ScoringConfig {
    /// Latency grading weight.
    pub w_latency: f32,
    /// Invalid blocks/txs penalty weight (threat V-15).
    pub w_invalid: f32,
    /// Protocol-violations penalty weight (rate-limit feed, V-17).
    pub w_violation: f32,
    /// Uptime grading weight.
    pub w_uptime: f32,
    /// `score < warn_threshold` → [`BanDecision::Warn`].
    pub warn_threshold: f32,
    /// `score < temp_ban_threshold` → [`BanDecision::TempBan`] (1 h).
    pub temp_ban_threshold: f32,
    /// `score < persistent_ban_threshold` → [`BanDecision::PersistentBan`]
    /// (30 d, `network.json`).
    pub persistent_ban_threshold: f32,
    /// Temp-ban duration.
    pub temp_ban_duration: Duration,
    /// Persistent-ban duration.
    pub persistent_ban_duration: Duration,
    /// Invalid events at which the invalid penalty reaches `w_invalid`.
    pub invalid_full_weight: u32,
    /// Violations at which the violation penalty reaches `w_violation`.
    pub violation_full_weight: u32,
}

impl Default for ScoringConfig {
    fn default() -> Self {
        ScoringConfig {
            w_latency: DEFAULT_W_LATENCY,
            w_invalid: DEFAULT_W_INVALID,
            w_violation: DEFAULT_W_VIOLATION,
            w_uptime: DEFAULT_W_UPTIME,
            warn_threshold: DEFAULT_WARN_THRESHOLD,
            temp_ban_threshold: DEFAULT_BAN_THRESHOLD,
            persistent_ban_threshold: DEFAULT_PERSISTENT_THRESHOLD,
            temp_ban_duration: DEFAULT_TEMP_BAN_DURATION,
            persistent_ban_duration: DEFAULT_PERSISTENT_BAN_DURATION,
            invalid_full_weight: DEFAULT_INVALID_FULL_WEIGHT,
            violation_full_weight: DEFAULT_VIOLATION_FULL_WEIGHT,
        }
    }
}

/// Threshold-band evaluation of one score (ADR-0022 ban state machine:
/// NoAction → Warn → TempBan → PersistentBan).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BanDecision {
    /// `score >= warn_threshold` — nothing to do.
    NoAction,
    /// `temp_ban_threshold <= score < warn_threshold` — log + reduced
    /// broadcast priority, connection stays up; recovery pulls the score
    /// back before any disconnect.
    Warn,
    /// `persistent_ban_threshold <= score < temp_ban_threshold` —
    /// connection dropped for `temp_ban_duration` (in-memory).
    TempBan {
        until: Instant,
    },
    /// Score below the persistent threshold — 30 d, written to
    /// `network.json`, survives restart.
    PersistentBan {
        until: Instant,
    },
}

/// Latency factor: `clamp(ms / 100, 0, 1)` (ADR-0022). ≥100 ms scores
/// zero on this factor.
pub fn latency_norm(record: &PeerRecord) -> f32 {
    let ms = record.latency_ewma.as_secs_f32() * 1000.0;
    (ms / LATENCY_NORM_MS).clamp(0.0, 1.0)
}

/// Uptime factor: the ADR-0022 responsive fraction
/// `responsive_secs / observed_secs`, clamped to `[0, 1]`.
///
/// A responsive peer sits at 1.0 from its first tick (no new-peer cold
/// start — deviation #4 of the prompt's cumulative-seconds wording); a
/// peer that stops responding decays toward 0 as `observed` grows past
/// `responsive`. Nothing observed yet → 1.0.
pub fn uptime_norm(record: &PeerRecord) -> f32 {
    if record.observed_seconds == 0 {
        return 1.0;
    }
    (record.uptime_seconds as f32 / record.observed_seconds as f32).clamp(0.0, 1.0)
}

/// The ADR-0022 weighted formula, pure (determinism checklist #1):
///
/// ```text
/// quality = (w_latency·(1 − latency_norm) + w_uptime·uptime_norm)
///           / (w_latency + w_uptime)             # grading part, ∈ [0, 1]
/// abuse   = w_invalid·(invalid / INVALID_FULL)
///         + w_violation·(violations / VIOLATION_FULL)   # unclamped, ≥ 0
/// score   = clamp01(quality − abuse)
/// ```
pub fn compute_score(record: &PeerRecord, config: &ScoringConfig) -> f32 {
    let grading_max = config.w_latency + config.w_uptime;
    let quality = if grading_max > 0.0 {
        (config.w_latency * (1.0 - latency_norm(record))
            + config.w_uptime * uptime_norm(record))
            / grading_max
    } else {
        0.0
    };
    let invalid_full = config.invalid_full_weight.max(1) as f32;
    let violation_full = config.violation_full_weight.max(1) as f32;
    let abuse = config.w_invalid * (record.invalid_count as f32 / invalid_full)
        + config.w_violation * (record.violation_count as f32 / violation_full);
    (quality - abuse).clamp(0.0, 1.0)
}

/// Threshold ladder (ADR-0022 ban state machine), pure. Comparisons carry
/// [`SCORE_EPSILON`] so exact-boundary scores land in the lower-severity
/// band (see the constant's doc).
pub fn decide(score: f32, config: &ScoringConfig, now: Instant) -> BanDecision {
    let below = |threshold: f32| score < threshold - SCORE_EPSILON;
    if below(config.persistent_ban_threshold) {
        BanDecision::PersistentBan {
            until: now + config.persistent_ban_duration,
        }
    } else if below(config.temp_ban_threshold) {
        BanDecision::TempBan {
            until: now + config.temp_ban_duration,
        }
    } else if below(config.warn_threshold) {
        BanDecision::Warn
    } else {
        BanDecision::NoAction
    }
}

/// Recompute `record.score` from its factors and return the band decision
/// for `now` (the prompt's `update_score`, with the clock the sketch's
/// signature lacks — deviation recorded above). Pure arithmetic; wall
/// time only stamps `until` (determinism checklist #1).
pub fn update_score(record: &mut PeerRecord, config: &ScoringConfig, now: Instant) -> BanDecision {
    let score = compute_score(record, config);
    record.score = score;
    decide(score, config, now)
}

/// Feed one observed message-latency sample into the record's EWMA
/// (`0.9 * ewma + 0.1 * sample`, ADR-0022). Grading only — latency can
/// never cross a ban threshold on its own (see module docs).
pub fn record_latency_sample(record: &mut PeerRecord, sample: Duration) {
    let next = (1.0 - LATENCY_EWMA_ALPHA) * record.latency_ewma.as_secs_f32()
        + LATENCY_EWMA_ALPHA * sample.as_secs_f32();
    record.latency_ewma = Duration::from_secs_f32(next.max(0.0));
}

/// Mark the current uptime window as responsive — called on every observed
/// message event (the S2-P17 «на каждое successful message» seam; the
/// actual second-accrual happens in
/// [`accrue_uptime`](Self::accrue_uptime) at tick edges).
pub fn note_message(record: &mut PeerRecord) {
    record.responsive_since_anchor = true;
}

/// Accrue wall time since the last anchor into the uptime fraction
/// (ADR-0022: `responsive/observed` over windows between accruals).
///
/// A window counts as responsive when at least one message arrived in it
/// — a peer messaging inside every tick window keeps `norm = 1.0`; a
/// silent peer's denominator grows while its numerator does not. Clock
/// enters through the injected `now` only (checklist #1, `tick_at`
/// pattern).
pub fn accrue_uptime(record: &mut PeerRecord, now: Instant) {
    let elapsed = now.saturating_duration_since(record.uptime_anchor).as_secs();
    if elapsed == 0 {
        return;
    }
    record.observed_seconds = record.observed_seconds.saturating_add(elapsed);
    if record.responsive_since_anchor {
        record.uptime_seconds = record.uptime_seconds.saturating_add(elapsed);
    }
    record.uptime_anchor = now;
    record.responsive_since_anchor = false;
}

/// Which counter family dominates the current abuse — the `BanReason` for
/// score-band bans minted by `tick()` (no single triggering event to
/// blame). Mirrors ADR-0022's audit `reason` string weighting.
pub fn dominant_reason(record: &PeerRecord, config: &ScoringConfig) -> BanReason {
    let invalid_full = config.invalid_full_weight.max(1) as f32;
    let violation_full = config.violation_full_weight.max(1) as f32;
    let invalid_weight = config.w_invalid * (record.invalid_count as f32 / invalid_full);
    let violation_weight = config.w_violation * (record.violation_count as f32 / violation_full);
    if record.invalid_count > 0 && invalid_weight >= violation_weight {
        BanReason::InvalidBlocks
    } else {
        BanReason::ProtocolViolation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> Instant {
        Instant::now()
    }

    fn record() -> PeerRecord {
        PeerRecord {
            node_id: crate::noise::NodeId([0; 32]),
            addr: "10.0.0.1:8081".parse().unwrap(),
            last_seen: t0(),
            score: 1.0,
            latency_ewma: Duration::ZERO,
            invalid_count: 0,
            violation_count: 0,
            uptime_seconds: 0,
            observed_seconds: 0,
            uptime_anchor: t0(),
            responsive_since_anchor: false,
            verified: false,
        }
    }

    /// The ADR-0022 §Milestones table, pinned (healthy peer, latency 0).
    #[test]
    fn milestone_table_matches_adr0022() {
        let cfg = ScoringConfig::default();
        let mut r = record();

        assert_eq!(compute_score(&r, &cfg), 1.0);

        // invalid = 7 → 0.417 NoAction; 8 → 0.333 Warn.
        r.invalid_count = 7;
        let s = compute_score(&r, &cfg);
        assert!((s - 0.4167).abs() < 1e-3, "invalid 7 -> {s}");
        assert_eq!(decide(s, &cfg, t0()), BanDecision::NoAction);
        r.invalid_count = 8;
        let s = compute_score(&r, &cfg);
        assert!((s - 0.3333).abs() < 1e-3, "invalid 8 -> {s}");
        assert!(matches!(decide(s, &cfg, t0()), BanDecision::Warn));

        // invalid = 10 → 0.167 TempBan; 12 → 0.0 PersistentBan.
        r.invalid_count = 10;
        let s = compute_score(&r, &cfg);
        assert!((s - 0.1667).abs() < 1e-3, "invalid 10 -> {s}");
        assert!(matches!(
            decide(s, &cfg, t0()),
            BanDecision::TempBan { .. }
        ));
        r.invalid_count = 12;
        assert_eq!(compute_score(&r, &cfg), 0.0);
        assert!(matches!(
            decide(0.0, &cfg, t0()),
            BanDecision::PersistentBan { .. }
        ));

        // Violations: 16 → 0.200 Warn boundary (temp needs < 0.2);
        // 17 → 0.150 TempBan; 20 → 0.0 PersistentBan.
        let mut r = record();
        r.violation_count = 16;
        let s = compute_score(&r, &cfg);
        assert!((s - 0.2).abs() < 1e-4, "violations 16 -> {s}");
        assert_eq!(decide(s, &cfg, t0()), BanDecision::Warn);
        r.violation_count = 17;
        let s = compute_score(&r, &cfg);
        assert!((s - 0.15).abs() < 1e-4, "violations 17 -> {s}");
        assert!(matches!(
            decide(s, &cfg, t0()),
            BanDecision::TempBan { .. }
        ));
        r.violation_count = 20;
        assert_eq!(compute_score(&r, &cfg), 0.0);
    }

    /// ADR-0022 false-positive bound: worst-case latency, zero
    /// violations, fully responsive → 0.333 (Warn), never banned.
    #[test]
    fn slow_responsive_peer_floors_at_warn_band() {
        let cfg = ScoringConfig::default();
        let mut r = record();
        r.latency_ewma = Duration::from_millis(200);
        r.uptime_seconds = 600;
        r.observed_seconds = 600;
        let s = compute_score(&r, &cfg);
        assert!((s - 0.3333).abs() < 1e-3, "got {s}");
        assert_eq!(decide(s, &cfg, t0()), BanDecision::Warn);
    }

    #[test]
    fn ewma_converges_and_uptime_fraction_tracks_responsiveness() {
        let mut r = record();
        for _ in 0..100 {
            record_latency_sample(&mut r, Duration::from_millis(200));
        }
        // Fixed point 200 ms; after 100 steps the residual is (200)·0.9^100 ≈ 0.
        assert!((r.latency_ewma.as_secs_f32() * 1000.0 - 200.0).abs() < 1.0);

        let now = t0();
        // Window 1: responsive (message flag set) — both counters grow.
        note_message(&mut r);
        accrue_uptime(&mut r, now + Duration::from_secs(60));
        assert_eq!(r.uptime_seconds, 60);
        assert_eq!(r.observed_seconds, 60);
        assert_eq!(uptime_norm(&r), 1.0);

        // Window 2: silent — the fraction decays.
        accrue_uptime(&mut r, now + Duration::from_secs(120));
        assert_eq!(r.uptime_seconds, 60);
        assert_eq!(r.observed_seconds, 120);
        assert!((uptime_norm(&r) - 0.5).abs() < 1e-6);

        // Window 3: responsive again — recovers toward 1.0.
        note_message(&mut r);
        accrue_uptime(&mut r, now + Duration::from_secs(180));
        assert_eq!(r.uptime_seconds, 120);
        assert_eq!(r.observed_seconds, 180);
        assert!((uptime_norm(&r) - 0.6667).abs() < 1e-3);
    }

    #[test]
    fn ban_durations_follow_config() {
        let cfg = ScoringConfig::default();
        let now = t0();
        match decide(0.1, &cfg, now) {
            BanDecision::TempBan { until } => {
                assert_eq!(until, now + Duration::from_secs(3600))
            }
            other => panic!("expected TempBan, got {other:?}"),
        }
        match decide(0.0, &cfg, now) {
            BanDecision::PersistentBan { until } => {
                assert_eq!(until, now + Duration::from_secs(60 * 60 * 24 * 30))
            }
            other => panic!("expected PersistentBan, got {other:?}"),
        }
    }

    #[test]
    fn dominant_reason_prefers_heavier_abuse() {
        let cfg = ScoringConfig::default();
        let mut r = record();
        r.invalid_count = 10;
        r.violation_count = 1;
        assert_eq!(dominant_reason(&r, &cfg), BanReason::InvalidBlocks);
        r.invalid_count = 0;
        assert_eq!(dominant_reason(&r, &cfg), BanReason::ProtocolViolation);
    }
}
