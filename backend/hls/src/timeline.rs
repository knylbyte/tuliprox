use super::{
    media_reserve::{HlsReadyMediaState, HlsReadyTimelineSnapshot, HlsReadyTimelineUnit},
    resource_identity::{
        HlsMediaPlaylistScope, HlsMediaResourceIdentity, HlsMediaResourceSemanticKey, HlsPublishedResourceHistory,
        HlsSegmentOccurrenceIdentity, HlsVerifiedOccurrenceBinding,
    },
    safe_proxy_session_id, HlsSession, MapCacheStatus, MapEntry, OriginMapFetchRef, OriginMapKey, ProxyMapId,
    ProxySessionId, SegmentCacheKey, SegmentFetchPriority, TransientResourceId,
};
use axum::http::StatusCode;
use log::info;
use std::{
    collections::{BTreeMap, HashMap},
    fmt,
    sync::{
        atomic::{AtomicU32, AtomicU64, Ordering},
        Arc,
    },
};
use tuliprox_parser::hls::origin_manifest::{
    ParsedByteRange, ParsedOriginManifest, ParsedOriginMap, ParsedOriginSegment,
};
use url::Url;

const SEGMENT_EXTENSIONS: &[&str] = &["ts", "mp4", "m4s", "m4v"];
const MAP_EXTENSIONS: &[&str] = &["mp4", "m4s", "m4v"];
pub const HLS_PROVISIONING_ORIGIN_EPOCH: u64 = u64::MAX;
pub const HLS_PROVISIONING_GAP_ORIGIN_EPOCH: u64 = u64::MAX - 1;
pub const HLS_PROVISIONING_TARGET_DURATION_SECS: u32 = 2;
pub const HLS_PROVISIONING_SEGMENT_DURATION_MS: u64 = 2_000;
const EFFECTIVE_ORIGIN_HOST_ID_DOMAIN: &[u8] = b"tuliprox/hls/effective-origin-host/v1\0";
const UNKNOWN_EFFECTIVE_ORIGIN_HOST_ID: u64 = 0;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct OriginSegmentKey {
    pub origin_epoch: u64,
    pub effective_host_id: u64,
    pub host_local_sequence: u64,
    /// Saturating sequence offset from the first segment committed in this host/epoch namespace.
    pub host_local_index: u32,
}

/// Returns a stable, non-reversible identifier for an effective manifest host.
///
/// The normalized host itself is deliberately not stored in timeline or cache identities. The domain-separated hash
/// is stable across processes and does not depend on Rust's randomized `HashMap` hasher.
pub fn effective_origin_host_id(effective_host: &str) -> u64 {
    let normalized_host = effective_host.trim().trim_end_matches('.').to_ascii_lowercase();
    let mut hasher = blake3::Hasher::new();
    hasher.update(EFFECTIVE_ORIGIN_HOST_ID_DOMAIN);
    hasher.update(normalized_host.as_bytes());
    let digest = hasher.finalize();
    let mut id_bytes = [0_u8; std::mem::size_of::<u64>()];
    id_bytes.copy_from_slice(&digest.as_bytes()[..std::mem::size_of::<u64>()]);
    let id = u64::from_le_bytes(id_bytes);
    if id == UNKNOWN_EFFECTIVE_ORIGIN_HOST_ID {
        1
    } else {
        id
    }
}

/// Volatile concrete origin URL for one segment download.
///
/// The URL is resolved against the final fetched manifest URL and may include a redirect/CDN host. Use it only as a
/// fetch target or sanitized diagnostics; logical occurrence identity is stored in `SegmentEntry::occurrence_identity`, and cache identity is the
/// proxy sequence based `SegmentCacheKey`.
#[derive(Clone, Eq, PartialEq)]
pub struct OriginSegmentFetchRef {
    /// Concrete URL used to refetch the segment object.
    ///
    /// This starts from `ParsedOriginSegment::resolved_origin_url`, which is resolved against the final manifest URL
    /// after redirects. It is a fetch target only; the occurrence evidence remains independent of this fetch target.
    pub resolved_origin_url: String,
    pub byte_range: Option<ParsedByteRange>,
    pub valid_until_ms: Option<u64>,
}

impl OriginSegmentFetchRef {
    pub fn is_valid_at(&self, now_ms: u64) -> bool {
        self.valid_until_ms.is_none_or(|valid_until_ms| now_ms <= valid_until_ms)
    }
}

impl SegmentEntry {
    pub fn media_resource_identity(&self) -> Option<HlsMediaResourceIdentity> {
        self.origin_fetch_ref
            .as_ref()
            .map(|fetch_ref| HlsMediaResourceIdentity::from_url(&fetch_ref.resolved_origin_url, fetch_ref.byte_range))
            .or(self.retained_resource_identity)
    }
}

impl fmt::Debug for OriginSegmentFetchRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OriginSegmentFetchRef")
            .field("resolved_origin_url", &"<redacted>")
            .field("byte_range", &self.byte_range)
            .field("valid_until_ms", &self.valid_until_ms)
            .finish()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum SegmentCacheStatus {
    Discovered,
    Queued { priority: SegmentFetchPriority, queued_at_ms: u64 },
    Fetching { priority: SegmentFetchPriority, started_at_ms: u64 },
    Ready { content_length: u64, ready_at_ms: u64 },
    CapacityDeferred { priority: SegmentFetchPriority, deferred_at_ms: u64 },
    FailedRetryable { failed_at_ms: u64, retry_after_ms: u64 },
    FailedPermanent { failed_at_ms: u64, status: Option<StatusCode> },
    Expired,
}

impl SegmentCacheStatus {
    /// True only while local capacity owns a concrete revision-bound retry.
    pub const fn awaits_capacity_recovery(&self) -> bool { matches!(self, Self::CapacityDeferred { .. }) }
}

/// Mutable access counters shared with response streams without holding a session lock.
#[derive(Debug, Default)]
pub struct CacheAccessState {
    active_readers: AtomicU32,
    last_accessed_at_ms: AtomicU64,
    pub(crate) retention: crate::publication_retention::HlsPublicationRetention,
}

impl CacheAccessState {
    pub fn new() -> Self { Self::default() }

    pub fn reader_started(&self, now_ms: u64) {
        self.active_readers.fetch_add(1, Ordering::AcqRel);
        self.last_accessed_at_ms.store(now_ms, Ordering::Release);
    }

    pub fn reader_finished(&self) {
        let mut current = self.active_readers.load(Ordering::Acquire);
        while current > 0 {
            match self.active_readers.compare_exchange_weak(current, current - 1, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return,
                Err(next) => current = next,
            }
        }
    }

    pub fn active_readers(&self) -> u32 { self.active_readers.load(Ordering::Acquire) }

    pub fn last_accessed_at_ms(&self) -> u64 { self.last_accessed_at_ms.load(Ordering::Acquire) }
}

impl PartialEq for CacheAccessState {
    fn eq(&self, other: &Self) -> bool {
        self.active_readers() == other.active_readers()
            && self.last_accessed_at_ms() == other.last_accessed_at_ms()
            && self.retention == other.retention
    }
}

impl Eq for CacheAccessState {}

pub fn default_content_type_for_segment_ext(extension: &str) -> &'static str {
    match extension {
        "ts" => "video/mp2t",
        "mp4" | "m4v" | "m4s" => "video/mp4",
        _ => "application/octet-stream",
    }
}

pub fn is_hls_provisioning_segment(entry: &SegmentEntry) -> bool {
    entry.origin_key.origin_epoch == HLS_PROVISIONING_ORIGIN_EPOCH
}

pub fn is_hls_provisioning_gap_segment(entry: &SegmentEntry) -> bool {
    entry.origin_key.origin_epoch == HLS_PROVISIONING_GAP_ORIGIN_EPOCH
}

#[derive(Clone, Eq, PartialEq)]
pub struct SegmentEntry {
    pub origin_key: OriginSegmentKey,
    pub proxy_seq: u64,
    /// Immutable session-local media offset; independent of the retained cache window.
    pub media_start_ms: u64,
    pub duration_ms: u64,
    pub proxy_file_ext: String,
    pub content_type: String,
    pub cache_key: SegmentCacheKey,
    pub discontinuity_before: bool,
    pub program_date_time: Option<String>,
    pub daterange_tags_before: Vec<String>,
    pub origin_byte_range: Option<ParsedByteRange>,
    pub map_ref: Option<ProxyMapId>,
    pub encryption: Option<HlsSegmentEncryption>,
    /// Immutable occurrence evidence, absent for synthetic provisioning entries.
    pub occurrence_identity: Option<HlsSegmentOccurrenceIdentity>,
    /// Legacy resource comparison retained while an account handoff invalidates the download URL.
    pub retained_resource_identity: Option<HlsMediaResourceIdentity>,
    pub origin_fetch_ref: Option<OriginSegmentFetchRef>,
    pub status: SegmentCacheStatus,
    pub last_rendered_at_ms: Option<u64>,
    pub access: Arc<CacheAccessState>,
}

#[derive(Clone)]
pub(crate) struct HlsRetainedGapRepair {
    pub(crate) slot: SegmentEntry,
    pub(crate) candidate: OriginSegmentFetchRef,
}

impl SegmentEntry {
    pub(crate) fn is_clear_ts_origin_404(&self) -> bool {
        matches!(self.status, SegmentCacheStatus::FailedPermanent { status: Some(StatusCode::NOT_FOUND), .. })
            && self.proxy_file_ext.eq_ignore_ascii_case("ts")
            && self.map_ref.is_none()
            && self.encryption.is_none()
    }
}

#[derive(Default)]
struct LiveMissingMedia {
    failed_origin: Option<OriginSegmentKey>,
    retired: bool,
    unresolved: bool,
    mapped_head: Option<(u64, OriginSegmentKey)>,
    skipped_boundaries: Vec<u64>,
}

impl LiveMissingMedia {
    fn record_failure(&mut self, entry: &SegmentEntry, unpublished_boundary: bool) {
        self.failed_origin = Some(entry.origin_key);
        self.mapped_head = None;
        if unpublished_boundary {
            self.skipped_boundaries.push(entry.proxy_seq);
        }
    }

    fn take_boundary(&mut self, successor: (u64, OriginSegmentKey), has_publication: bool) -> Option<(u64, Vec<u64>)> {
        let gap = std::mem::take(self);
        let (seq, origin) = successor;
        ((gap.failed_origin.is_some_and(|failed| {
            failed.origin_epoch == origin.origin_epoch && failed.host_local_sequence < origin.host_local_sequence
        }) || (gap.retired && (!gap.unresolved || !gap.skipped_boundaries.is_empty())))
            && (has_publication || !gap.skipped_boundaries.is_empty()))
        .then_some((seq, gap.skipped_boundaries))
    }
}

impl HlsSession {
    /// Locate the first held slot needed by this cursor, without falling back to
    /// a later window or to a resource known only through publication history.
    pub(crate) fn first_playback_blocker(&self, lease: &super::HlsAccessLease) -> Option<&SegmentEntry> {
        use super::media_reserve::HlsManifestDeliveryMode;
        if self.startup.is_some()
            || !matches!(self.mode, super::HlsSessionMode::NormalCacheTimeline)
            || lease.last_manifest_snapshot.as_ref()?.delivery_mode != HlsManifestDeliveryMode::NormalCacheTimeline
        {
            return None;
        }
        let cursor = &lease.playback_cursor;
        cursor.first_segment_started_at_ms?;
        let mut expected = match cursor.highest_contiguous_completed_proxy_seq {
            Some(completed) => completed.checked_add(1)?,
            None => cursor.first_requested_proxy_seq?,
        };
        for (&seq, entry) in self.segments.range(expected..) {
            if seq != expected {
                return None;
            }
            if !matches!(entry.status, SegmentCacheStatus::Ready { .. })
                || entry.map_ref.is_some()
                || entry.encryption.is_some()
            {
                return Some(entry);
            }
            expected = expected.checked_add(1)?;
        }
        None
    }

    pub(crate) fn has_retained_playback_gap(&self, lease: &super::HlsAccessLease) -> bool {
        lease.last_manifest_snapshot.as_ref().is_some_and(|snapshot| {
            self.retains_prior_group_member(snapshot)
                && self.last_rendered_manifest.as_ref().is_none_or(|rendered| {
                    self.group_member_start_proxy_seq.is_some_and(|start| rendered.last_proxy_seq < start)
                })
        }) || self
            .first_playback_blocker(lease)
            .is_some_and(|slot| slot.origin_key.origin_epoch < self.origin_epoch && slot.is_clear_ts_origin_404())
    }

    pub(crate) fn origin_discontinuity_before(&self, entry: &SegmentEntry) -> bool {
        self.origin_content.relocated_origin_discontinuities.contains(&entry.proxy_seq)
            || (entry.discontinuity_before
                && !self.origin_content.proxy_only_discontinuities.contains(&entry.proxy_seq))
    }

    pub(crate) fn record_retired_live_mapping(&mut self, seq: u64, discontinuity_before: bool) {
        if self.live_discontinuity_published_through.is_some_and(|published| seq <= published) {
            return;
        }
        let left = self
            .retired_live_mappings
            .range(..seq)
            .next_back()
            .filter(|(_, end)| end.checked_add(1) == Some(seq))
            .map(|(&start, _)| start);
        let right = self.retired_live_mappings.remove(&seq.saturating_add(1));
        self.retired_live_mappings.insert(left.unwrap_or(seq), right.unwrap_or(seq));
        if discontinuity_before {
            self.live_recovery_discontinuities.entry(seq).or_default();
        }
    }

    pub(crate) fn reconcile_live_discontinuities(&mut self) {
        if self.origin_source.archive_reference.is_some() {
            return;
        }
        let Some(origin_head) = self.publishable_origin_head_proxy_seq else { return };
        let published_through = self.live_discontinuity_published_through;
        let start = published_through.map_or(0, |seq| seq.saturating_add(1));
        let mut boundaries = Vec::new();
        let mut gap = LiveMissingMedia::default();
        let has_publication = self.published_live_origin_baseline.is_some();
        let mut recoveries = self.live_recovery_discontinuities.range(start..).peekable();
        let mut retired = self.retired_live_mappings.iter().filter(|(_, last)| **last >= start).peekable();

        for (&seq, entry) in self.segments.range(start..) {
            while let Some((&boundary, published)) = recoveries.next_if(|(boundary, _)| **boundary < seq) {
                if published.is_none()
                    && self
                        .retired_live_mappings
                        .range(..=boundary)
                        .next_back()
                        .is_some_and(|(_, last)| boundary <= *last)
                {
                    gap.skipped_boundaries.push(boundary);
                    continue;
                }
                // A collected successor still owns its stored transition.
                gap = LiveMissingMedia::default();
            }
            while retired.next_if(|(_, last)| **last < seq).is_some() {
                gap.retired = true;
                gap.mapped_head = None;
            }
            if is_hls_provisioning_segment(entry) || is_hls_provisioning_gap_segment(entry) {
                continue;
            }
            if entry.last_rendered_at_ms.is_some() {
                gap = LiveMissingMedia::default();
                continue;
            }
            let recovery = recoveries.next_if(|(boundary, _)| **boundary == seq).map(|(_, published)| published);
            if matches!(entry.status, SegmentCacheStatus::FailedPermanent { .. }) {
                let published = recovery.is_some_and(Option::is_some);
                if published {
                    gap = LiveMissingMedia::default();
                }
                gap.record_failure(entry, entry.discontinuity_before && !published);
                continue;
            }
            // The mapped jump or handoff already accounts for the preceding
            // gap, even if its earlier marker belonged to another origin epoch.
            if let Some(published) = recovery {
                if published.is_none() && !gap.skipped_boundaries.is_empty() {
                    boundaries.push((seq, std::mem::take(&mut gap.skipped_boundaries)));
                }
                gap = LiveMissingMedia::default();
                continue;
            }
            let ready = matches!(entry.status, SegmentCacheStatus::Ready { .. });
            if !ready && seq < origin_head {
                gap.unresolved = true;
                continue;
            }
            if gap.failed_origin.is_none() && !gap.retired {
                gap.unresolved = !ready;
                continue;
            }
            // Retained READY media can close a gap after the origin has moved on.
            // Inside the accepted window, keep the earliest viable successor
            // while its fetch is still in progress.
            let successor = *gap.mapped_head.get_or_insert((seq, entry.origin_key));
            if ready {
                boundaries.extend(gap.take_boundary(successor, has_publication));
            }
        }
        // An accepted window may not have started fetching yet. Record the
        // confirmed jump at its mapped successor before the next render.
        if let Some(head) = gap.mapped_head.filter(|_| gap.failed_origin.is_some() && recoveries.next().is_none()) {
            boundaries.extend(gap.take_boundary(head, has_publication));
        }
        for (seq, skipped) in boundaries {
            for failed in skipped {
                self.remove_unpublished_discontinuity(failed);
            }
            self.mark_live_recovery_discontinuity(seq);
        }
    }

    fn remove_unpublished_discontinuity(&mut self, seq: u64) {
        self.live_recovery_discontinuities.remove(&seq);
        if let Some(entry) = self.segments.get_mut(&seq) {
            if !self.origin_content.proxy_only_discontinuities.remove(&seq) {
                self.origin_content.relocated_origin_discontinuities.insert(seq);
            }
            entry.discontinuity_before = false;
        }
    }

    fn mark_live_recovery_discontinuity(&mut self, head: u64) {
        let entry = self.segments.get_mut(&head).expect("live head is retained");
        if !entry.discontinuity_before {
            entry.discontinuity_before = true;
            self.origin_content.proxy_only_discontinuities.insert(head);
        }
        self.live_recovery_discontinuities.entry(head).or_default();
    }

