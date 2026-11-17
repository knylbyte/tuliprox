use super::build_manifest_timeline_fingerprint;
use crate::{manifest_acceptance::*, HlsSessionKey};
use std::fmt::Write;

fn observation(
    index: usize,
    stream: &str,
    playlist_url: &str,
    sequence: u64,
    media_base: &str,
) -> HlsManifestCandidateObservation {
    let mut body = format!("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:{sequence}\n");
    for msn in sequence..sequence + 3 {
        let timestamp = chrono::DateTime::from_timestamp(i64::try_from(msn * 4).unwrap(), 0).unwrap();
        writeln!(body, "#EXT-X-PROGRAM-DATE-TIME:{}\n#EXTINF:4,\n{media_base}/common{msn}.ts", timestamp.to_rfc3339())
            .unwrap();
    }
    HlsManifestCandidateObservation {
        candidate_index: index,
        candidate_slot: index,
        effective_host: Some(url::Url::parse(playlist_url).unwrap().host_str().unwrap().to_owned()),
        host_relation: HlsCandidateHostRelation::OtherHost,
        host_local_media_sequence: sequence,
        host_local_highwater: Some(sequence + 2),
        local_sequence_relation: None,
        resource_timeline_evidence: HlsResourceTimelineEvidence::Eligible,
        timeline_fingerprint: build_manifest_timeline_fingerprint(&HlsSessionKey::new(1, stream), &body, playlist_url)
            .timeline,
        target_duration_secs: Some(4),
        manifest_fetch_elapsed_ms: 1,
        switch_segment_readiness: HlsSwitchSegmentReadiness::RequiresStaging,
        committed_content_anchor: HlsCommittedContentAnchorEvidence::Unavailable,
        emergency_evidence: HlsEmergencyAcceptanceEvidence::INCOMPATIBLE,
        evidence: HlsCrossHostAcceptanceEvidence::Insufficient,
    }
}

const MANIFEST_ROTATIONS: [(&str, &str); 4] = [
    ("https://cdn.example/video-low/index.m3u8", "https://cdn.example/video-high/index.m3u8"),
    ("https://cdn.example/index.m3u8?track=low", "https://cdn.example/index.m3u8?track=high"),
    ("https://cdn.example/index.m3u8?a=1&b=2", "https://cdn.example/index.m3u8?b=2&a=1"),
    ("https://cdn.example/index.m3u8", "https://cdn.example/index.m3u8?"),
];

#[test]
fn different_logical_sources_do_not_form_consensus() {
    for (left, right) in MANIFEST_ROTATIONS {
        for sequence in [100, 101, 103] {
            let samples = [observation(0, "first", left, 100, ""), observation(1, "second", right, sequence, "")];
            let cohorts = alternative_cohorts(&samples);
            assert_eq!(cohorts.len(), 2, "{left} vs {right}, MSN {sequence}");
            assert!(cohorts.iter().all(|cohort| {
                cohort.successful_samples == 1 && cohort.evidence == HlsCrossHostAcceptanceEvidence::Insufficient
            }));
            assert_eq!(
                evaluate_manifest_acceptance(HlsManifestAcceptanceInput {
                    preferred_min_target_duration_secs: None,
                    excluded_candidates: &[],
                    full_burst_completed: true,
                    current_burst_is_full_plan: true,
                    trigger: HlsManifestAcceptanceTrigger::RecoveryRequired,
                    previous_alternative: None,
                    observations: &samples,
                }),
                HlsManifestCommitPlan::HoldAlternative
            );
        }
    }
}