    /// Read the overlap from the complete candidate before the handoff trims
    /// existing mappings. The repair keeps the old proxy slot and media clock.
    pub(crate) fn retained_gap_repair(
        &self,
        lease: &super::HlsAccessLease,
        manifest: &ParsedOriginManifest,
        anchor_proxy_seq: u64,
    ) -> Option<HlsRetainedGapRepair> {
        let slot = self.first_playback_blocker(lease)?;
        let anchor = self.segments.get(&anchor_proxy_seq)?;
        if !slot.is_clear_ts_origin_404()
            || slot.proxy_seq >= anchor_proxy_seq
            || slot.origin_key.origin_epoch != anchor.origin_key.origin_epoch
            || slot.origin_key.effective_host_id != anchor.origin_key.effective_host_id
        {
            return None;
        }
        let start = manifest.segments.iter().position(|seg| seg.origin_seq == slot.origin_key.host_local_sequence)?;
        let end = manifest.segments.iter().position(|seg| seg.origin_seq == anchor.origin_key.host_local_sequence)?;
        let candidate = &manifest.segments[start];
        let elapsed =
            manifest.segments.get(start..end)?.iter().try_fold(0_u64, |ms, seg| ms.checked_add(seg.duration_ms))?;
        if candidate.map_ref.is_some()
            || candidate.encryption.is_some()
            || candidate.duration_ms != slot.duration_ms
            || candidate.discontinuity_before != self.origin_discontinuity_before(slot)
            || candidate.program_date_time != slot.program_date_time
            || slot.media_start_ms.checked_add(elapsed)? != anchor.media_start_ms
            || !slot
                .occurrence_identity?
                .resource()
                .matches(HlsMediaResourceIdentity::from_url(&candidate.resource_uri, candidate.origin_byte_range))
        {
            return None;
        }
        Some(HlsRetainedGapRepair {
            slot: slot.clone(),
            candidate: OriginSegmentFetchRef {
                resolved_origin_url: candidate.resolved_origin_url.clone(),
                byte_range: candidate.origin_byte_range,
                valid_until_ms: None,
            },
        })
    }
}

impl fmt::Debug for SegmentEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SegmentEntry")
            .field("origin_key", &self.origin_key)
            .field("proxy_seq", &self.proxy_seq)
            .field("media_start_ms", &self.media_start_ms)
            .field("duration_ms", &self.duration_ms)
            .field("proxy_file_ext", &self.proxy_file_ext)
            .field("content_type", &self.content_type)
            .field("cache_key", &self.cache_key)
            .field("discontinuity_before", &self.discontinuity_before)
            .field("program_date_time", &self.program_date_time)
            .field("daterange_tags_before", &self.daterange_tags_before)
            .field("origin_byte_range", &self.origin_byte_range)
            .field("map_ref", &self.map_ref)
            .field("encryption", &self.encryption)
            .field("occurrence_identity", &self.occurrence_identity)
            .field("retained_resource_identity", &self.retained_resource_identity)
            .field("origin_fetch_ref", &self.origin_fetch_ref)
            .field("status", &self.status)
            .field("last_rendered_at_ms", &self.last_rendered_at_ms)
            .field("active_readers", &self.access.active_readers())
            .field("last_accessed_at_ms", &self.access.last_accessed_at_ms())
            .finish()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum TimelineMapError {
    UnsupportedSegmentExtension,
    UnsupportedMapExtension,
    ProxySequenceOverflow,
    ProxyMapIdOverflow,
    MissingKeyResource,
    PublishedResourceReplay {
        previous_proxy_tail: Option<u64>,
        existing_proxy_seq: u64,
        candidate_position: usize,
        candidate_origin_seq: u64,
        resource_key: HlsMediaResourceSemanticKey,
        decision: HlsResourceReplayDecision,
    },
    OriginSequenceResourceConflict {
        existing_proxy_seq: u64,
        candidate_origin_seq: u64,
    },
    OriginRolloverResourceReplay {
        existing_proxy_seq: u64,
        candidate_origin_seq: u64,
    },
}

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq)]
pub enum HlsResourceReplayDecision {
    RejectReplayOnly,
    RejectContradictoryOrder,
}

impl HlsResourceReplayDecision {
    pub const fn as_log_value(self) -> &'static str {
        match self {
            Self::RejectReplayOnly => "reject-replay-only",
            Self::RejectContradictoryOrder => "reject-contradictory-order",
        }
    }
}

/// Lease-materializable encryption state bound to an opaque key-resource route.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HlsSegmentEncryption {
    pub resource_id: TransientResourceId,
    pub resource_extension: String,
    pub iv: Option<String>,
    pub key_format: Option<String>,
    pub key_format_versions: Option<String>,
}

/// Mutation-free view of the exact entries an explicit origin handoff would retain or append.
///
/// Callers may use these cache keys to stage the first switch segment and its MAP. A later commit is safe only after
/// the caller has revalidated its session/acceptance generation; if the session changed in between, a fresh preview
/// must be created.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HlsOriginHandoffPreview {
    pub origin_epoch: u64,
    pub segments: Vec<SegmentEntry>,
    pub maps: Vec<MapEntry>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum HlsOriginHandoffPreviewError {
    Timeline(TimelineMapError),
    PreviewInconsistent,
}

impl From<TimelineMapError> for HlsOriginHandoffPreviewError {
    fn from(value: TimelineMapError) -> Self { Self::Timeline(value) }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum TimelineApplyMode {
    CurrentOrigin,
    ExplicitHandoff {
        discontinuity_sequence: u64,
    },
    /// Handoff to a rebased origin baseline. When no viewer holds the
    /// retained head, a gap moves publication to the new window head.
    Rebase {
        retained_head_in_use: bool,
    },
}

impl TimelineApplyMode {
    const fn handoff_discontinuity_sequence(self) -> Option<u64> {
        match self {
            Self::CurrentOrigin => None,
            Self::ExplicitHandoff { discontinuity_sequence } => Some(discontinuity_sequence),
            Self::Rebase { .. } => Some(0),
        }
    }
}

#[derive(Debug, Default)]
struct TimelineApplyResult {
    segment_proxy_seqs: Vec<u64>,
    map_ids: Vec<ProxyMapId>,
}

#[derive(Clone)]
#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
struct TimelineDraft {
    session_key: super::HlsSessionKey,
    proxy_session_id: ProxySessionId,
    origin_epoch: u64,
    origin_epoch_effective_host_id: Option<u64>,
    origin_epoch_sequence_base: Option<u64>,
    origin_seq_highwater: Option<u64>,
    proxy_next_seq: Option<u64>,
    next_media_start_ms: u64,
    origin_to_proxy: HashMap<OriginSegmentKey, u64>,
    published_resource_history: HlsPublishedResourceHistory,
    verified_origin_bindings: Vec<HlsVerifiedOccurrenceBinding>,
    known_verified_bindings: Vec<HlsVerifiedOccurrenceBinding>,
    discontinuity_sequence: u64,
    pending_handoff_discontinuity_sequence: Option<u64>,
    pending_origin_epoch_handoff: bool,
    fresh_manifest_commit_required: Option<crate::HlsFreshManifestRequiredReason>,
    group_member_start_proxy_seq: Option<u64>,
    new_proxy_only_discontinuities: Vec<u64>,
    new_live_recovery_discontinuities: Vec<u64>,
    segments: BTreeMap<u64, SegmentEntry>,
    maps: BTreeMap<ProxyMapId, MapEntry>,
    origin_map_to_proxy: HashMap<(u64, HlsMediaResourceIdentity), ProxyMapId>,
    next_proxy_map_id: u64,
    publishable_origin_head_proxy_seq: Option<u64>,
    publishable_origin_tail_proxy_seq: Option<u64>,
    origin_version: Option<u16>,
    target_duration: Option<u32>,
    independent_segments: bool,
    origin_window_proxy_seqs: Vec<u64>,
    last_rendered_first_proxy_seq: Option<u64>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum OriginEpochTransitionReason {
    Rollover,
    Handoff,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum CandidateSegmentDisposition {
    CurrentMapping(u64),
    PublishedOverlap(u64),
    New,
}

#[derive(Debug)]
struct CandidateTimelinePlan<'a> {
    segments: Vec<&'a ParsedOriginSegment>,
    trimmed_published_prefix: bool,
    /// The candidate repeats the publishable tail, so its new media continues
    /// the retained timeline without a gap.
    overlaps_publishable_tail: bool,
}

impl From<&HlsSession> for TimelineDraft {
    fn from(session: &HlsSession) -> Self {
        Self {
            session_key: session.playlist_source_key().into_owned(),
            proxy_session_id: session.proxy_session_id.clone(),
            origin_epoch: session.origin_epoch,
            origin_epoch_effective_host_id: session.origin_epoch_effective_host_id,
            origin_epoch_sequence_base: session.origin_epoch_sequence_base,
            origin_seq_highwater: session.origin_seq_highwater,
            proxy_next_seq: session.proxy_next_seq,
            next_media_start_ms: session.next_media_start_ms,
            origin_to_proxy: session.origin_to_proxy.clone(),
            published_resource_history: session.published_resource_history.clone(),
            verified_origin_bindings: crate::refresh::origin_probe::HlsOriginContentEvidence::verified_bindings(
                session,
            ),
            known_verified_bindings: session.origin_content.known_verified_bindings().collect(),
            discontinuity_sequence: session.discontinuity_sequence,
            pending_handoff_discontinuity_sequence: session.pending_handoff_discontinuity_sequence,
            pending_origin_epoch_handoff: session.pending_origin_epoch_handoff,
            fresh_manifest_commit_required: session.fresh_manifest_commit_required,
            group_member_start_proxy_seq: session.group_member_start_proxy_seq,
            new_proxy_only_discontinuities: Vec::new(),
            new_live_recovery_discontinuities: Vec::new(),
            segments: session.segments.clone(),
            maps: session.maps.clone(),
            origin_map_to_proxy: session.origin_map_to_proxy.clone(),
            next_proxy_map_id: session.next_proxy_map_id,
            publishable_origin_head_proxy_seq: session.publishable_origin_head_proxy_seq,
            publishable_origin_tail_proxy_seq: session.publishable_origin_tail_proxy_seq,
            origin_version: session.origin_version,
            target_duration: session.target_duration,
            independent_segments: session.independent_segments,
            origin_window_proxy_seqs: session.live_origin_sequences().to_vec(),
            last_rendered_first_proxy_seq: session
                .last_rendered_manifest
                .as_ref()
                .map(|manifest| manifest.first_proxy_seq),
        }
    }
}

impl HlsSession {
    #[cfg(any(test, feature = "test-support"))]
    pub fn apply_origin_manifest(&mut self, manifest: &ParsedOriginManifest) -> Result<(), TimelineMapError> {
        let effective_host_id = inferred_effective_origin_host_id(manifest);
        self.apply_origin_manifest_for_host(manifest, effective_host_id)
    }

    /// Applies a manifest in the namespace of its effective manifest host.
    ///
    /// A different host may start a new transport epoch; unchanged logical occurrences retain their proxy slots.
    /// The caller must derive the ID from the final effective manifest host, not from a provider
    /// URL or an individual absolute segment URI.
    pub fn apply_origin_manifest_for_host(
        &mut self,
        manifest: &ParsedOriginManifest,
        effective_host_id: u64,
    ) -> Result<(), TimelineMapError> {
        let mut draft = TimelineDraft::from(&*self);
        draft.apply_manifest(manifest, effective_host_id, TimelineApplyMode::CurrentOrigin)?;
        self.commit_timeline_draft(draft);
        Ok(())
    }

    /// Applies a checked cache-anchored continuation without opening a transport-only content epoch.
    pub(crate) fn apply_continuous_origin_manifest(
        &mut self,
        manifest: &ParsedOriginManifest,
        effective_host_id: u64,
    ) -> Result<(), TimelineMapError> {
        let mut draft = TimelineDraft::from(&*self);
        draft.origin_epoch_effective_host_id = Some(effective_host_id);
        draft.apply_manifest(manifest, effective_host_id, TimelineApplyMode::CurrentOrigin)?;
        self.commit_timeline_draft(draft);
        Ok(())
    }

    /// Revalidates an origin baseline, retaining overlap proven to continue the current epoch.
    ///
    /// Otherwise a retained timeline is separated with an explicit discontinuity. If only stale baseline metadata remains,
    /// the epoch still advances, but no synthetic discontinuity is added ahead of the first media ever published by
    /// this session. `retained_head_in_use` keeps the published head for viewers
    /// that still hold media protection on the retained timeline.
    pub fn apply_origin_rebase_manifest(
        &mut self,
        manifest: &ParsedOriginManifest,
        effective_host_id: u64,
        retained_head_in_use: bool,
    ) -> Result<(), TimelineMapError> {
        let mut draft = TimelineDraft::from(&*self);
        if draft.segments.is_empty() {
            if draft.has_origin_namespace_baseline() {
                draft.start_new_origin_epoch(manifest, OriginEpochTransitionReason::Handoff)?;
                if !draft.published_resource_history.is_empty() {
                    draft.pending_handoff_discontinuity_sequence = Some(0);
                }
            }
            draft.apply_manifest(manifest, effective_host_id, TimelineApplyMode::CurrentOrigin)?;
        } else if draft.can_continue_origin_manifest(manifest, effective_host_id)
            || (self.last_rendered_manifest.is_none()
                && draft.published_resource_history.is_empty()
                && draft.origin_epoch_effective_host_id == Some(effective_host_id)
                && draft.pending_handoff_discontinuity_sequence.is_none()
                && !draft.pending_origin_epoch_handoff)
        {
            // Revalidation does not invalidate verified current overlap. Unpublished
            // overlap also needs refreshed fetch references rather than prefix trimming.
            draft.apply_manifest(manifest, effective_host_id, TimelineApplyMode::CurrentOrigin)?;
        } else {
            draft.apply_manifest(manifest, effective_host_id, TimelineApplyMode::Rebase { retained_head_in_use })?;
        }
        self.commit_timeline_draft(draft);
        Ok(())
    }

    /// Previews the exact timeline/cache entries for an explicit handoff without mutating this session.
    pub fn preview_origin_handoff_manifest(
        &self,
        manifest: &ParsedOriginManifest,
        effective_host_id: u64,
        discontinuity_sequence: u64,
    ) -> Result<HlsOriginHandoffPreview, HlsOriginHandoffPreviewError> {
        let mut draft = TimelineDraft::from(self);
        let result = draft.apply_manifest(
            manifest,
            effective_host_id,
            TimelineApplyMode::ExplicitHandoff { discontinuity_sequence },
        )?;
        handoff_preview_from_draft(&draft, result)
    }

    pub(crate) fn preview_verified_restart(
        &self,
        manifest: &ParsedOriginManifest,
        effective_host_id: u64,
    ) -> Result<HlsOriginHandoffPreview, HlsOriginHandoffPreviewError> {
        let mut draft = TimelineDraft::from(self);
        let result = draft.apply_restart(manifest, effective_host_id)?;
        handoff_preview_from_draft(&draft, result)
    }

    pub(crate) fn apply_verified_restart(
        &mut self,
        manifest: &ParsedOriginManifest,
        effective_host_id: u64,
        expected: &HlsOriginHandoffPreview,
    ) -> Result<(), HlsOriginHandoffPreviewError> {
        let mut draft = TimelineDraft::from(&*self);
        let result = draft.apply_restart(manifest, effective_host_id)?;
        if handoff_preview_from_draft(&draft, result)? != *expected {
            return Err(HlsOriginHandoffPreviewError::PreviewInconsistent);
        }
        self.commit_timeline_draft(draft);
        Ok(())
    }

    pub(crate) fn apply_manifest_restart(
        &mut self,
        manifest: &ParsedOriginManifest,
        effective_host_id: u64,
    ) -> Result<(), TimelineMapError> {
        let mut draft = TimelineDraft::from(&*self);
        draft.apply_restart(manifest, effective_host_id)?;
        self.commit_timeline_draft(draft);
        Ok(())
    }

    pub(crate) fn check_rollover_replay(&self, manifest: &ParsedOriginManifest) -> Result<(), TimelineMapError> {
        check_rollover_replay(
            &self.playlist_source_key(),
            manifest,
            &self.segments,
            &self.published_resource_history,
            self.origin_content.known_verified_bindings(),
        )
    }

    pub(crate) fn apply_verified_query_manifest(
        &mut self,
        manifest: &ParsedOriginManifest,
        effective_host_id: u64,
        bindings: &[HlsVerifiedOccurrenceBinding],
    ) -> Result<(), TimelineMapError> {
        let mut draft = TimelineDraft::from(&*self);
        draft.verified_origin_bindings.extend_from_slice(bindings);
        draft.origin_epoch_effective_host_id = Some(effective_host_id);
        draft.apply_manifest(manifest, effective_host_id, TimelineApplyMode::CurrentOrigin)?;
        self.commit_timeline_draft(draft);
        Ok(())
    }

    /// Commits an explicit handoff only if a freshly built draft still exactly matches the previously staged preview.
    ///
    /// The orchestration layer must call this while holding the session write lock and after checking its acceptance
    /// generation. Mapping, fetch/dependency or published-byte changes reject the preview without mutation.
    /// A retained overlap may finish downloading while the new continuation is staged; that READY advance is preserved.
    pub fn apply_origin_handoff_manifest_if_preview_matches(
        &mut self,
        manifest: &ParsedOriginManifest,
        effective_host_id: u64,
        discontinuity_sequence: u64,
        expected_preview: &HlsOriginHandoffPreview,
    ) -> Result<(), HlsOriginHandoffPreviewError> {
        let mut draft = TimelineDraft::from(&*self);
        let result = draft.apply_manifest(
            manifest,
            effective_host_id,
            TimelineApplyMode::ExplicitHandoff { discontinuity_sequence },
        )?;
        let actual_preview = handoff_preview_from_draft(&draft, result)?;
        if actual_preview.origin_epoch != expected_preview.origin_epoch
            || actual_preview.maps != expected_preview.maps
            || actual_preview.segments.len() != expected_preview.segments.len()
            || !actual_preview.segments.iter().zip(&expected_preview.segments).all(|(actual, expected)| {
                if actual == expected {
                    return true;
                }
                if self.segments.contains_key(&actual.proxy_seq)
                    && matches!(actual.status, SegmentCacheStatus::Ready { .. })
                    && !matches!(expected.status, SegmentCacheStatus::Ready { .. })
                {
                    let mut advanced = expected.clone();
                    advanced.status = actual.status.clone();
                    return advanced == *actual;
                }
                false
            })
        {
            return Err(HlsOriginHandoffPreviewError::PreviewInconsistent);
        }
        self.commit_timeline_draft(draft);
        Ok(())
    }

    fn commit_timeline_draft(&mut self, draft: TimelineDraft) {
        if self.origin_epoch != draft.origin_epoch {
            self.origin_control.clear_manifest_hosts();
        }
        self.origin_epoch = draft.origin_epoch;
        self.origin_epoch_effective_host_id = draft.origin_epoch_effective_host_id;
        self.origin_epoch_sequence_base = draft.origin_epoch_sequence_base;
        self.origin_seq_highwater = draft.origin_seq_highwater;
        self.proxy_next_seq = draft.proxy_next_seq;
        self.next_media_start_ms = draft.next_media_start_ms;
        self.origin_to_proxy = draft.origin_to_proxy;
        self.published_resource_history = draft.published_resource_history;
        self.discontinuity_sequence = draft.discontinuity_sequence;
        self.pending_handoff_discontinuity_sequence = draft.pending_handoff_discontinuity_sequence;
        self.pending_origin_epoch_handoff = draft.pending_origin_epoch_handoff;
        self.segments = draft.segments;
        self.origin_content.proxy_only_discontinuities.retain(|seq| self.segments.contains_key(seq));
        self.origin_content.relocated_origin_discontinuities.retain(|seq| self.segments.contains_key(seq));
        self.origin_content.proxy_only_discontinuities.extend(draft.new_proxy_only_discontinuities);
        self.live_recovery_discontinuities
            .extend(draft.new_live_recovery_discontinuities.into_iter().map(|seq| (seq, None)));
        self.maps = draft.maps;
        self.origin_map_to_proxy = draft.origin_map_to_proxy;
        self.next_proxy_map_id = draft.next_proxy_map_id;
        self.publishable_origin_head_proxy_seq = draft.publishable_origin_head_proxy_seq;
        self.publishable_origin_tail_proxy_seq = draft.publishable_origin_tail_proxy_seq;
        self.origin_version = draft.origin_version;
        self.target_duration = draft.target_duration;
        self.independent_segments = draft.independent_segments;
        self.origin_window_durations_ms = draft
            .origin_window_proxy_seqs
            .iter()
            .filter_map(|seq| self.segments.get(seq).map(|entry| entry.duration_ms))
            .collect();
        self.origin_window_proxy_seqs = Some(draft.origin_window_proxy_seqs);
        if let Some(startup) = self.startup.as_mut() {
            startup.revisions.retain(|seq, _| self.segments.contains_key(seq));
        }
        self.reconcile_live_discontinuities();
    }

    /// Captures the contiguous timeline beginning at the lease's first advertised sequence.
    ///
    /// The snapshot contains only metadata and is safe to evaluate after releasing the session lock.
    pub fn ready_timeline_snapshot(&self, first_proxy_seq: u64, now_ms: u64) -> HlsReadyTimelineSnapshot {
        let mut expected_proxy_seq = first_proxy_seq;
        let mut units = Vec::new();
        for (&proxy_seq, segment) in self.segments.range(first_proxy_seq..) {
            if proxy_seq != expected_proxy_seq {
                break;
            }
            let required_map_ready = segment.map_ref.is_none_or(|map_id| {
                self.maps.get(&map_id).is_some_and(|map| matches!(map.status, super::MapCacheStatus::Ready { .. }))
            });
            let key_ready_valid_until_ms = segment.encryption.as_ref().and_then(|encryption| {
                self.transient.ready_key_object_valid_until_ms(
                    &self.proxy_session_id,
                    &encryption.resource_id,
                    &encryption.resource_extension,
                    now_ms,
                )
            });
            units.push(HlsReadyTimelineUnit {
                proxy_seq,
                start_ms: segment.media_start_ms,
                duration_ms: segment.duration_ms,
                state: if matches!(segment.status, SegmentCacheStatus::Ready { .. }) {
                    HlsReadyMediaState::Ready
                } else {
                    HlsReadyMediaState::NotReady
                },
                required_map_ready,
                required_key_ready: segment.encryption.is_none() || key_ready_valid_until_ms.is_some(),
                key_ready_valid_until_ms,
            });
            expected_proxy_seq = expected_proxy_seq.saturating_add(1);
        }
        HlsReadyTimelineSnapshot { units: Arc::from(units) }
    }