#[test]
fn different_logical_sources_requalify_landscape_and_burst_history() {
    for (left, right) in MANIFEST_ROTATIONS {
        let original = [observation(0, "first", left, 100, "")];
        let landscape = manifest_acceptance_landscape(&original);
        let previous = alternative_cohorts_with_history(&original, None, true).remove(0);
        for sequence in [100, 101, 103] {
            let changed = [observation(1, "second", right, sequence, "")];
            assert_eq!(
                classify_reduced_retry_landscape(&landscape, &changed),
                HlsReducedRetryLandscapeChange::TimelineConflict
            );
            let current = alternative_cohorts_with_history(&changed, Some(&previous), true).remove(0);
            assert_eq!((current.total_samples, current.consecutive_confirmed_full_bursts), (1, 1));
            assert_eq!(current.evidence, HlsCrossHostAcceptanceEvidence::Insufficient);
        }
    }
}

#[test]
fn sliding_windows_preserve_recovery_consensus_across_manifest_rotations() {
    for (first, second) in MANIFEST_ROTATIONS {
        for sequence in [100, 101, 103] {
            let samples = [
                observation(0, "shared", first, 100, "https://media-a.example"),
                observation(1, "shared", second, sequence, "http://media-b.example:8080"),
            ];
            if sequence == 100 {
                assert_eq!(samples[0].timeline_fingerprint, samples[1].timeline_fingerprint);
            } else {
                assert_ne!(
                    samples[0].timeline_fingerprint.normalized_resource_pattern_hash,
                    samples[1].timeline_fingerprint.normalized_resource_pattern_hash
                );
            }
            let cohorts = alternative_cohorts(&samples);
            assert_eq!(cohorts.len(), 1);
            assert_eq!(
                cohorts[0].evidence,
                HlsCrossHostAcceptanceEvidence::BurstConsensusNewEpoch { successful_samples: 2 }
            );
            let landscape = manifest_acceptance_landscape(&samples[..1]);
            assert_eq!(
                classify_reduced_retry_landscape(&landscape, &samples[1..]),
                HlsReducedRetryLandscapeChange::Unchanged
            );
            let previous = alternative_cohorts_with_history(&samples[..1], None, true).remove(0);
            let current = alternative_cohorts_with_history(&samples[1..], Some(&previous), true).remove(0);
            assert_eq!((current.total_samples, current.consecutive_confirmed_full_bursts), (2, 2));
        }
    }
}

#[test]
fn landscape_matches_the_existing_scope_among_same_host_cohorts() {
    for (left, right) in MANIFEST_ROTATIONS {
        let landscape = manifest_acceptance_landscape(&[
            observation(0, "first", left, 100, ""),
            observation(1, "second", right, 100, ""),
        ]);
        assert_eq!(landscape.alternatives.len(), 2);
        for sequence in [100, 101, 103] {
            assert_eq!(
                classify_reduced_retry_landscape(&landscape, &[observation(2, "second", right, sequence, "")]),
                HlsReducedRetryLandscapeChange::Unchanged
            );
        }
    }
}

#[test]
fn pdt_anchor_accepts_manifest_rotations_within_the_same_logical_source() {
    let mut pinned = observation(0, "shared", "https://origin.example/index.m3u8?track=low", 100, "");
    pinned.host_relation = HlsCandidateHostRelation::PinnedHost;
    for (stream, url, expected) in [
        (
            "shared",
            "http://cdn.example:8080/index.m3u8?track=low",
            HlsCrossHostAcceptanceEvidence::StrongTimelineAnchor { overlapping_segments: 3 },
        ),
        (
            "shared",
            "https://cdn.example/rotated/reload.m3u8?delivery=2",
            HlsCrossHostAcceptanceEvidence::StrongTimelineAnchor { overlapping_segments: 3 },
        ),
        (
            "other-stream",
            "https://cdn.example/rotated/reload.m3u8?delivery=2",
            HlsCrossHostAcceptanceEvidence::Insufficient,
        ),
    ] {
        let alternative = observation(1, stream, url, 100, "http://media.example:8080");
        let cohort = alternative_cohorts(&[pinned.clone(), alternative]).remove(0);
        assert_eq!(cohort.evidence, expected, "{stream}, {url}");
    }
}