    /// Reports whether local capacity is the first blocker at the READY
    /// boundary. Later deferred prefetches and unavailable MAP/key dependencies
    /// cannot suppress terminal handling for an unrelated origin failure.
    pub fn capacity_recovery_blocks_ready_timeline(&self, timeline: &HlsReadyTimelineSnapshot) -> bool {
        timeline
            .units
            .iter()
            .find(|unit| {
                unit.state != HlsReadyMediaState::Ready || !unit.required_map_ready || !unit.required_key_ready
            })
            .is_some_and(|unit| {
                unit.required_map_ready
                    && unit.required_key_ready
                    && self
                        .segments
                        .get(&unit.proxy_seq)
                        .is_some_and(|segment| segment.status.awaits_capacity_recovery())
            })
    }
}

fn handoff_preview_from_draft(
    draft: &TimelineDraft,
    result: TimelineApplyResult,
) -> Result<HlsOriginHandoffPreview, HlsOriginHandoffPreviewError> {
    let segments = result
        .segment_proxy_seqs
        .into_iter()
        .map(|proxy_seq| {
            draft.segments.get(&proxy_seq).cloned().ok_or(HlsOriginHandoffPreviewError::PreviewInconsistent)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let maps = result
        .map_ids
        .into_iter()
        .map(|map_id| draft.maps.get(&map_id).cloned().ok_or(HlsOriginHandoffPreviewError::PreviewInconsistent))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(HlsOriginHandoffPreview { origin_epoch: draft.origin_epoch, segments, maps })
}

fn check_rollover_replay(
    key: &super::HlsSessionKey,
    manifest: &ParsedOriginManifest,
    segments: &BTreeMap<u64, SegmentEntry>,
    history: &HlsPublishedResourceHistory,
    verified: impl Iterator<Item = HlsVerifiedOccurrenceBinding>,
) -> Result<(), TimelineMapError> {
    if manifest.origin_manifest_sequence != 0 {
        return Ok(());
    }
    let Some(scope) = HlsMediaPlaylistScope::from_session(key) else { return Ok(()) };
    let known = history
        .entries_across_epochs()
        .chain(segments.values().filter_map(|entry| entry.occurrence_identity.map(|id| (id, entry.proxy_seq))))
        .chain(verified.map(|binding| (binding.candidate, binding.proxy_seq)))
        .filter(|(id, _)| id.playlist_scope() == scope)
        .map(|(id, seq)| (id.resource(), seq))
        .collect::<HashMap<_, _>>();
    for segment in &manifest.segments {
        let identity = HlsSegmentOccurrenceIdentity::from_parsed(scope, segment);
        if let Some(&existing_proxy_seq) = known.get(&identity.resource()) {
            return Err(TimelineMapError::OriginRolloverResourceReplay {
                existing_proxy_seq,
                candidate_origin_seq: segment.origin_seq,
            });
        }
    }
    Ok(())
}

impl TimelineDraft {
    fn apply_restart(
        &mut self,
        manifest: &ParsedOriginManifest,
        effective_host_id: u64,
    ) -> Result<TimelineApplyResult, TimelineMapError> {
        self.start_new_origin_epoch(manifest, OriginEpochTransitionReason::Handoff)?;
        self.published_resource_history.begin_restart(self.proxy_next_seq.unwrap_or_default());
        self.pending_handoff_discontinuity_sequence = Some(0);
        self.pending_origin_epoch_handoff = false;
        self.publishable_origin_head_proxy_seq = None;
        self.publishable_origin_tail_proxy_seq = None;
        self.apply_manifest(manifest, effective_host_id, TimelineApplyMode::CurrentOrigin)
    }

    fn can_continue_origin_manifest(&self, manifest: &ParsedOriginManifest, effective_host_id: u64) -> bool {
        let Some(highwater) = self.origin_seq_highwater else { return false };
        if self.origin_epoch_effective_host_id != Some(effective_host_id)
            || self.pending_handoff_discontinuity_sequence.is_some()
            || self.pending_origin_epoch_handoff
            || manifest.segments.first().is_none_or(|segment| segment.origin_seq > highwater)
            || manifest.segments.last().is_none_or(|segment| segment.origin_seq < highwater)
        {
            return false;
        }

        let mut previous_proxy_seq: Option<u64> = None;
        for parsed in &manifest.segments {
            match self.classify_candidate_segment(parsed) {
                Ok(CandidateSegmentDisposition::CurrentMapping(proxy_seq)) if parsed.origin_seq <= highwater => {
                    if previous_proxy_seq.is_some_and(|previous| previous.checked_add(1) != Some(proxy_seq))
                        || self.segments[&proxy_seq].duration_ms != parsed.duration_ms
                    {
                        return false;
                    }
                    previous_proxy_seq = Some(proxy_seq);
                }
                Ok(CandidateSegmentDisposition::New) if parsed.origin_seq > highwater => {}
                _ => return false,
            }
        }
        // The overlap must reach the current tail; a historical match alone
        // cannot establish continuity or insert media before newer proxy entries.
        previous_proxy_seq.and_then(|seq| seq.checked_add(1)) == self.proxy_next_seq
    }

    fn apply_manifest(
        &mut self,
        manifest: &ParsedOriginManifest,
        effective_host_id: u64,
        mode: TimelineApplyMode,
    ) -> Result<TimelineApplyResult, TimelineMapError> {
        let candidate_plan = self.candidate_timeline_plan(manifest, effective_host_id, mode)?;
        self.origin_window_proxy_seqs.clear();
        if candidate_plan.segments.is_empty() {
            return Ok(TimelineApplyResult::default());
        }
        if let Some(discontinuity_sequence) = mode.handoff_discontinuity_sequence() {
            self.pending_handoff_discontinuity_sequence = Some(discontinuity_sequence);
            self.pending_origin_epoch_handoff = true;
        }
        let handoff_discontinuity_sequence = self.pending_handoff_discontinuity_sequence.take();
        // A rebase that shares no media with the retained timeline starts after a
        // gap. Keeping the old head would publish historical media ahead of the
        // discontinuity and make new playback jump forward.
        let rebase_after_gap = mode == TimelineApplyMode::Rebase { retained_head_in_use: false }
            && !candidate_plan.overlaps_publishable_tail;
        let handoff_publishable_head_proxy_seq =
            handoff_discontinuity_sequence.and(self.publishable_origin_head_proxy_seq).filter(|_| !rebase_after_gap);
        let origin_epoch_handoff = self.pending_origin_epoch_handoff;
        self.pending_origin_epoch_handoff = false;
        if self.segments.is_empty() && self.published_resource_history.is_empty() {
            let initial_sequence = manifest
                .discontinuity_sequence
                .unwrap_or(0)
                .saturating_add(handoff_discontinuity_sequence.unwrap_or(0));
            self.discontinuity_sequence = self.discontinuity_sequence.max(initial_sequence);
        }
        self.origin_version = manifest.version;
        self.target_duration = manifest.target_duration;
        self.independent_segments = manifest.independent_segments;
        let epoch_transition_discontinuity =
            self.apply_origin_epoch_transition_for_manifest(manifest, effective_host_id, origin_epoch_handoff)?;
        let mut mark_handoff_discontinuity = handoff_discontinuity_sequence.is_some()
            || epoch_transition_discontinuity
            || candidate_plan.trimmed_published_prefix;
        let mut manifest_head_proxy_seq = None;
        let mut manifest_tail_proxy_seq = None;
        let mut result = TimelineApplyResult::default();
        for parsed in candidate_plan.segments {
            let proxy_seq = self.map_origin_segment(parsed, manifest, effective_host_id, mark_handoff_discontinuity)?;
            manifest_head_proxy_seq.get_or_insert(proxy_seq);
            manifest_tail_proxy_seq = Some(proxy_seq);
            result.segment_proxy_seqs.push(proxy_seq);
            if let Some(map_id) = self.segments.get(&proxy_seq).and_then(|segment| segment.map_ref) {
                if !result.map_ids.contains(&map_id) {
                    result.map_ids.push(map_id);
                }
            }
            mark_handoff_discontinuity = false;
        }

        let is_provisioning_member_handoff = origin_epoch_handoff
            && self.fresh_manifest_commit_required == Some(crate::HlsFreshManifestRequiredReason::ProvisioningHandoff);
        let mut handoff_head = handoff_publishable_head_proxy_seq
            .filter(|_| !is_provisioning_member_handoff || self.group_member_start_proxy_seq.is_none());
        if is_provisioning_member_handoff {
            // Unpublished, unready entries of the previous member that the new member's window does not
            // contain can never become renderable; left in place they block the publishable head.
            let before = self.segments.len();
            let last_published_first_seq = self.last_rendered_first_proxy_seq;
            let mut pruned_hidden_discontinuities = 0_u64;
            self.segments.retain(|proxy_seq, entry| {
                let keep = matches!(entry.status, SegmentCacheStatus::Ready { .. })
                    || entry.last_rendered_at_ms.is_some()
                    || result.segment_proxy_seqs.contains(proxy_seq);
                if !keep
                    && entry.discontinuity_before
                    && last_published_first_seq.is_some_and(|first_seq| *proxy_seq < first_seq)
                {
                    pruned_hidden_discontinuities = pruned_hidden_discontinuities.saturating_add(1);
                }
                keep
            });
            self.discontinuity_sequence = self.discontinuity_sequence.saturating_add(pruned_hidden_discontinuities);
            if self.segments.len() != before {
                self.origin_to_proxy.retain(|_, proxy_seq| self.segments.contains_key(proxy_seq));
                handoff_head = None;
            }
        }

        let retained_gap_floor = self.publishable_origin_head_proxy_seq.filter(|head| {
            self.segments
                .range(..*head)
                .any(|(_, entry)| matches!(entry.status, SegmentCacheStatus::FailedPermanent { .. }))
        });
        self.publishable_origin_head_proxy_seq = handoff_head
            .filter(|head| self.segments.contains_key(head))
            .or(manifest_head_proxy_seq)
            .map(|head| retained_gap_floor.map_or(head, |floor| head.max(floor)));
        self.publishable_origin_tail_proxy_seq = manifest_tail_proxy_seq;
        self.origin_window_proxy_seqs.clone_from(&result.segment_proxy_seqs);
        Ok(result)
    }

    fn candidate_timeline_plan<'a>(
        &self,
        manifest: &'a ParsedOriginManifest,
        _effective_host_id: u64,
        mode: TimelineApplyMode,
    ) -> Result<CandidateTimelinePlan<'a>, TimelineMapError> {
        let explicit_handoff =
            matches!(mode, TimelineApplyMode::ExplicitHandoff { .. } | TimelineApplyMode::Rebase { .. });
        let mut segments = Vec::with_capacity(manifest.segments.len());
        let mut trimmed_published_prefix = false;
        let mut last_existing_proxy = None;
        let mut saw_new = false;
        let mut overlaps_publishable_tail = false;
        let mut first_published_overlap = None;

        for (candidate_position, parsed) in manifest.segments.iter().enumerate() {
            let resource_key =
                HlsMediaResourceIdentity::from_url(&parsed.resource_uri, parsed.origin_byte_range).semantic_key();
            let disposition = self.classify_candidate_segment(parsed)?;
            match disposition {
                CandidateSegmentDisposition::CurrentMapping(proxy_seq)
                | CandidateSegmentDisposition::PublishedOverlap(proxy_seq) => {
                    if saw_new || last_existing_proxy.is_some_and(|previous| proxy_seq <= previous) {
                        return Err(TimelineMapError::PublishedResourceReplay {
                            previous_proxy_tail: self.proxy_next_seq.and_then(|next| next.checked_sub(1)),
                            existing_proxy_seq: proxy_seq,
                            candidate_position,
                            candidate_origin_seq: parsed.origin_seq,
                            resource_key,
                            decision: HlsResourceReplayDecision::RejectContradictoryOrder,
                        });
                    }
                    last_existing_proxy = Some(proxy_seq);
                    overlaps_publishable_tail |= self.publishable_origin_tail_proxy_seq == Some(proxy_seq);
                    if matches!(disposition, CandidateSegmentDisposition::CurrentMapping(_)) {
                        segments.push(parsed);
                    } else {
                        trimmed_published_prefix = true;
                        first_published_overlap.get_or_insert((
                            proxy_seq,
                            candidate_position,
                            parsed.origin_seq,
                            resource_key,
                        ));
                    }
                }
                CandidateSegmentDisposition::New => {
                    saw_new = true;
                    segments.push(parsed);
                }
            }
        }
        if segments.is_empty() && !explicit_handoff && trimmed_published_prefix {
            let (existing_proxy_seq, candidate_position, candidate_origin_seq, resource_key) =
                first_published_overlap.unwrap_or_default();
            return Err(TimelineMapError::PublishedResourceReplay {
                previous_proxy_tail: self.proxy_next_seq.and_then(|next| next.checked_sub(1)),
                existing_proxy_seq,
                candidate_position,
                candidate_origin_seq,
                resource_key,
                decision: HlsResourceReplayDecision::RejectReplayOnly,
            });
        }
        Ok(CandidateTimelinePlan { segments, trimmed_published_prefix, overlaps_publishable_tail })
    }

    fn classify_candidate_segment(
        &self,
        parsed: &ParsedOriginSegment,
    ) -> Result<CandidateSegmentDisposition, TimelineMapError> {
        let Some(scope) = HlsMediaPlaylistScope::from_session(&self.session_key) else {
            return Ok(CandidateSegmentDisposition::New);
        };
        let identity = HlsSegmentOccurrenceIdentity::from_parsed(scope, parsed);
        if let Some((existing, proxy_seq)) = self.current_occurrence_binding(identity) {
            if existing != identity
                && !self.verified_origin_bindings.iter().any(|binding| binding.matches(proxy_seq, existing, identity))
            {
                return Err(TimelineMapError::OriginSequenceResourceConflict {
                    existing_proxy_seq: proxy_seq,
                    candidate_origin_seq: parsed.origin_seq,
                });
            }
            return Ok(CandidateSegmentDisposition::CurrentMapping(proxy_seq));
        }
        if let Some((existing, proxy_seq)) = self.published_resource_history.binding_for(identity) {
            if existing != identity
                && !self.verified_origin_bindings.iter().any(|binding| binding.matches(proxy_seq, existing, identity))
            {
                return Err(TimelineMapError::OriginSequenceResourceConflict {
                    existing_proxy_seq: proxy_seq,
                    candidate_origin_seq: parsed.origin_seq,
                });
            }
            return Ok(CandidateSegmentDisposition::PublishedOverlap(proxy_seq));
        }
        Ok(CandidateSegmentDisposition::New)
    }

    fn current_occurrence_binding(
        &self,
        identity: HlsSegmentOccurrenceIdentity,
    ) -> Option<(HlsSegmentOccurrenceIdentity, u64)> {
        self.segments
            .values()
            .filter(|entry| entry.proxy_seq >= self.published_resource_history.restart_proxy_floor())
            .find_map(|entry| {
                entry
                    .occurrence_identity
                    .filter(|existing| existing.same_position(identity))
                    .map(|existing| (existing, entry.proxy_seq))
            })
    }

    fn has_origin_namespace_baseline(&self) -> bool {
        self.origin_epoch_effective_host_id.is_some()
            || self.origin_epoch_sequence_base.is_some()
            || self.origin_seq_highwater.is_some()
            || !self.origin_to_proxy.is_empty()
            || !self.origin_map_to_proxy.is_empty()
    }

    fn apply_origin_epoch_transition_for_manifest(
        &mut self,
        manifest: &ParsedOriginManifest,
        effective_host_id: u64,
        origin_epoch_handoff: bool,
    ) -> Result<bool, TimelineMapError> {
        let effective_host_changed =
            self.origin_epoch_effective_host_id.is_some_and(|committed_host_id| committed_host_id != effective_host_id);
        if self.has_origin_namespace_baseline() && (origin_epoch_handoff || effective_host_changed) {
            self.start_new_origin_epoch(manifest, OriginEpochTransitionReason::Handoff)?;
            self.origin_epoch_effective_host_id = Some(effective_host_id);
            return Ok(true);
        }

        if self.should_start_new_origin_epoch_for_rollover(manifest) {
            self.start_new_origin_epoch(manifest, OriginEpochTransitionReason::Rollover)?;
            self.origin_epoch_effective_host_id = Some(effective_host_id);
            return Ok(true);
        }

        self.origin_epoch_effective_host_id = Some(effective_host_id);
        Ok(false)
    }

    fn should_start_new_origin_epoch_for_rollover(&self, manifest: &ParsedOriginManifest) -> bool {
        let Some(highwater) = self.origin_seq_highwater else {
            return false;
        };
        manifest.segments.last().is_some_and(|segment| segment.origin_seq < highwater)
    }

    fn start_new_origin_epoch(
        &mut self,
        manifest: &ParsedOriginManifest,
        reason: OriginEpochTransitionReason,
    ) -> Result<(), TimelineMapError> {
        check_rollover_replay(
            &self.session_key,
            manifest,
            &self.segments,
            &self.published_resource_history,
            self.known_verified_bindings.iter().copied(),
        )?;
        let previous_highwater = self.origin_seq_highwater;
        self.origin_epoch = self.origin_epoch.checked_add(1).ok_or(TimelineMapError::ProxySequenceOverflow)?;
        self.origin_epoch_effective_host_id = None;
        self.origin_epoch_sequence_base = None;
        self.origin_seq_highwater = None;
        if let (OriginEpochTransitionReason::Rollover, Some(highwater)) = (reason, previous_highwater) {
            info!(
                "HLS media sequence rollover detected: proxy_session={} previous_highwater={highwater} next_origin_seq={} origin_epoch={}",
                safe_proxy_session_id(&self.proxy_session_id),
                manifest.origin_manifest_sequence,
                self.origin_epoch
            );
        }
        Ok(())
    }

    fn map_origin_segment(
        &mut self,
        parsed: &ParsedOriginSegment,
        manifest: &ParsedOriginManifest,
        effective_host_id: u64,
        mut proxy_discontinuity_before: bool,
    ) -> Result<u64, TimelineMapError> {
        let current_epoch_key = self.origin_segment_key(effective_host_id, parsed.origin_seq);
        let previous_highwater = self.origin_seq_highwater;
        self.origin_seq_highwater =
            Some(previous_highwater.map_or(parsed.origin_seq, |highwater| highwater.max(parsed.origin_seq)));
        if let CandidateSegmentDisposition::CurrentMapping(proxy_seq) = self.classify_candidate_segment(parsed)? {
            let retain_binding = self.segments.get(&proxy_seq).is_some_and(|entry| {
                matches!(entry.status, SegmentCacheStatus::Ready { .. })
                    || (matches!(entry.status, SegmentCacheStatus::FailedPermanent { .. })
                        && entry.origin_fetch_ref.is_some())
            });
            let dependencies = if retain_binding {
                self.refresh_bound_segment_map(proxy_seq, parsed, manifest);
                None
            } else {
                Some((self.segment_map_ref(parsed, manifest)?, map_segment_encryption(parsed)?))
            };
            if let Some(entry) = self.segments.get_mut(&proxy_seq) {
                if !retain_binding {
                    entry.origin_fetch_ref = Some(OriginSegmentFetchRef {
                        resolved_origin_url: parsed.resolved_origin_url.clone(),
                        byte_range: parsed.origin_byte_range,
                        valid_until_ms: None,
                    });
                    entry.retained_resource_identity = None;
                }
                if let Some((map_ref, encryption)) = dependencies {
                    entry.map_ref = map_ref;
                    entry.encryption = encryption;
                }
            }
            self.origin_to_proxy.insert(current_epoch_key, proxy_seq);
            return Ok(proxy_seq);
        }
        // The current epoch's highwater includes all previously mapped media.
        // A forward jump beyond its successor skips media this timeline never had.
        let missing_origin_segments = previous_highwater
            .map(|highwater| parsed.origin_seq.saturating_sub(highwater.saturating_add(1)))
            .filter(|missing| *missing > 0);
        if let Some(missing_origin_segments) = missing_origin_segments {
            info!(
                "HLS forward jump accepted: proxy_session={} origin_sequence={} missing_segments={missing_origin_segments}",
                safe_proxy_session_id(&self.proxy_session_id),
                parsed.origin_seq
            );
            proxy_discontinuity_before = true;
        }
        let map_ref = self.segment_map_ref(parsed, manifest)?;

        let origin_key = current_epoch_key;

        let proxy_seq = self.proxy_next_seq.unwrap_or_default();
        self.proxy_next_seq = Some(proxy_seq.checked_add(1).ok_or(TimelineMapError::ProxySequenceOverflow)?);
        if proxy_discontinuity_before && self.session_key.archive_reference.is_none() {
            self.new_live_recovery_discontinuities.push(proxy_seq);
        }

        let proxy_file_ext = proxy_extension_from_url(&parsed.resolved_origin_url, SEGMENT_EXTENSIONS)
            .ok_or(TimelineMapError::UnsupportedSegmentExtension)?;
        let entry = SegmentEntry {
            origin_key,
            proxy_seq,
            media_start_ms: self.next_media_start_ms,
            duration_ms: parsed.duration_ms,
            content_type: default_content_type_for_segment_ext(&proxy_file_ext).to_string(),
            cache_key: SegmentCacheKey::new(self.proxy_session_id.clone(), proxy_seq, &proxy_file_ext),
            proxy_file_ext,
            discontinuity_before: parsed.discontinuity_before || proxy_discontinuity_before,
            program_date_time: parsed.program_date_time.clone(),
            daterange_tags_before: parsed.daterange_tags_before.clone(),
            origin_byte_range: parsed.origin_byte_range,
            map_ref,
            encryption: map_segment_encryption(parsed)?,
            occurrence_identity: HlsMediaPlaylistScope::from_session(&self.session_key)
                .map(|scope| HlsSegmentOccurrenceIdentity::from_parsed(scope, parsed)),
            retained_resource_identity: None,
            // Preserve the concrete fetch target resolved by the parser; do not derive provider/session identity from it.
            origin_fetch_ref: Some(OriginSegmentFetchRef {
                resolved_origin_url: parsed.resolved_origin_url.clone(),
                byte_range: parsed.origin_byte_range,
                valid_until_ms: None,
            }),
            status: SegmentCacheStatus::Discovered,
            last_rendered_at_ms: None,
            access: Arc::new(CacheAccessState::new()),
        };
        if proxy_discontinuity_before && !parsed.discontinuity_before {
            self.new_proxy_only_discontinuities.push(proxy_seq);
        }

        self.next_media_start_ms = self.next_media_start_ms.saturating_add(parsed.duration_ms);
        self.origin_to_proxy.insert(origin_key, proxy_seq);
        self.segments.insert(proxy_seq, entry);
        Ok(proxy_seq)
    }

    fn segment_map_ref(
        &mut self,
        parsed: &ParsedOriginSegment,
        manifest: &ParsedOriginManifest,
    ) -> Result<Option<ProxyMapId>, TimelineMapError> {
        parsed
            .map_ref
            .map(|map_id| {
                let parsed_map = manifest.maps.get(map_id).ok_or(TimelineMapError::UnsupportedMapExtension)?;
                self.map_origin_map(parsed_map)
            })
            .transpose()
    }

    fn origin_segment_key(&mut self, effective_host_id: u64, host_local_sequence: u64) -> OriginSegmentKey {
        let sequence_base = *self.origin_epoch_sequence_base.get_or_insert(host_local_sequence);
        let relative_index = host_local_sequence.saturating_sub(sequence_base);
        let host_local_index = u32::try_from(relative_index).unwrap_or(u32::MAX);
        OriginSegmentKey { origin_epoch: self.origin_epoch, effective_host_id, host_local_sequence, host_local_index }
    }

    fn map_origin_map(&mut self, parsed_map: &ParsedOriginMap) -> Result<ProxyMapId, TimelineMapError> {
        let binding = self.origin_map_binding(parsed_map);
        if let Some(proxy_map_id) = self.origin_map_to_proxy.get(&binding).copied() {
            self.refresh_map_fetch_ref(proxy_map_id, parsed_map);
            return Ok(proxy_map_id);
        }
        let origin_map_key = OriginMapKey {
            origin_epoch: self.origin_epoch,
            resolved_origin_uri: parsed_map.resolved_origin_uri.clone(),
            byte_range: parsed_map.byte_range,
        };
        let proxy_map_id = ProxyMapId(self.next_proxy_map_id);
        self.next_proxy_map_id = self.next_proxy_map_id.checked_add(1).ok_or(TimelineMapError::ProxyMapIdOverflow)?;
        let proxy_file_ext = proxy_extension_from_url(&origin_map_key.resolved_origin_uri, MAP_EXTENSIONS)
            .ok_or(TimelineMapError::UnsupportedMapExtension)?;
        self.maps
            .insert(proxy_map_id, MapEntry::new(&self.proxy_session_id, proxy_map_id, origin_map_key, proxy_file_ext));
        self.origin_map_to_proxy.insert(binding, proxy_map_id);
        Ok(proxy_map_id)
    }

    fn origin_map_binding(&self, parsed_map: &ParsedOriginMap) -> (u64, HlsMediaResourceIdentity) {
        (self.origin_epoch, HlsMediaResourceIdentity::from_url(&parsed_map.resource_uri, parsed_map.byte_range))
    }

    fn refresh_bound_segment_map(
        &mut self,
        proxy_seq: u64,
        parsed: &ParsedOriginSegment,
        manifest: &ParsedOriginManifest,
    ) {
        let Some(bound_id) = self.segments.get(&proxy_seq).and_then(|entry| entry.map_ref) else { return };
        let Some(parsed_map) = parsed.map_ref.and_then(|id| manifest.maps.get(id)) else { return };
        if self.origin_map_to_proxy.get(&self.origin_map_binding(parsed_map)) == Some(&bound_id)
            && self.maps.get(&bound_id).is_some_and(|entry| !matches!(entry.status, MapCacheStatus::Ready { .. }))
        {
            self.refresh_map_fetch_ref(bound_id, parsed_map);
        }
    }

    fn refresh_map_fetch_ref(&mut self, proxy_map_id: ProxyMapId, parsed_map: &ParsedOriginMap) {
        let Some(entry) = self.maps.get_mut(&proxy_map_id) else { return };
        if matches!(entry.status, MapCacheStatus::FailedRetryable { .. } | MapCacheStatus::FailedPermanent { .. })
            && entry
                .origin_fetch_ref
                .as_ref()
                .is_none_or(|previous| previous.resolved_origin_url != parsed_map.resolved_origin_uri)
        {
            entry.status = MapCacheStatus::Discovered;
        }
        entry.origin_fetch_ref = Some(OriginMapFetchRef {
            resolved_origin_url: parsed_map.resolved_origin_uri.clone(),
            byte_range: parsed_map.byte_range,
            valid_until_ms: None,
        });
    }
}

pub(crate) fn map_segment_encryption(
    parsed: &ParsedOriginSegment,
) -> Result<Option<HlsSegmentEncryption>, TimelineMapError> {
    parsed
        .encryption
        .as_ref()
        .map(|encryption| {
            let resource_id = encryption
                .proxy_resource_id
                .as_ref()
                .filter(|value| !value.is_empty())
                .cloned()
                .ok_or(TimelineMapError::MissingKeyResource)?;
            let resource_extension = encryption
                .proxy_resource_extension
                .as_ref()
                .filter(|value| !value.is_empty())
                .cloned()
                .ok_or(TimelineMapError::MissingKeyResource)?;
            Ok(HlsSegmentEncryption {
                resource_id: TransientResourceId(resource_id),
                resource_extension,
                // The proxy rewrites MEDIA-SEQUENCE into its own monotone timeline. An
                // omitted origin IV would therefore be re-derived from the wrong sequence
                // by clients unless the origin's host-local media sequence is materialized.
                iv: Some(encryption.iv.clone().unwrap_or_else(|| format!("0x{:032x}", parsed.origin_seq))),
                key_format: encryption.key_format.clone(),
                key_format_versions: encryption.key_format_versions.clone(),
            })
        })
        .transpose()
}

fn proxy_extension_from_url(url: &str, allowed_extensions: &[&str]) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    let file_name = parsed.path_segments()?.next_back()?;
    let extension = file_name.rsplit_once('.')?.1.to_ascii_lowercase();
    allowed_extensions.contains(&extension.as_str()).then_some(extension)
}

#[cfg(any(test, feature = "test-support"))]
fn inferred_effective_origin_host_id(manifest: &ParsedOriginManifest) -> u64 {
    manifest
        .segments
        .first()
        .and_then(|segment| Url::parse(&segment.resolved_origin_url).ok())
        .and_then(|url| url.host_str().map(effective_origin_host_id))
        .or_else(|| {
            manifest
                .maps
                .first()
                .and_then(|map| Url::parse(&map.resolved_origin_uri).ok())
                .and_then(|url| url.host_str().map(effective_origin_host_id))
        })
        .unwrap_or(UNKNOWN_EFFECTIVE_ORIGIN_HOST_ID)
}

#[cfg(test)]
mod tests {
    use super::{
        effective_origin_host_id, HlsOriginHandoffPreviewError, OriginSegmentKey, SegmentCacheStatus, SegmentEntry,
        TimelineMapError,
    };
    use crate::{HlsSession, HlsSessionKey, MapCacheStatus, ProxyMapId, SegmentFetchPriority};
    use axum::http::StatusCode;
    use tuliprox_parser::hls::origin_manifest::{parse_origin_media_manifest, OriginManifestParseOutcome};

    const BASE_URL: &str = "http://origin.example.com/live/final/index.m3u8";

    fn session() -> HlsSession { HlsSession::new(HlsSessionKey::new(1, "12345"), b"secret", 0) }

    fn origin_key(epoch: u64, host: &str, sequence: u64, index: u32) -> OriginSegmentKey {
        OriginSegmentKey {
            origin_epoch: epoch,
            effective_host_id: effective_origin_host_id(host),
            host_local_sequence: sequence,
            host_local_index: index,
        }
    }

    fn normal_manifest(body: &str) -> tuliprox_parser::hls::origin_manifest::ParsedOriginManifest {
        normal_manifest_at(body, BASE_URL)
    }

    fn normal_manifest_at(
        body: &str,
        final_manifest_url: &str,
    ) -> tuliprox_parser::hls::origin_manifest::ParsedOriginManifest {
        match parse_origin_media_manifest(body, final_manifest_url) {
            OriginManifestParseOutcome::Normal(manifest) => manifest,
            OriginManifestParseOutcome::TransientPassthrough { reason } => {
                panic!("expected normal manifest: {reason:?}")
            }
        }
    }

    fn publish_ready_manifest(session: &mut HlsSession, rendered_at_ms: u64) -> String {
        for segment in session.segments.values_mut() {
            segment.status = SegmentCacheStatus::Ready { content_length: 1, ready_at_ms: rendered_at_ms };
        }
        session.render_and_store_manifest(rendered_at_ms).expect("ready manifest should render").body
    }

    fn six_segment_manifest(first_sequence: u64) -> tuliprox_parser::hls::origin_manifest::ParsedOriginManifest {
        let segments: String = (first_sequence..first_sequence + 6).fold(String::new(), |mut body, sequence| {
            body.push_str("#EXTINF:4,\n");
            body.push_str(&sequence.to_string());
            body.push_str(".ts\n");
            body
        });
        normal_manifest(&format!(
            "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:{first_sequence}\n{segments}"
        ))
    }

    fn rebase_overlap_manifest(
        first_sequence: u64,
        resources: &[&str],
    ) -> tuliprox_parser::hls::origin_manifest::ParsedOriginManifest {
        use std::fmt::Write as _;
        let mut body = format!("#EXTM3U\n#EXT-X-TARGETDURATION:7\n#EXT-X-MEDIA-SEQUENCE:{first_sequence}\n");
        for resource in resources {
            writeln!(body, "#EXTINF:7,\n{resource}").unwrap();
        }
        normal_manifest(&body)
    }

    fn rebase_overlap_session() -> HlsSession {
        let mut state = session();
        let baseline = rebase_overlap_manifest(182_137, &["182137.ts", "182138.ts", "182139.ts", "182140.ts"]);
        state.apply_origin_manifest(&baseline).expect("baseline maps");
        publish_ready_manifest(&mut state, 1);
        let rebase = rebase_overlap_manifest(182_139, &["182139.ts", "182140.ts", "182141.ts", "182142.ts"]);
        state.mark_pending_origin_epoch_handoff_discontinuity(0);
        state
            .apply_origin_rebase_manifest(&rebase, effective_origin_host_id("origin.example.com"), false)
            .expect("rebase trims the two retained resources");
        assert_eq!(state.live_origin_sequences(), &[2, 3, 4, 5]);
        state
    }

    #[test]
    fn rebase_continuation_reuses_cached_overlap_without_starting_an_epoch() {
        for retained_head_in_use in [false, true] {
            let mut state = session();
            let baseline = rebase_overlap_manifest(37, &["alpha.ts", "beta.ts", "gamma.ts", "delta.ts"]);
            state.apply_origin_manifest(&baseline).unwrap();
            publish_ready_manifest(&mut state, 1);
            let epoch = state.origin_epoch;
            let cached = [state.segments[&2].clone(), state.segments[&3].clone()];
            let next = rebase_overlap_manifest(39, &["gamma.ts", "delta.ts", "epsilon.ts", "zeta.ts"]);

            state
                .apply_origin_rebase_manifest(
                    &next,
                    effective_origin_host_id("origin.example.com"),
                    retained_head_in_use,
                )
                .expect("a verified sliding window continues the current timeline");

            assert_eq!(state.live_origin_sequences(), &[2, 3, 4, 5]);
            assert_eq!(state.origin_epoch, epoch);
            assert_eq!(state.origin_seq_highwater, Some(42));
            assert_eq!(state.proxy_next_seq, Some(6));
            for (seq, previous) in [2, 3].into_iter().zip(&cached) {
                let current = &state.segments[&seq];
                assert_eq!(current.cache_key, previous.cache_key);
                assert_eq!(current.origin_key, previous.origin_key);
                assert_eq!(current.status, previous.status);
                assert!(std::sync::Arc::ptr_eq(&current.access, &previous.access));
                assert_eq!(current.origin_fetch_ref, previous.origin_fetch_ref);
            }
            assert!(!state.segments[&4].discontinuity_before);

            state
                .apply_origin_rebase_manifest(
                    &next,
                    effective_origin_host_id("origin.example.com"),
                    retained_head_in_use,
                )
                .expect("repeated revalidation reuses the same complete window");
            assert_eq!(state.live_origin_sequences(), &[2, 3, 4, 5]);
            assert_eq!(state.origin_epoch, epoch);
            assert_eq!(state.proxy_next_seq, Some(6));
        }
    }

    #[test]
    fn rebase_continuation_does_not_reuse_a_different_or_unproven_origin_baseline() {
        for case in ["host", "handoff", "restart", "duration", "gap", "collected"] {
            let mut state = session();
            state.apply_origin_manifest(&rebase_overlap_manifest(37, &["37.ts", "38.ts", "39.ts", "40.ts"])).unwrap();
            publish_ready_manifest(&mut state, 1);
            let epoch = state.origin_epoch;
            let mut host = effective_origin_host_id("origin.example.com");
            let mut next = rebase_overlap_manifest(39, &["39.ts", "40.ts", "41.ts", "42.ts"]);
            match case {
                "host" => host = effective_origin_host_id("other.example.com"),
                "handoff" => state.mark_pending_origin_epoch_handoff_discontinuity(0),
                "restart" => next = rebase_overlap_manifest(1, &["39.ts", "40.ts", "41.ts", "42.ts"]),
                "duration" => next.segments[0].duration_ms = 6_000,
                "gap" => next = rebase_overlap_manifest(60, &["60.ts", "61.ts", "62.ts", "63.ts"]),
                "collected" => {
                    state.segments.retain(|seq, _| *seq < 2);
                    state.origin_to_proxy.retain(|_, seq| *seq < 2);
                }
                _ => unreachable!(),
            }

            state.apply_origin_rebase_manifest(&next, host, false).unwrap();

            assert_eq!(state.origin_epoch, epoch + 1, "{case}: retain the real rebase path");
            assert_eq!(
                state.live_origin_sequences(),
                match case {
                    "gap" | "restart" => &[4, 5, 6, 7][..],
                    "collected" => &[4, 5][..],
                    _ => &[2, 3, 4, 5][..],
                }
            );
            assert_eq!(
                state.segments[&4].discontinuity_before,
                matches!(case, "gap" | "restart" | "collected"),
                "{case}"
            );
        }
    }

    #[test]
    fn rebase_continuation_rejects_changed_bindings_but_preserves_new_msn_repetition() {
        for case in ["uri", "range", "new-msn"] {
            let mut state = session();
            state.apply_origin_manifest(&rebase_overlap_manifest(37, &["37.ts", "38.ts", "39.ts", "40.ts"])).unwrap();
            publish_ready_manifest(&mut state, 1);
            let before = super::TimelineDraft::from(&state);
            let mut next = rebase_overlap_manifest(39, &["39.ts", "40.ts", "41.ts", "42.ts"]);
            match case {
                "uri" => {
                    next.segments[0].resource_uri = "/changed.ts".into();
                    next.segments[0].resolved_origin_url = "http://origin.example.com/changed.ts".into();
                }
                "range" => next.segments[0].origin_byte_range = Some(super::ParsedByteRange { offset: 0, length: 512 }),
                "new-msn" => {
                    next.segments[3].resource_uri = "/live/final/38.ts".into();
                    next.segments[3].resolved_origin_url = "http://origin.example.com/live/final/38.ts".into();
                }
                _ => unreachable!(),
            }
            let result =
                state.apply_origin_rebase_manifest(&next, effective_origin_host_id("origin.example.com"), false);
            if case == "new-msn" {
                result.unwrap();
                assert_eq!(state.live_origin_sequences(), &[2, 3, 4, 5]);
            } else {
                assert_eq!(
                    result,
                    Err(TimelineMapError::OriginSequenceResourceConflict {
                        existing_proxy_seq: 2,
                        candidate_origin_seq: 39
                    })
                );
                assert_eq!(super::TimelineDraft::from(&state), before);
            }
        }
    }

    #[test]
    fn rebase_overlap_retains_current_mappings_after_the_trimmed_prefix() {
        for old_media_collected in [false, true] {
            let mut state = rebase_overlap_session();
            if old_media_collected {
                state.segments.retain(|seq, _| *seq >= 4);
                state.origin_to_proxy.retain(|_, seq| *seq >= 4);
            }
            let epoch = state.origin_epoch;
            let keys = [state.segments[&4].cache_key.clone(), state.segments[&5].cache_key.clone()];
            let repeated = rebase_overlap_manifest(182_139, &["182139.ts", "182140.ts", "182141.ts", "182142.ts"]);
            state.apply_origin_manifest(&repeated).expect("polling the rebased window keeps its current overlap");
            assert_eq!(
                state.live_origin_sequences(),
                if old_media_collected { &[4, 5][..] } else { &[2, 3, 4, 5][..] }
            );
            assert_eq!(state.proxy_next_seq, Some(6));
            let next = rebase_overlap_manifest(182_140, &["182140.ts", "182141.ts", "182142.ts", "182143.ts"]);

            state.apply_origin_manifest(&next).expect("old prefix followed by current overlap is valid");

            assert_eq!(
                state.live_origin_sequences(),
                if old_media_collected { &[4, 5, 6][..] } else { &[3, 4, 5, 6][..] }
            );
            assert_eq!(state.origin_epoch, epoch, "sliding overlap must not start another epoch");
            assert_eq!(state.origin_seq_highwater, Some(182_143));
            assert_eq!(state.proxy_next_seq, Some(7), "only the new resource gets a proxy sequence");
            assert_eq!([state.segments[&4].cache_key.clone(), state.segments[&5].cache_key.clone()], keys);
            assert_eq!(state.segments[&6].origin_key.host_local_sequence, 182_143);

            state.apply_origin_manifest(&next).expect("repeated polling keeps the same mappings");
            assert_eq!(
                state.live_origin_sequences(),
                if old_media_collected { &[4, 5, 6][..] } else { &[3, 4, 5, 6][..] }
            );
            assert_eq!(state.proxy_next_seq, Some(7));
        }
    }

    #[test]
    fn rebase_overlap_rejects_same_msn_binding_changes_atomically() {
        for (first, resources, proxy, msn) in [
            (182_139, ["182139.ts", "new-140.ts", "182141.ts", "182142.ts"], 3, 182_140),
            (182_140, ["182140.ts", "182141.ts", "182139.ts", "182143.ts"], 5, 182_142),
            (182_140, ["182140.ts", "changed-141.ts", "182142.ts", "182143.ts"], 4, 182_141),
        ] {
            let mut state = rebase_overlap_session();
            let before = super::TimelineDraft::from(&state);
            assert_eq!(
                state.apply_origin_manifest(&rebase_overlap_manifest(first, &resources)),
                Err(TimelineMapError::OriginSequenceResourceConflict {
                    existing_proxy_seq: proxy,
                    candidate_origin_seq: msn
                })
            );
            assert_eq!(super::TimelineDraft::from(&state), before);
        }
    }

    #[test]
    fn rebase_after_origin_gap_publishes_from_the_new_window_head() {
        let mut session = session();
        session.apply_origin_manifest(&six_segment_manifest(0)).expect("baseline maps");
        publish_ready_manifest(&mut session, 1);

        let host_id = effective_origin_host_id("origin.example.com");
        session.apply_origin_rebase_manifest(&six_segment_manifest(60), host_id, false).expect("rebase maps");

        assert_eq!(session.publishable_origin_head_proxy_seq, Some(6));
        assert_eq!(session.publishable_origin_tail_proxy_seq, Some(11));
        assert!(session.segments.get(&6).expect("first rebased segment").discontinuity_before);
    }

    #[test]
    fn rebase_overlapping_only_before_the_published_tail_publishes_from_the_new_window_head() {
        let mut session = session();
        session.apply_origin_manifest(&six_segment_manifest(0)).expect("baseline maps");
        publish_ready_manifest(&mut session, 1);

        let host_id = effective_origin_host_id("origin.example.com");
        let rebase = normal_manifest(
            "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:59\n\
             #EXTINF:4,\n3.ts\n#EXTINF:4,\n60.ts\n#EXTINF:4,\n61.ts\n#EXTINF:4,\n62.ts\n",
        );
        session.apply_origin_rebase_manifest(&rebase, host_id, false).expect("rebase maps");

        assert_eq!(session.publishable_origin_head_proxy_seq, Some(6));
        assert_eq!(session.publishable_origin_tail_proxy_seq, Some(9));
    }

    #[test]
    fn rebase_after_origin_gap_keeps_the_head_while_viewers_still_hold_it() {
        let mut session = session();
        session.apply_origin_manifest(&six_segment_manifest(0)).expect("baseline maps");
        publish_ready_manifest(&mut session, 1);

        let host_id = effective_origin_host_id("origin.example.com");
        session.apply_origin_rebase_manifest(&six_segment_manifest(60), host_id, true).expect("rebase maps");

        assert_eq!(session.publishable_origin_head_proxy_seq, Some(0));
        assert!(session.segments.get(&6).expect("first rebased segment").discontinuity_before);
    }

    #[test]
    fn rebase_continuation_overlapping_the_retained_timeline_reuses_its_current_window() {
        let mut session = session();
        session.apply_origin_manifest(&six_segment_manifest(0)).expect("baseline maps");
        publish_ready_manifest(&mut session, 1);

        let host_id = effective_origin_host_id("origin.example.com");
        session.apply_origin_rebase_manifest(&six_segment_manifest(3), host_id, false).expect("rebase maps");

        assert_eq!(session.publishable_origin_head_proxy_seq, Some(3));
        assert_eq!(session.publishable_origin_tail_proxy_seq, Some(8));
        assert_eq!(session.live_origin_sequences(), &[3, 4, 5, 6, 7, 8]);
        assert_eq!(session.origin_epoch, 0);
    }

    #[test]
    fn historical_uri_under_conflicting_msn_is_not_trimmed_as_overlap() {
        let mut state = session();
        state.apply_origin_manifest(&six_segment_manifest(471)).unwrap();
        publish_ready_manifest(&mut state, 1);
        let before = super::TimelineDraft::from(&state);
        let candidate = rebase_overlap_manifest(476, &["474.ts", "480.ts", "481.ts"]);
        assert_eq!(
            state.apply_origin_manifest(&candidate),
            Err(TimelineMapError::OriginSequenceResourceConflict { existing_proxy_seq: 5, candidate_origin_seq: 476 })
        );
        assert_eq!(super::TimelineDraft::from(&state), before);
    }

    #[test]
    fn explicit_handoff_reuses_all_unchanged_overlap_in_preview_and_commit() {
        let mut state = session();
        state
            .apply_origin_manifest_for_host(&six_segment_manifest(476), effective_origin_host_id("a.example"))
            .unwrap();
        publish_ready_manifest(&mut state, 1);
        let old = state.segments[&3].clone();
        let next = six_segment_manifest(479);
        let host = effective_origin_host_id("b.example");
        let preview = state.preview_origin_handoff_manifest(&next, host, 0).unwrap();
        assert_eq!(preview.segments.iter().map(|entry| entry.proxy_seq).collect::<Vec<_>>(), [3, 4, 5, 6, 7, 8]);
        assert_eq!(preview.segments[0], old);
        state.apply_origin_handoff_manifest_if_preview_matches(&next, host, 0, &preview).unwrap();
        assert_eq!(state.proxy_next_seq, Some(9));
        assert_eq!(state.segments[&3], old);
        publish_ready_manifest(&mut state, 2);
    }

    #[test]
    fn published_resource_history_survives_segment_gc() {
        let mut session = session();
        let first = normal_manifest(
            "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:480\n\
             #EXTINF:4,\n480.ts\n#EXTINF:4,\n481.ts\n#EXTINF:4,\n482.ts\n",
        );
        let replay_then_new = normal_manifest(
            "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:480\n\
             #EXTINF:4,\n480.ts\n#EXTINF:4,\n481.ts\n#EXTINF:4,\n482.ts\n#EXTINF:4,\n480.ts\n",
        );

        session.apply_origin_manifest(&first).expect("baseline maps");
        publish_ready_manifest(&mut session, 1);
        session.segments.clear();
        session.origin_to_proxy.clear();
        session.apply_origin_manifest(&replay_then_new).expect("GC-independent history trims replay");

        assert_eq!(session.proxy_next_seq, Some(4));
        assert_eq!(session.publishable_origin_head_proxy_seq, Some(3));
        assert!(session.segments.get(&3).expect("new head").discontinuity_before);
        assert_eq!(session.published_resource_history.len(), 3);
    }

    #[test]
    fn same_uri_under_new_msn_after_new_media_is_not_a_replay() {
        let mut state = session();
        state.apply_origin_manifest(&rebase_overlap_manifest(480, &["480.ts", "481.ts", "482.ts"])).unwrap();
        publish_ready_manifest(&mut state, 1);
        state.apply_origin_manifest(&rebase_overlap_manifest(490, &["490.ts", "480.ts", "491.ts"])).unwrap();
        assert_eq!(state.proxy_next_seq, Some(6));
        assert_eq!(state.segments[&4].origin_key.host_local_sequence, 491);
        assert_ne!(state.segments[&0].occurrence_identity, state.segments[&4].occurrence_identity);
    }

    #[test]
    fn parsed_target_duration_is_stored_for_account_overlap_timing() {
        let mut session = session();
        let manifest =
            normal_manifest("#EXTM3U\n#EXT-X-TARGETDURATION:12\n#EXT-X-MEDIA-SEQUENCE:10\n#EXTINF:12.0,\n10.ts\n");

        session.apply_origin_manifest(&manifest).expect("manifest should map");

        assert_eq!(session.target_duration, Some(12));
        assert_eq!(session.account_overlap_timing().target_duration_ms, 12_000);
        assert_eq!(session.account_overlap_timing().hard_active_window_ms, 12_000);
        assert_eq!(session.account_overlap_timing().soft_active_window_ms, 24_000);
    }

    #[test]
    fn capacity_recovery_blocks_only_at_first_dependency_ready_boundary() {
        let mut session = session();
        let manifest = normal_manifest(
            "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MAP:URI=\"init.mp4\"\n\
             #EXTINF:4.0,\n0.m4s\n#EXTINF:4.0,\n1.m4s\n#EXTINF:4.0,\n2.m4s\n",
        );
        session.apply_origin_manifest(&manifest).expect("capacity timeline maps");
        for segment in session.segments.values_mut() {
            segment.status = SegmentCacheStatus::Ready { content_length: 1, ready_at_ms: 1 };
        }
        session.segments.get_mut(&1).expect("deferred segment").status =
            SegmentCacheStatus::CapacityDeferred { priority: SegmentFetchPriority::Prefetch, deferred_at_ms: 2 };

        let missing_map = session.ready_timeline_snapshot(0, 3);
        assert!(!session.capacity_recovery_blocks_ready_timeline(&missing_map));

        session.maps.get_mut(&ProxyMapId(0)).expect("shared map").status =
            MapCacheStatus::Ready { content_length: 1, ready_at_ms: 3 };
        let deferred_boundary = session.ready_timeline_snapshot(0, 3);
        assert!(session.capacity_recovery_blocks_ready_timeline(&deferred_boundary));

        session.segments.get_mut(&0).expect("unrecoverable head").status =
            SegmentCacheStatus::FailedPermanent { failed_at_ms: 4, status: None };
        let unrecoverable_boundary = session.ready_timeline_snapshot(0, 4);
        assert!(!session.capacity_recovery_blocks_ready_timeline(&unrecoverable_boundary));
    }

    #[test]
    fn ready_timeline_snapshot_stops_before_the_first_sequence_gap() {
        let mut session = session();
        let manifest = normal_manifest(
            "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:20\n\
             #EXTINF:4.0,\n20.ts\n#EXTINF:4.0,\n21.ts\n#EXTINF:4.0,\n22.ts\n",
        );
        session.apply_origin_manifest(&manifest).expect("timeline maps");
        for segment in session.segments.values_mut() {
            segment.status = SegmentCacheStatus::Ready { content_length: 1, ready_at_ms: 1 };
        }
        session.segments.remove(&1);

        let snapshot = session.ready_timeline_snapshot(0, 2);

        assert_eq!(snapshot.units.len(), 1);
        assert_eq!(snapshot.units[0].proxy_seq, 0);
        assert_eq!(snapshot.units[0].start_ms, 0);
        assert!(session.ready_timeline_snapshot(1, 2).units.is_empty());
    }

    #[test]
    fn implicit_aes_iv_is_materialized_from_host_local_sequence_before_proxy_rewrite() {
        let mut manifest = normal_manifest(
            "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:77\n\
             #EXT-X-KEY:METHOD=AES-128,URI=\"origin-a.key\"\n#EXTINF:4,\n77.ts\n\
             #EXT-X-KEY:METHOD=AES-128,URI=\"origin-b.key\",IV=0x00000000000000000000000000000001\n#EXTINF:4,\n78.ts\n\
             #EXT-X-KEY:METHOD=NONE\n#EXTINF:4,\n79.ts\n",
        );
        for encryption in manifest.segments.iter_mut().filter_map(|segment| segment.encryption.as_mut()) {
            encryption.proxy_resource_id = Some("opaque-key".to_string());
            encryption.proxy_resource_extension = Some("key".to_string());
        }
        let mut session = session();

        session.apply_origin_manifest(&manifest).expect("encrypted timeline maps");

        let implicit = session.segments.get(&0).expect("implicit-IV segment");
        assert_eq!(implicit.origin_key.host_local_sequence, 77);
        assert_eq!(
            implicit.encryption.as_ref().and_then(|state| state.iv.as_deref()),
            Some("0x0000000000000000000000000000004d")
        );
        let explicit = session.segments.get(&1).expect("explicit-IV segment");
        assert_eq!(explicit.origin_key.host_local_sequence, 78);
        assert_eq!(
            explicit.encryption.as_ref().and_then(|state| state.iv.as_deref()),
            Some("0x00000000000000000000000000000001")
        );
        assert!(session.segments.get(&2).expect("METHOD=NONE segment").encryption.is_none());
    }

    #[test]
    fn origin_rollover_maps_to_monotone_proxy_sequence() {
        let mut session = session();
        let first = normal_manifest(
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:322\n#EXTINF:4.0,\n322.ts\n#EXTINF:4.0,\n323.ts\n#EXTINF:4.0,\n324.ts\n",
        );
        let second = normal_manifest(
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:4.0,\n0.ts\n#EXTINF:4.0,\n1.ts\n#EXTINF:4.0,\n2.ts\n",
        );

        session.apply_origin_manifest(&first).expect("first manifest should map");
        session.apply_origin_manifest(&second).expect("second manifest should map");

        assert_eq!(session.segments.keys().copied().collect::<Vec<_>>(), vec![0, 1, 2, 3, 4, 5]);
        assert_eq!(session.origin_to_proxy.get(&origin_key(1, "origin.example.com", 0, 0)), Some(&3));
        assert!(session.segments.get(&3).expect("rollover segment").discontinuity_before);
    }

    #[test]
    fn rollover_with_only_previously_seen_resources_does_not_advance_timeline() {
        let mut session = session();
        let old_low = normal_manifest(
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:4.0,\n0.ts\n#EXTINF:4.0,\n1.ts\n#EXTINF:4.0,\n2.ts\n",
        );
        let high = normal_manifest(
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:190\n#EXTINF:4.0,\n190.ts\n#EXTINF:4.0,\n191.ts\n#EXTINF:4.0,\n192.ts\n",
        );
        let rollover = normal_manifest(
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:4.0,\n0.ts\n#EXTINF:4.0,\n1.ts\n#EXTINF:4.0,\n2.ts\n",
        );

        session.apply_origin_manifest(&old_low).expect("old low manifest should map");
        session.apply_origin_manifest(&high).expect("high manifest should map");
        assert!(matches!(
            session.apply_origin_manifest(&rollover),
            Err(TimelineMapError::OriginRolloverResourceReplay { .. })
        ));

        assert_eq!(session.origin_epoch, 0);
        assert_eq!(session.origin_seq_highwater, Some(192));
        assert_eq!(session.origin_to_proxy.get(&origin_key(0, "origin.example.com", 0, 0)), Some(&0));
        assert_eq!(session.origin_to_proxy.get(&origin_key(0, "origin.example.com", 190, 190)), Some(&3));
        assert_eq!(session.segments.keys().copied().collect::<Vec<_>>(), vec![0, 1, 2, 3, 4, 5]);
        assert_eq!(session.proxy_next_seq, Some(6));
    }

    #[test]
    fn same_host_rollover_after_timeline_gc_starts_exactly_one_new_epoch() {
        let mut session = session();
        let first =
            normal_manifest("#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:1000\n#EXTINF:4.0,\n1000.ts\n#EXTINF:4.0,\n1001.ts\n");
        let rollover = normal_manifest("#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:4.0,\n0.ts\n#EXTINF:4.0,\n1.ts\n");
        let sliding = normal_manifest("#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:1\n#EXTINF:4.0,\n1.ts\n#EXTINF:4.0,\n2.ts\n");

        session.apply_origin_manifest(&first).expect("first manifest should map");
        let previous_epoch = session.origin_epoch;
        session.segments.clear();
        session.origin_to_proxy.clear();

        assert_eq!(session.origin_seq_highwater, Some(1_001));
        assert_eq!(session.origin_epoch_sequence_base, Some(1_000));
        session.apply_origin_manifest(&rollover).expect("rollover after GC should map in a new epoch");

        let rollover_epoch = previous_epoch.saturating_add(1);
        assert_eq!(session.origin_epoch, rollover_epoch);
        assert_eq!(session.origin_seq_highwater, Some(1));
        assert_eq!(session.origin_epoch_sequence_base, Some(0));
        assert_eq!(session.origin_to_proxy.get(&origin_key(rollover_epoch, "origin.example.com", 0, 0)), Some(&2));
        assert!(session.segments.get(&2).expect("rollover head").discontinuity_before);

        session.apply_origin_manifest(&sliding).expect("same-host sliding refresh should remain in the new epoch");

        assert_eq!(session.origin_epoch, rollover_epoch);
        assert_eq!(session.origin_seq_highwater, Some(2));
        assert_eq!(session.origin_to_proxy.get(&origin_key(rollover_epoch, "origin.example.com", 2, 2)), Some(&4));
        assert!(!session.segments.get(&4).expect("sliding tail").discontinuity_before);
    }

    #[test]
    fn known_origin_key_is_not_remapped() {
        let mut session = session();
        let manifest = normal_manifest("#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:10\n#EXTINF:4.0,\n10.ts\n");

        session.apply_origin_manifest(&manifest).expect("manifest should map");
        session.apply_origin_manifest(&manifest).expect("same manifest should be ignored");

        assert_eq!(session.segments.len(), 1);
        assert_eq!(session.proxy_next_seq, Some(1));
    }

    #[test]
    fn overlapping_old_origin_sequences_do_not_trigger_rollover() {
        let mut session = session();
        let first = normal_manifest(
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:100\n#EXTINF:4.0,\n100.ts\n#EXTINF:4.0,\n101.ts\n#EXTINF:4.0,\n102.ts\n",
        );
        let overlap = normal_manifest(
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:101\n#EXTINF:4.0,\n101.ts\n#EXTINF:4.0,\n102.ts\n#EXTINF:4.0,\n103.ts\n",
        );

        session.apply_origin_manifest(&first).expect("first manifest should map");
        session.apply_origin_manifest(&overlap).expect("overlapping manifest should map");

        assert_eq!(session.origin_epoch, 0);
        assert_eq!(session.origin_seq_highwater, Some(103));
        assert_eq!(session.segments.keys().copied().collect::<Vec<_>>(), vec![0, 1, 2, 3]);
        assert_eq!(session.publishable_origin_head_proxy_seq, Some(1));
        assert_eq!(session.publishable_origin_tail_proxy_seq, Some(3));
    }

    #[test]
    fn forward_jump_preserves_compact_proxy_sequence() {
        for origin_marked in [false, true] {
            let mut session = session();
            let first = normal_manifest("#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:100\n#EXTINF:4.0,\n100.ts\n");
            let tag = if origin_marked { "#EXT-X-DISCONTINUITY\n" } else { "" };
            let jump = normal_manifest(&format!(
                "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:107\n{tag}#EXTINF:4.0,\n107.ts\n#EXTINF:4.0,\n108.ts\n#EXTINF:4.0,\n109.ts\n",
            ));

            session.apply_origin_manifest(&first).unwrap();
            let previous = session.segments[&0].clone();
            session.apply_origin_manifest(&jump).unwrap();
            session.apply_origin_manifest(&jump).unwrap();

            assert_eq!(session.segments[&0], previous);
            assert_eq!(session.origin_epoch, 0);
            assert_eq!(session.origin_to_proxy.get(&origin_key(0, "origin.example.com", 107, 7)), Some(&1));
            assert_eq!(session.proxy_next_seq, Some(4));
            assert_eq!(session.publishable_origin_head_proxy_seq, Some(1));
            assert_eq!(session.publishable_origin_tail_proxy_seq, Some(3));
            let boundaries = session
                .segments
                .values()
                .filter(|entry| entry.discontinuity_before)
                .map(|entry| entry.proxy_seq)
                .collect::<Vec<_>>();
            assert_eq!(boundaries, [1]);
            assert_eq!(session.origin_discontinuity_before(&session.segments[&1]), origin_marked);
        }
    }

    #[test]
    fn same_origin_slide_marks_skipped_permanent_failure_on_existing_unpublished_successor() {
        let mut state = session();
        state.apply_origin_manifest(&six_segment_manifest(100)).unwrap();
        publish_ready_manifest(&mut state, 1);
        state.apply_origin_manifest(&six_segment_manifest(102)).unwrap();
        let failed = SegmentCacheStatus::FailedPermanent { status: Some(StatusCode::NOT_FOUND), failed_at_ms: 2 };
        state.segments.get_mut(&6).unwrap().status = failed.clone();
        let before = state.segments[&5].clone();
        let successor = state.segments[&7].clone();
        let slide = six_segment_manifest(107);
        state.apply_origin_manifest(&slide).unwrap();
        state.apply_origin_manifest(&slide).unwrap();

        assert_eq!(state.origin_epoch, 0);
        assert_eq!(state.segments[&5], before);
        assert_eq!(state.segments[&6].status, failed);
        assert_eq!(state.segments[&7], SegmentEntry { discontinuity_before: true, ..successor });
        assert!(!state.origin_discontinuity_before(&state.segments[&7]));
        assert!(!state.segments[&8].discontinuity_before);
        let rendered = publish_ready_manifest(&mut state, 3);
        assert!(rendered.contains("#EXT-X-MEDIA-SEQUENCE:7\n#EXT-X-DISCONTINUITY-SEQUENCE:0\n"));
        assert_eq!(rendered.lines().filter(|line| *line == "#EXT-X-DISCONTINUITY").count(), 1);
    }

    #[test]
    fn host_handoff_with_only_published_overlap_does_not_advance() {
        let mut session = session();
        let first = normal_manifest("#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:100\n#EXTINF:4.0,\n100.ts\n");
        let second = normal_manifest("#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:100\n#EXTINF:4.0,\n100.ts\n");

        session.apply_origin_manifest(&first).expect("first manifest should map");
        session.mark_pending_origin_epoch_handoff_discontinuity(0);
        session.apply_origin_manifest(&second).expect("second manifest should map");

        assert_eq!(session.origin_to_proxy.get(&origin_key(0, "origin.example.com", 100, 0)), Some(&0));
        assert_eq!(session.origin_epoch, 1);
        assert_eq!(session.segments.len(), 1);
        assert_eq!(session.proxy_next_seq, Some(1));
    }

    #[test]
    fn handoff_rejects_published_overlap_after_a_new_resource() {
        let mut session = session();
        let first = normal_manifest("#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:100\n#EXTINF:4.0,\n100.ts\n#EXTINF:4.0,\n101.ts\n");
        let second = normal_manifest(
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:99\n#EXTINF:4.0,\n99.ts\n#EXTINF:4.0,\n100.ts\n#EXTINF:4.0,\n101.ts\n",
        );

        session.apply_origin_manifest(&first).expect("first manifest should map");
        session.mark_pending_origin_epoch_handoff_discontinuity(0);
        assert!(matches!(
            session.apply_origin_manifest(&second),
            Err(TimelineMapError::PublishedResourceReplay { existing_proxy_seq: 0, candidate_origin_seq: 100, .. })
        ));
        assert_eq!(session.origin_epoch, 0);
        assert_eq!(session.proxy_next_seq, Some(2));
    }

    #[test]
    fn cross_host_candidate_with_only_identical_resources_does_not_advance() {
        let mut session = session();
        let first = normal_manifest_at(
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:100\n#EXTINF:4.0,\n100.ts\n",
            "https://cdn-a.example.net/live/index.m3u8",
        );
        let second = normal_manifest_at(
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:100\n#EXTINF:4.0,\n100.ts\n",
            "https://cdn-b.example.net/live/index.m3u8",
        );
        let host_a = effective_origin_host_id("cdn-a.example.net");
        let host_b = effective_origin_host_id("cdn-b.example.net");

        session.apply_origin_manifest_for_host(&first, host_a).expect("first host should map");
        session.apply_origin_manifest_for_host(&second, host_b).expect("second host should map in a new epoch");

        assert_ne!(host_a, host_b);
        assert_eq!(session.origin_epoch, 1);
        assert_eq!(session.origin_to_proxy.get(&origin_key(0, "cdn-a.example.net", 100, 0)), Some(&0));
        assert_eq!(session.origin_to_proxy.get(&origin_key(1, "cdn-b.example.net", 100, 0)), Some(&0));
        assert_eq!(session.proxy_next_seq, Some(1));
    }

    #[test]
    fn explicit_handoff_always_starts_new_epoch_even_for_lower_disjoint_sequence_range() {
        let mut session = session();
        let first = normal_manifest("#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:100\n#EXTINF:4.0,\n100.ts\n#EXTINF:4.0,\n101.ts\n");
        let second = normal_manifest("#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:4.0,\n0.ts\n#EXTINF:4.0,\n1.ts\n");
        let host_id = effective_origin_host_id("origin.example.com");

        session.apply_origin_manifest_for_host(&first, host_id).expect("first manifest should map");
        let preview =
            session.preview_origin_handoff_manifest(&second, host_id, 0).expect("explicit handoff should preview");
        session
            .apply_origin_handoff_manifest_if_preview_matches(&second, host_id, 0, &preview)
            .expect("explicit handoff should map in a new epoch");

        assert_eq!(session.origin_epoch, 1);
        assert_eq!(session.origin_seq_highwater, Some(1));
        assert_eq!(session.origin_to_proxy.get(&origin_key(1, "origin.example.com", 0, 0)), Some(&2));
        assert!(session.segments.get(&2).expect("handoff head").discontinuity_before);
    }

    #[test]
    fn cross_host_handoff_after_timeline_gc_starts_exactly_one_new_epoch() {
        let mut session = session();
        let first = normal_manifest_at(
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:1000\n#EXTINF:4.0,\n1000.ts\n#EXTINF:4.0,\n1001.ts\n",
            "https://cdn-a.example.net/live/index.m3u8",
        );
        let handoff = normal_manifest_at(
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:5\n#EXTINF:4.0,\n5.ts\n#EXTINF:4.0,\n6.ts\n",
            "https://cdn-b.example.net/live/index.m3u8",
        );
        let sliding = normal_manifest_at(
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:6\n#EXTINF:4.0,\n6.ts\n#EXTINF:4.0,\n7.ts\n",
            "https://cdn-b.example.net/live/index.m3u8",
        );
        let host_a = effective_origin_host_id("cdn-a.example.net");
        let host_b = effective_origin_host_id("cdn-b.example.net");

        session.apply_origin_manifest_for_host(&first, host_a).expect("first host should map");
        let previous_epoch = session.origin_epoch;
        session.segments.clear();
        session.origin_to_proxy.clear();

        assert_eq!(session.origin_epoch_effective_host_id, Some(host_a));
        assert_eq!(session.origin_seq_highwater, Some(1_001));
        assert_eq!(session.origin_epoch_sequence_base, Some(1_000));
        let preview = session
            .preview_origin_handoff_manifest(&handoff, host_b, 0)
            .expect("cross-host handoff after GC should preview in a new epoch");
        session
            .apply_origin_handoff_manifest_if_preview_matches(&handoff, host_b, 0, &preview)
            .expect("matching cross-host handoff should commit");

        let handoff_epoch = previous_epoch.saturating_add(1);
        assert_eq!(session.origin_epoch, handoff_epoch);
        assert_eq!(session.origin_epoch_effective_host_id, Some(host_b));
        assert_eq!(session.origin_seq_highwater, Some(6));
        assert_eq!(session.origin_epoch_sequence_base, Some(5));
        assert_eq!(session.origin_to_proxy.get(&origin_key(handoff_epoch, "cdn-b.example.net", 5, 0)), Some(&2));
        assert!(session.segments.get(&2).expect("handoff head").discontinuity_before);

        session
            .apply_origin_manifest_for_host(&sliding, host_b)
            .expect("same-host sliding refresh should remain in the handoff epoch");

        assert_eq!(session.origin_epoch, handoff_epoch);
        assert_eq!(session.origin_seq_highwater, Some(7));
        assert_eq!(session.origin_to_proxy.get(&origin_key(handoff_epoch, "cdn-b.example.net", 7, 2)), Some(&4));
        assert!(!session.segments.get(&4).expect("sliding tail").discontinuity_before);
    }

    #[test]
    fn handoff_preview_is_mutation_free_and_matches_later_commit_entries() {
        let mut session = session();
        let first = normal_manifest_at(
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:10\n#EXTINF:4.0,\n10.ts\n",
            "https://cdn-a.example.net/live/index.m3u8",
        );
        let handoff = normal_manifest_at(
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:500\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4.0,\n500.m4s\n#EXTINF:4.0,\n501.m4s\n",
            "https://cdn-b.example.net/live/index.m3u8",
        );
        let host_a = effective_origin_host_id("cdn-a.example.net");
        let host_b = effective_origin_host_id("cdn-b.example.net");
        session.apply_origin_manifest_for_host(&first, host_a).expect("first manifest should map");
        let old_epoch = session.origin_epoch;
        let old_segment_count = session.segments.len();
        let old_map_count = session.maps.len();
        let old_next_proxy_seq = session.proxy_next_seq;

        let preview = session.preview_origin_handoff_manifest(&handoff, host_b, 7).expect("handoff preview should map");

        assert_eq!(session.origin_epoch, old_epoch);
        assert_eq!(session.segments.len(), old_segment_count);
        assert_eq!(session.maps.len(), old_map_count);
        assert_eq!(session.proxy_next_seq, old_next_proxy_seq);
        assert_eq!(preview.origin_epoch, old_epoch + 1);
        assert_eq!(preview.segments.len(), 2);
        assert_eq!(preview.maps.len(), 1);
        assert!(preview.segments[0].discontinuity_before);
        assert_eq!(preview.segments[0].map_ref, Some(preview.maps[0].proxy_map_id));

        session
            .apply_origin_handoff_manifest_if_preview_matches(&handoff, host_b, 7, &preview)
            .expect("matching handoff preview should commit");

        let committed_segments = preview
            .segments
            .iter()
            .map(|planned| session.segments.get(&planned.proxy_seq).cloned().expect("committed segment"))
            .collect::<Vec<_>>();
        let committed_maps = preview
            .maps
            .iter()
            .map(|planned| session.maps.get(&planned.proxy_map_id).cloned().expect("committed map"))
            .collect::<Vec<_>>();
        assert_eq!(committed_segments, preview.segments);
        assert_eq!(committed_maps, preview.maps);
    }

    #[test]
    fn handoff_commit_rejects_stale_preview_without_partial_mutation() {
        let mut session = session();
        let first = normal_manifest_at(
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:10\n#EXTINF:4.0,\n10.ts\n",
            "https://cdn-a.example.net/live/index.m3u8",
        );
        let progressed = normal_manifest_at(
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:10\n#EXTINF:4.0,\n10.ts\n#EXTINF:4.0,\n11.ts\n",
            "https://cdn-a.example.net/live/index.m3u8",
        );
        let handoff = normal_manifest_at(
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:500\n#EXTINF:4.0,\n500.ts\n",
            "https://cdn-b.example.net/live/index.m3u8",
        );
        let host_a = effective_origin_host_id("cdn-a.example.net");
        let host_b = effective_origin_host_id("cdn-b.example.net");
        session.apply_origin_manifest_for_host(&first, host_a).expect("first manifest should map");
        let stale_preview =
            session.preview_origin_handoff_manifest(&handoff, host_b, 0).expect("handoff preview should map");
        session.apply_origin_manifest_for_host(&progressed, host_a).expect("same-host progress should commit");
        let epoch_before_rejected_commit = session.origin_epoch;
        let segments_before_rejected_commit = session.segments.clone();
        let next_proxy_seq_before_rejected_commit = session.proxy_next_seq;

        let result = session.apply_origin_handoff_manifest_if_preview_matches(&handoff, host_b, 0, &stale_preview);

        assert_eq!(result, Err(HlsOriginHandoffPreviewError::PreviewInconsistent));
        assert_eq!(session.origin_epoch, epoch_before_rejected_commit);
        assert_eq!(session.segments, segments_before_rejected_commit);
        assert_eq!(session.proxy_next_seq, next_proxy_seq_before_rejected_commit);
    }

    #[test]
    fn effective_host_id_is_stable_and_normalized_without_retaining_host_text() {
        let normalized = effective_origin_host_id("CDN.Example.NET.");

        assert_eq!(normalized, effective_origin_host_id("cdn.example.net"));
        assert_ne!(normalized, effective_origin_host_id("other.example.net"));
        assert_ne!(normalized, 0);
    }

    #[test]
    fn mapping_error_does_not_commit_partial_session_state() {
        let mut session = session();
        let first = normal_manifest("#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:10\n#EXTINF:4.0,\n10.ts\n");
        let invalid =
            normal_manifest("#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:11\n#EXTINF:4.0,\n11.ts\n#EXTINF:4.0,\n12.webm\n");

        session.apply_origin_manifest(&first).expect("first manifest should map");
        let previous_proxy_next_seq = session.proxy_next_seq;
        let previous_highwater = session.origin_seq_highwater;
        let previous_head = session.publishable_origin_head_proxy_seq;
        let previous_tail = session.publishable_origin_tail_proxy_seq;
        let previous_segments = session.segments.clone();
        let previous_origin_to_proxy = session.origin_to_proxy.clone();

        assert_eq!(session.apply_origin_manifest(&invalid), Err(TimelineMapError::UnsupportedSegmentExtension));

        assert_eq!(session.proxy_next_seq, previous_proxy_next_seq);
        assert_eq!(session.origin_seq_highwater, previous_highwater);
        assert_eq!(session.publishable_origin_head_proxy_seq, previous_head);
        assert_eq!(session.publishable_origin_tail_proxy_seq, previous_tail);
        assert_eq!(session.segments, previous_segments);
        assert_eq!(session.origin_to_proxy, previous_origin_to_proxy);
    }

    #[test]
    fn unsupported_segment_extension_fails_mapping() {
        let mut session = session();
        let manifest = normal_manifest("#EXTM3U\n#EXTINF:4.0,\nseg.webm\n");

        assert_eq!(session.apply_origin_manifest(&manifest), Err(TimelineMapError::UnsupportedSegmentExtension));
    }

    #[test]
    fn mapped_maps_start_as_discovered_placeholders() {
        let mut session = session();
        let manifest = normal_manifest("#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4.0,\nseg.m4s\n");

        session.apply_origin_manifest(&manifest).expect("manifest should map");

        let map = session.maps.values().next().expect("map placeholder");
        assert_eq!(map.status, MapCacheStatus::Discovered);
        assert_eq!(map.proxy_map_id, ProxyMapId(0));
        assert_eq!(map.proxy_file_ext, "mp4");
        assert_eq!(map.origin_key.origin_epoch, 0);
        assert_eq!(map.origin_key.byte_range, None);
        assert_eq!(map.cache_key.proxy_map_id(), ProxyMapId(0));
        let fetch_ref = map.origin_fetch_ref.as_ref().expect("map fetch ref");
        assert_eq!(fetch_ref.byte_range, None);
        assert!(format!("{fetch_ref:?}").contains("<redacted>"));
        assert!(!format!("{fetch_ref:?}").contains("init.mp4"));
    }

    #[test]
    fn map_fetch_ref_preserves_final_manifest_host_for_relative_map_uri() {
        let mut session = session();
        let manifest = parse_origin_media_manifest(
            "#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4.0,\nseg.m4s\n",
            "https://cdn.example.net/live/redirected/playlist.m3u8",
        );
        let tuliprox_parser::hls::origin_manifest::OriginManifestParseOutcome::Normal(manifest) = manifest else {
            panic!("manifest should parse as normal timeline");
        };

        session.apply_origin_manifest(&manifest).expect("manifest should map");

        let map = session.maps.get(&ProxyMapId(0)).expect("map placeholder");
        assert_eq!(map.origin_key.resolved_origin_uri, "https://cdn.example.net/live/redirected/init.mp4");
        assert_eq!(
            map.origin_fetch_ref.as_ref().expect("map fetch ref").resolved_origin_url,
            "https://cdn.example.net/live/redirected/init.mp4"
        );
    }

    #[test]
    fn manifest_mapping_sets_origin_fetch_ref() {
        let mut session = session();
        let manifest = normal_manifest("#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:1\n#EXTINF:4.0,\nseg.ts\n");

        session.apply_origin_manifest(&manifest).expect("manifest should map");

        let segment = session.segments.get(&0).expect("segment should be mapped");
        let fetch_ref = segment.origin_fetch_ref.as_ref().expect("fetch ref should be set");
        assert!(format!("{fetch_ref:?}").contains("<redacted>"));
        assert!(!format!("{fetch_ref:?}").contains("seg.ts"));
    }

    #[test]
    fn segment_fetch_ref_preserves_final_manifest_host_for_relative_segment_uri() {
        let mut session = session();
        let manifest = parse_origin_media_manifest(
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:10\n#EXTINF:4.0,\nmedia/seg001.ts\n",
            "https://cdn.example.net/live/redirected/playlist.m3u8",
        );
        let tuliprox_parser::hls::origin_manifest::OriginManifestParseOutcome::Normal(manifest) = manifest else {
            panic!("manifest should parse as normal timeline");
        };

        session.apply_origin_manifest(&manifest).expect("manifest should map");

        let segment = session.segments.get(&0).expect("segment should be mapped");
        assert_eq!(segment.origin_key, origin_key(0, "cdn.example.net", 10, 0));
        assert_eq!(
            segment.origin_fetch_ref.as_ref().expect("fetch ref").resolved_origin_url,
            "https://cdn.example.net/live/redirected/media/seg001.ts"
        );
    }

    #[test]
    fn cold_start_prefetch_prioritizes_visible_window_then_known_tail() {
        let mut session = session();
        session.initial_prefetch_gap_segments = 3;
        session.configure_segment_prefetch_queue(6);
        let manifest = normal_manifest(
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:100\n#EXTINF:4.0,\n100.ts\n#EXTINF:4.0,\n101.ts\n#EXTINF:4.0,\n102.ts\n#EXTINF:4.0,\n103.ts\n#EXTINF:4.0,\n104.ts\n#EXTINF:4.0,\n105.ts\n",
        );

        session.apply_origin_manifest(&manifest).expect("manifest should map");
        session.queue_manifest_prefetch_candidates(10);

        assert!(matches!(
            session.segments.get(&0).expect("0").status,
            SegmentCacheStatus::Queued { priority: SegmentFetchPriority::RenderWindow, .. }
        ));
        assert!(matches!(
            session.segments.get(&2).expect("2").status,
            SegmentCacheStatus::Queued { priority: SegmentFetchPriority::RenderWindow, .. }
        ));
        assert!(matches!(
            session.segments.get(&3).expect("3").status,
            SegmentCacheStatus::Queued { priority: SegmentFetchPriority::Prefetch, .. }
        ));
        assert_eq!(session.segment_prefetch_queue.prefetch_len(), 3);
    }

    #[test]
    fn same_map_uri_with_different_byterange_uses_distinct_proxy_map_ids() {
        let mut session = session();
        let manifest = normal_manifest(
            "#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\",BYTERANGE=\"100@0\"\n#EXTINF:4.0,\n1.m4s\n#EXT-X-MAP:URI=\"init.mp4\",BYTERANGE=\"100@100\"\n#EXTINF:4.0,\n2.m4s\n",
        );

        session.apply_origin_manifest(&manifest).expect("manifest should map");

        assert_eq!(session.maps.len(), 2);
        assert_eq!(session.segments.get(&0).expect("first segment").map_ref, Some(ProxyMapId(0)));
        assert_eq!(session.segments.get(&1).expect("second segment").map_ref, Some(ProxyMapId(1)));
    }

    #[test]
    fn same_map_uri_in_same_epoch_reuses_proxy_map_id() {
        let mut session = session();
        let manifest = normal_manifest(
            "#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4.0,\n1.m4s\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4.0,\n2.m4s\n",
        );

        session.apply_origin_manifest(&manifest).expect("manifest should map");

        assert_eq!(session.maps.len(), 1);
        assert_eq!(session.segments.get(&0).expect("first segment").map_ref, Some(ProxyMapId(0)));
        assert_eq!(session.segments.get(&1).expect("second segment").map_ref, Some(ProxyMapId(0)));
    }

    #[test]
    fn same_map_uri_after_rollover_uses_distinct_proxy_map_id() {
        let mut session = session();
        let first =
            normal_manifest("#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:322\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4.0,\n322.m4s\n");
        let second =
            normal_manifest("#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4.0,\n0.m4s\n");

        session.apply_origin_manifest(&first).expect("first manifest should map");
        session.apply_origin_manifest(&second).expect("second manifest should map");

        assert_eq!(session.maps.len(), 2);
        assert_eq!(session.segments.get(&0).expect("first epoch segment").map_ref, Some(ProxyMapId(0)));
        assert_eq!(session.segments.get(&1).expect("second epoch segment").map_ref, Some(ProxyMapId(1)));
        assert_eq!(session.maps.get(&ProxyMapId(1)).expect("second map").origin_key.origin_epoch, 1);
    }

    #[test]
    fn same_relative_map_uri_on_different_final_hosts_uses_distinct_proxy_map_ids() {
        let mut session = session();
        let first = parse_origin_media_manifest(
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4.0,\n0.m4s\n",
            "https://cdn-a.example.net/live/playlist.m3u8",
        );
        let second = parse_origin_media_manifest(
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:1\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4.0,\n1.m4s\n",
            "https://cdn-b.example.net/live/playlist.m3u8",
        );
        let tuliprox_parser::hls::origin_manifest::OriginManifestParseOutcome::Normal(first) = first else {
            panic!("first manifest should parse as normal timeline");
        };
        let tuliprox_parser::hls::origin_manifest::OriginManifestParseOutcome::Normal(second) = second else {
            panic!("second manifest should parse as normal timeline");
        };

        session.apply_origin_manifest(&first).expect("first manifest should map");
        session.apply_origin_manifest(&second).expect("second manifest should map");

        assert_eq!(session.maps.len(), 2);
        assert_eq!(
            session.maps.get(&ProxyMapId(0)).expect("first map").origin_key.resolved_origin_uri,
            "https://cdn-a.example.net/live/init.mp4"
        );
        assert_eq!(
            session.maps.get(&ProxyMapId(1)).expect("second map").origin_key.resolved_origin_uri,
            "https://cdn-b.example.net/live/init.mp4"
        );
        assert_eq!(session.segments.get(&0).expect("first segment").map_ref, Some(ProxyMapId(0)));
        assert_eq!(session.segments.get(&1).expect("second segment").map_ref, Some(ProxyMapId(1)));
    }

    #[test]
    fn media_clock_survives_complete_collection_and_origin_rollover_with_variable_durations() {
        let mut session = session();
        session
            .apply_origin_manifest(&normal_manifest(
                "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXT-X-MEDIA-SEQUENCE:100\n\
             #EXTINF:4,\n100.ts\n#EXTINF:6,\n101.ts\n#EXTINF:5,\n102.ts\n",
            ))
            .expect("initial variable timeline");
        assert_eq!(
            session.segments.values().map(|entry| entry.media_start_ms).collect::<Vec<_>>(),
            vec![0, 4_000, 10_000]
        );
        session.segments.clear();
        session.origin_to_proxy.clear();
        session
            .apply_origin_rebase_manifest(
                &normal_manifest(
                    "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXT-X-MEDIA-SEQUENCE:0\n\
             #EXTINF:6,\nnew-0.ts\n#EXTINF:3,\nnew-1.ts\n",
                ),
                2,
                false,
            )
            .expect("rebased timeline after collection");
        assert_eq!(
            session.segments.values().map(|entry| (entry.proxy_seq, entry.media_start_ms)).collect::<Vec<_>>(),
            vec![(3, 15_000), (4, 21_000)]
        );
        assert_eq!(session.next_media_start_ms, 24_000);
    }

    /// After a host handoff, a later refresh past the old published prefix keeps the epoch and sequence mapping.
    #[test]
    fn sliding_refresh_after_handoff_keeps_current_mapping_after_old_published_prefix() {
        let mut session = session();
        let host_a = effective_origin_host_id("cdn-a.example.com");
        let host_b = effective_origin_host_id("cdn-b.example.com");
        session.apply_origin_manifest_for_host(&six_segment_manifest(0), host_a).unwrap();
        publish_ready_manifest(&mut session, 1);
        let handoff = six_segment_manifest(3);
        let preview = session.preview_origin_handoff_manifest(&handoff, host_b, 0).unwrap();
        session.apply_origin_handoff_manifest_if_preview_matches(&handoff, host_b, 0, &preview).unwrap();
        publish_ready_manifest(&mut session, 2);
        let epoch = session.origin_epoch;
        session
            .apply_origin_manifest_for_host(&six_segment_manifest(4), host_b)
            .expect("old published prefix followed by newer current mappings must remain valid");
        assert_eq!(session.origin_epoch, epoch);
        assert_eq!(session.origin_seq_highwater, Some(9));
        assert_eq!(session.proxy_next_seq, Some(10));
    }

    /// After a host handoff, a refresh that reorders already published segments is rejected.
    #[test]
    fn sliding_refresh_after_handoff_rejects_current_mapping_behind_trimmed_prefix() {
        let mut session = session();
        let host_a = effective_origin_host_id("cdn-a.example.com");
        let host_b = effective_origin_host_id("cdn-b.example.com");
        session.apply_origin_manifest_for_host(&six_segment_manifest(0), host_a).unwrap();
        publish_ready_manifest(&mut session, 1);
        let handoff = six_segment_manifest(3);
        let preview = session.preview_origin_handoff_manifest(&handoff, host_b, 0).unwrap();
        session.apply_origin_handoff_manifest_if_preview_matches(&handoff, host_b, 0, &preview).unwrap();
        publish_ready_manifest(&mut session, 2);
        let contradictory = normal_manifest(
            "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:5\n\
             #EXTINF:4,\n8.ts\n#EXTINF:4,\n6.ts\n#EXTINF:4,\n7.ts\n#EXTINF:4,\n8.ts\n#EXTINF:4,\n9.ts\n",
        );
        assert!(matches!(
            session.apply_origin_manifest_for_host(&contradictory, host_b),
            Err(TimelineMapError::OriginSequenceResourceConflict { existing_proxy_seq: 5, candidate_origin_seq: 5 })
        ));
    }
    #[test]
    fn occurrence_evidence_survives_account_handoff_fetch_replacement_and_headers() {
        let mut session = HlsSession::new(HlsSessionKey::new(7, "stream"), b"secret", 0);
        session.replace_origin_account_binding(Some(crate::origin::HlsOriginAccountBinding::rebound(
            std::sync::Arc::from("input"),
            std::sync::Arc::from("account-a"),
            "owner".to_string(),
            1,
            0,
        )));
        let manifest = normal_manifest("#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:101\n#EXTINF:4,\nseg.ts\n");
        session.apply_origin_manifest(&manifest).unwrap();
        let seq = *session.segments.first_key_value().unwrap().0;
        let identity = session.segments[&seq].occurrence_identity.expect("stored occurrence");
        let resource = session.segments[&seq].media_resource_identity().unwrap();
        let old_work = session.start_origin_work();
        session.handoff_origin_account(crate::origin::HlsOriginAccountBinding::rebound(
            std::sync::Arc::from("input"),
            std::sync::Arc::from("account-b"),
            "owner".to_string(),
            2,
            1,
        ));
        assert!(!session.finish_origin_work(old_work));
        assert!(session.segments[&seq].origin_fetch_ref.is_none());
        assert_eq!(session.segments[&seq].occurrence_identity, Some(identity));
        assert_eq!(session.segments[&seq].media_resource_identity(), Some(resource));
        // An ordinary same-playlist reload attaches current fetch metadata without replacing evidence.
        session.apply_origin_manifest(&manifest).unwrap();
        assert!(session.segments[&seq].origin_fetch_ref.is_some());
        let entry = session.segments.get_mut(&seq).unwrap();
        entry.origin_fetch_ref.as_mut().unwrap().resolved_origin_url =
            "http://cdn-b.example:8080/live/final/seg.ts".to_string();
        assert_eq!(entry.occurrence_identity, Some(identity));
        assert_eq!(entry.media_resource_identity(), Some(resource));
        // Replacing the account-dependent target cannot rewrite stored playlist evidence,
        // even when it supplies a different path/query or transport range.
        entry.origin_fetch_ref.as_mut().unwrap().resolved_origin_url =
            "https://account-c.example/rebound/seg.ts?account=fixture".to_string();
        entry.origin_fetch_ref.as_mut().unwrap().byte_range = Some(super::ParsedByteRange { length: 100, offset: 0 });
        assert_eq!(entry.occurrence_identity, Some(identity));
        assert_eq!(entry.origin_byte_range, None);
        for range in [None, Some(axum::http::HeaderValue::from_static("bytes=0-99"))] {
            let mut client_headers = axum::http::HeaderMap::new();
            if let Some(range) = range {
                client_headers.insert(axum::http::header::RANGE, range);
            }
            let headers = crate::build_hls_origin_resource_headers(
                &client_headers,
                &axum::http::HeaderMap::new(),
                entry.origin_byte_range,
            )
            .unwrap();
            assert!(!headers.contains_key(axum::http::header::RANGE));
            for content_range in ["bytes 0-99/1000", "bytes 500-999/1000"] {
                let mut response_headers = axum::http::HeaderMap::new();
                response_headers.insert(axum::http::header::CONTENT_RANGE, content_range.parse().unwrap());
                assert!(response_headers.contains_key(axum::http::header::CONTENT_RANGE));
                assert_eq!(entry.occurrence_identity, Some(identity));
                assert_eq!(entry.origin_byte_range, None);
            }
        }
    }
    #[test]
    fn occurrence_mirror_overlap_preserves_ready_slots_and_published_revision() {
        let mut state = session();
        let baseline = normal_manifest_at("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:100\n#EXTINF:4,\na.ts\n#EXTINF:4,\nb.ts\n#EXTINF:4,\nc.ts\n", "https://a.example/live/index.m3u8");
        state.apply_origin_manifest_for_host(&baseline, effective_origin_host_id("a.example")).unwrap();
        publish_ready_manifest(&mut state, 1);
        let old = state.segments[&1].clone();
        let store = crate::SegmentRevisionStore::default();
        let revision = store.create(state.proxy_session_id.clone(), 1, crate::SegmentRevisionKind::Raw).unwrap();
        revision.revision().bind_origin(&state);
        assert!(revision
            .revision()
            .complete(crate::CachedSegmentMetadata { path: "/synthetic/published-revision.ts".into(), size: 1 }));
        let revision_id = revision.revision().id();
        let published = state.last_rendered_manifest.clone().unwrap();
        let next = normal_manifest_at("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:101\n#EXTINF:4,\nb.ts\n#EXTINF:4,\nc.ts\n#EXTINF:4,\nd.ts\n", "http://b.example:8080/live/index.m3u8");
        state.apply_origin_manifest_for_host(&next, effective_origin_host_id("b.example")).unwrap();
        assert_eq!(state.live_origin_sequences(), &[1, 2, 3]);
        assert_eq!(state.segments.len(), 4);
        assert_eq!(state.segments[&1], old);
        assert_eq!(revision.revision().id(), revision_id);
        assert!(revision.revision().is_complete());
        assert!(revision.revision().origin_binding_is_current(&state));
        assert_eq!(state.last_rendered_manifest, Some(published));
        assert_eq!(state.segments[&3].media_start_ms, 12_000);
    }

    #[test]
    fn group_timeline_uses_members_scope_for_overlap_and_independent_handoff() {
        let source = crate::HlsOriginSource::new(
            7,
            std::sync::Arc::from("member-a"),
            "411",
            crate::origin::HlsOriginSourceKind::DirectMediaPlaylist,
        );
        let mut state =
            HlsSession::new_with_origin_source(crate::HlsSessionKey::for_group(1, 555), source.clone(), b"group", 0);
        let proxy = state.proxy_session_id.clone();
        let baseline = rebase_overlap_manifest(100, &["a.ts", "b.ts", "c.ts"]);
        state.apply_origin_manifest_for_host(&baseline, effective_origin_host_id("a.example")).unwrap();
        publish_ready_manifest(&mut state, 1);
        let old = state.segments[&1].clone();
        let original_scope = old.occurrence_identity.expect("group occurrence").playlist_scope();
        assert_eq!(
            Some(original_scope),
            crate::resource_identity::HlsMediaPlaylistScope::from_session(&source.session_key())
        );

        let overlap = rebase_overlap_manifest(101, &["b.ts", "c.ts", "d.ts"]);
        state.apply_continuous_origin_manifest(&overlap, effective_origin_host_id("mirror.example")).unwrap();
        assert_eq!(state.live_origin_sequences(), &[1, 2, 3]);
        assert_eq!(state.segments[&1], old);
        let conflicting = rebase_overlap_manifest(101, &["changed.ts", "c.ts", "d.ts"]);
        assert_eq!(
            state.apply_continuous_origin_manifest(&conflicting, effective_origin_host_id("mirror.example")),
            Err(TimelineMapError::OriginSequenceResourceConflict { existing_proxy_seq: 1, candidate_origin_seq: 101 })
        );
        assert_eq!(state.live_origin_sequences(), &[1, 2, 3]);
        let epoch = state.origin_epoch;
        let next = state.proxy_next_seq.unwrap();
        let independent = crate::HlsOriginSource::new(
            8,
            std::sync::Arc::from("member-b"),
            "722",
            crate::origin::HlsOriginSourceKind::DirectMediaPlaylist,
        );
        let binding = crate::HlsOriginAccountBinding::rebound(
            std::sync::Arc::from("member-b"),
            std::sync::Arc::from("account-b"),
            crate::origin::build_hls_origin_session_owner(&proxy),
            1,
            2,
        );
        state.handoff_origin_member(independent.clone(), binding);
        assert!(crate::refresh::continuity::HlsCacheOriginAnchor::capture(&state).is_none());
        let b = rebase_overlap_manifest(100, &["new1.ts", "new2.ts", "new3.ts"]);
        state.apply_origin_manifest_for_host(&b, effective_origin_host_id("new.example")).unwrap();
        let first = &state.segments[&next];
        assert_eq!(state.proxy_session_id, proxy);
        assert_eq!(state.origin_epoch, epoch + 1);
        assert!(first.discontinuity_before);
        assert_eq!(first.origin_key.host_local_sequence, 100);
        assert_ne!(first.occurrence_identity.expect("B scope").playlist_scope(), original_scope);
        assert_eq!(state.segments[&1], old);
    }

    #[test]
    fn occurrence_binding_conflicts_reject_complete_candidate_atomically() {
        for (reference, range) in [("a.ts", ""), ("b.ts?changed=1", ""), ("b.ts", "#EXT-X-BYTERANGE:100@0\n")] {
            let mut state = session();
            state.apply_origin_manifest(&rebase_overlap_manifest(100, &["a.ts", "b.ts", "c.ts"])).unwrap();
            publish_ready_manifest(&mut state, 1);
            let before = super::TimelineDraft::from(&state);
            let published = state.last_rendered_manifest.clone();
            let candidate = normal_manifest(&format!("#EXTM3U\n#EXT-X-VERSION:4\n#EXT-X-MEDIA-SEQUENCE:101\n{range}#EXTINF:7,\n{reference}\n#EXTINF:7,\nc.ts\n"));
            assert_eq!(
                state.apply_origin_manifest(&candidate),
                Err(TimelineMapError::OriginSequenceResourceConflict {
                    existing_proxy_seq: 1,
                    candidate_origin_seq: 101
                })
            );
            assert_eq!(super::TimelineDraft::from(&state), before);
            assert_eq!(state.last_rendered_manifest, published);
        }
    }

    #[test]
    fn occurrence_repeated_uri_with_discontinuity_has_distinct_msn_and_renders() {
        let mut state = session();
        let manifest = normal_manifest("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:100\n#EXTINF:4,\nsame.ts\n#EXT-X-DISCONTINUITY\n#EXTINF:4,\nsame.ts\n#EXTINF:4,\nnext.ts\n");
        state.apply_origin_manifest(&manifest).unwrap();
        assert_eq!(state.live_origin_sequences(), &[0, 1, 2]);
        assert_ne!(state.segments[&0].occurrence_identity, state.segments[&1].occurrence_identity);
        let rendered = publish_ready_manifest(&mut state, 1);
        assert!(rendered.contains("#EXT-X-DISCONTINUITY"));
        assert_eq!(state.published_resource_history.len(), 3);
        state.apply_origin_manifest(&manifest).unwrap();
        assert_eq!(state.proxy_next_seq, Some(3));
    }

    #[test]
    fn occurrence_history_preserves_collected_msn_binding_and_restart_boundary() {
        let mut state = session();
        state.apply_origin_manifest(&rebase_overlap_manifest(100, &["same.ts", "b.ts", "c.ts"])).unwrap();
        publish_ready_manifest(&mut state, 1);
        state.segments.remove(&0);
        state.origin_to_proxy.retain(|_, seq| *seq != 0);
        state.apply_origin_manifest(&rebase_overlap_manifest(100, &["same.ts", "b.ts", "c.ts", "same.ts"])).unwrap();
        assert_eq!(state.live_origin_sequences(), &[1, 2, 3]);
        assert_eq!(state.proxy_next_seq, Some(4));
        assert!(!state.segments.contains_key(&0));
        assert_eq!(state.segments[&3].origin_key.host_local_sequence, 103);
        let before = super::TimelineDraft::from(&state);
        assert!(matches!(
            state.apply_origin_manifest(&rebase_overlap_manifest(100, &["changed.ts"])),
            Err(TimelineMapError::OriginSequenceResourceConflict { existing_proxy_seq: 0, .. })
        ));
        assert_eq!(super::TimelineDraft::from(&state), before);
        // Only the explicitly verified restart API opens a new comparison range.
        let restart = rebase_overlap_manifest(100, &["changed.ts"]);
        let host = effective_origin_host_id("origin.example.com");
        let preview = state.preview_verified_restart(&restart, host).unwrap();
        state.apply_verified_restart(&restart, host, &preview).unwrap();
        assert_eq!(state.live_origin_sequences(), &[4]);
    }

    #[test]
    fn occurrence_ready_dependencies_are_preserved_across_new_transport_offers() {
        let mut state = session();
        let baseline = normal_manifest("#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:100\n#EXT-X-MAP:URI=\"old.mp4\"\n#EXT-X-KEY:METHOD=AES-128,URI=\"old.key\"\n#EXTINF:4,\nseg.ts\n");
        let mut baseline = baseline;
        baseline.segments[0].encryption.as_mut().unwrap().proxy_resource_id = Some("old-key".into());
        baseline.segments[0].encryption.as_mut().unwrap().proxy_resource_extension = Some("key".into());
        state.apply_origin_manifest(&baseline).unwrap();
        state.segments.get_mut(&0).unwrap().status = SegmentCacheStatus::Ready { content_length: 10, ready_at_ms: 1 };
        let old = state.segments[&0].clone();
        let maps = state.maps.clone();
        let replacement = normal_manifest_at("#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:100\n#EXT-X-MAP:URI=\"new.mp4\"\n#EXT-X-KEY:METHOD=AES-128,URI=\"new.key\"\n#EXTINF:4,\nseg.ts\n", "https://mirror.example/live/final/index.m3u8");
        state.apply_origin_manifest_for_host(&replacement, effective_origin_host_id("mirror.example")).unwrap();
        assert_eq!(state.segments[&0], old);
        assert_eq!(state.maps, maps, "unverified replacement dependencies must not allocate new bindings");
    }
}
