use super::{
    is_hls_provisioning_gap_segment, is_hls_provisioning_segment, session::HlsPublishedLiveOriginBaseline,
    HlsSegmentEncryption, HlsSession, MapCacheStatus, ProxySessionId, SegmentCacheStatus, SegmentEntry,
    HLS_ACCESS_LEASE_ID_PLACEHOLDER, HLS_PROVISIONING_TARGET_DURATION_SECS,
};
use crate::live_window::HlsPublishedWindow;
use shared::model::HlsStripMode;
use std::fmt::Write as _;
use tuliprox_core::{
    model::StripConfig,
    utils::{format_hls_duration_ms, hls_target_duration_secs},
};

pub(crate) const MIN_VISIBLE_SEGMENTS: usize = 3;
const TARGET_VISIBLE_SEGMENTS: usize = 6;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct RenderPolicy {
    pub initial_render_gap_segments: usize,
    pub max_not_ready_render_segments: usize,
}

impl RenderPolicy {
    pub fn new(initial_render_gap_segments: usize) -> Self {
        Self { initial_render_gap_segments, max_not_ready_render_segments: 2 }
    }

    pub fn from_strip_config(strip: &StripConfig, segment_durations_ms: &[u64]) -> Self {
        match strip.mode {
            HlsStripMode::Segments => Self::new(usize::try_from(strip.value).unwrap_or(usize::MAX)),
            HlsStripMode::Seconds => {
                let target_ms = strip.value.saturating_mul(1_000);
                let mut accumulated_ms = 0_u64;
                let mut gap_segments = 0_usize;
                for duration_ms in segment_durations_ms.iter().rev() {
                    if accumulated_ms >= target_ms {
                        break;
                    }
                    accumulated_ms = accumulated_ms.saturating_add(*duration_ms);
                    gap_segments = gap_segments.saturating_add(1);
                }
                Self::new(gap_segments)
            }
        }
    }
}

impl Default for RenderPolicy {
    fn default() -> Self { Self::new(0) }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RenderedManifest {
    pub body: String,
    pub first_proxy_seq: u64,
    pub last_proxy_seq: u64,
    pub discontinuity_sequence: u64,
    pub target_duration_ms: u64,
    pub playlist_duration_ms: u64,
    pub valid_until_ms: u64,
    pub render_gap_segments: usize,
    pub rendered_at_ms: u64,
    pub segment_proxy_seqs: Vec<u64>,
}

impl RenderedManifest {
    pub(crate) fn published_window(&self) -> HlsPublishedWindow {
        HlsPublishedWindow {
            first: self.first_proxy_seq,
            last: self.last_proxy_seq,
            tagged_head: self
                .body
                .lines()
                .take_while(|line| !line.starts_with("#EXTINF:"))
                .any(|line| line == "#EXT-X-DISCONTINUITY"),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RenderedManifestStoreOutcome {
    Stored,
    Rejected(RenderedManifestStoreRejectReason),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RenderedManifestStoreRejectReason {
    RegressiveMediaSequence { previous_first_proxy_seq: u64, candidate_first_proxy_seq: u64 },
    DuplicateMediaResource { existing_proxy_seq: u64, candidate_proxy_seq: u64 },
    CommitGenerationExhausted,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum RenderError {
    NoRenderableWindow,
    InvalidState,
    StoreRejected(RenderedManifestStoreRejectReason),
}

pub struct HlsManifestRenderer;

#[derive(Clone, Copy)]
enum RenderTailPolicy {
    RequirePlannedTail,
    TruncateUnavailableTail,
}

impl HlsManifestRenderer {
    /// Reuses the last stored session window as the sole source for a new entry.
    /// Fast start applies its entry slice to that source instead of the latest origin commit.
    pub fn render_recovery_entry_window(
        session: &HlsSession,
        strip: &tuliprox_core::model::StripConfig,
        now_ms: u64,
    ) -> Result<(RenderedManifest, Option<crate::live_window::HlsLiveWindowPlan>), RenderError> {
        let source = session.last_rendered_manifest.as_ref().ok_or(RenderError::NoRenderableWindow)?;
        if now_ms > source.valid_until_ms
            || source.segment_proxy_seqs.is_empty()
            || source
                .segment_proxy_seqs
                .iter()
                .any(|seq| session.segments.get(seq).is_none_or(crate::is_hls_provisioning_segment))
        {
            return Err(RenderError::NoRenderableWindow);
        }
        if !session.uses_fast_start_live_window() {
            return Ok((source.clone(), None));
        }
        let plan = session.plan_recovery_entry(source, strip).ok_or(RenderError::InvalidState)?;
        render_window(session, &plan.visible, plan.effective_strip_segments, source.rendered_at_ms, None)
            .map(|rendered| (rendered, Some(plan)))
    }

    pub fn render_live_window(
        session: &HlsSession,
        plan: &crate::live_window::HlsLiveWindowPlan,
        rendered_at_ms: u64,
    ) -> Result<RenderedManifest, RenderError> {
        Self::validate_live_window(session, plan)?;
        render_window(session, &plan.visible, plan.effective_strip_segments, rendered_at_ms, plan.previous_window)
    }

    fn validate_live_window(
        session: &HlsSession,
        plan: &crate::live_window::HlsLiveWindowPlan,
    ) -> Result<(), RenderError> {
        if !plan.is_current(session) {
            return Err(RenderError::InvalidState);
        }
        let minimum =
            if session.allows_short_origin_windows() || plan.is_startup_recovery() { 1 } else { MIN_VISIBLE_SEGMENTS };
        let startup = session.startup.as_ref().ok_or(RenderError::InvalidState)?;
        let head = plan.visible.first().ok_or(RenderError::NoRenderableWindow)?;
        if plan.visible.len() < minimum
            || !startup
                .revisions
                .get(head)
                .is_some_and(|revision| revision.revision().is_startup_head_ready(startup.config.mode))
            || !plan
                .visible
                .iter()
                .all(|seq| session.segments.get(seq).is_some_and(|entry| is_renderable(entry, session)))
            || (plan.is_startup_recovery() && !plan.is_ready(session))
        {
            return Err(RenderError::NoRenderableWindow);
        }
        Ok(())
    }

    pub fn render(session: &HlsSession, rendered_at_ms: u64) -> Result<RenderedManifest, RenderError> {
        if session.uses_fast_start_live_window() {
            let plan = session.session_live_window_plan().ok_or(RenderError::NoRenderableWindow)?;
            return Self::render_live_window(session, &plan, rendered_at_ms);
        }
        for render_gap_segments in 0..=session.render_policy.initial_render_gap_segments {
            let Some(window) = select_window(session, render_gap_segments, RenderTailPolicy::TruncateUnavailableTail)
            else {
                continue;
            };
            let previous = session.last_rendered_manifest.as_ref().map(RenderedManifest::published_window);
            let manifest = render_window(session, &window, render_gap_segments, rendered_at_ms, previous)?;
            return Ok(manifest);
        }

        Err(RenderError::NoRenderableWindow)
    }
}

impl HlsSession {
    pub(crate) fn prepare_live_window_discontinuity(&mut self, plan: &crate::live_window::HlsLiveWindowPlan) {
        if HlsManifestRenderer::validate_live_window(self, plan).is_ok() {
            self.reconcile_live_discontinuities();
        }
    }

    pub(crate) fn render_manifest_for_publication(
        &mut self,
        rendered_at_ms: u64,
    ) -> Result<RenderedManifest, RenderError> {
        self.reconcile_live_discontinuities();
        HlsManifestRenderer::render(self, rendered_at_ms)
    }

    pub fn render_and_store_manifest(&mut self, rendered_at_ms: u64) -> Result<RenderedManifest, RenderError> {
        self.reconcile_startup_eligibility();
        super::segment_fetcher::recompute_unpublished_live_head(self);
        let rendered = self.render_manifest_for_publication(rendered_at_ms)?;
        match self.store_rendered_manifest(rendered.clone()) {
            RenderedManifestStoreOutcome::Stored => Ok(rendered),
            RenderedManifestStoreOutcome::Rejected(reason) => Err(RenderError::StoreRejected(reason)),
        }
    }

    pub fn store_rendered_manifest(&mut self, rendered: RenderedManifest) -> RenderedManifestStoreOutcome {
        if let Some(previous) = &self.last_rendered_manifest {
            if rendered.first_proxy_seq < previous.first_proxy_seq {
                return RenderedManifestStoreOutcome::Rejected(
                    RenderedManifestStoreRejectReason::RegressiveMediaSequence {
                        previous_first_proxy_seq: previous.first_proxy_seq,
                        candidate_first_proxy_seq: rendered.first_proxy_seq,
                    },
                );
            }
        }
        let rendered_identities = rendered
            .segment_proxy_seqs
            .iter()
            .filter_map(|proxy_seq| {
                self.segments
                    .get(proxy_seq)
                    .and_then(|entry| entry.occurrence_identity)
                    .map(|identity| (*proxy_seq, identity))
            })
            .collect::<Vec<_>>();
        for (index, (candidate_proxy_seq, candidate_identity)) in rendered_identities.iter().enumerate() {
            if let Some((existing_proxy_seq, _)) = rendered_identities[..index]
                .iter()
                .find(|(_, existing_identity)| existing_identity.same_position(*candidate_identity))
            {
                return RenderedManifestStoreOutcome::Rejected(
                    RenderedManifestStoreRejectReason::DuplicateMediaResource {
                        existing_proxy_seq: *existing_proxy_seq,
                        candidate_proxy_seq: *candidate_proxy_seq,
                    },
                );
            }
            if let Some((published, existing_proxy_seq)) =
                self.published_resource_history.binding_for(*candidate_identity)
            {
                if existing_proxy_seq != *candidate_proxy_seq || published != *candidate_identity {
                    return RenderedManifestStoreOutcome::Rejected(
                        RenderedManifestStoreRejectReason::DuplicateMediaResource {
                            existing_proxy_seq,
                            candidate_proxy_seq: *candidate_proxy_seq,
                        },
                    );
                }
            }
        }
        let published_origin_evidence = rendered.segment_proxy_seqs.iter().find_map(|proxy_seq| {
            let segment = self.segments.get(proxy_seq)?;
            if is_local_provisioning_segment(segment) {
                return None;
            }
            Some(HlsPublishedLiveOriginBaseline {
                evidence_proxy_seq: *proxy_seq,
                origin_epoch: segment.origin_key.origin_epoch,
                rendered_at_ms: rendered.rendered_at_ms,
            })
        });
        let Some(commit_identity) = self.next_manifest_commit_identity(rendered.rendered_at_ms) else {
            return RenderedManifestStoreOutcome::Rejected(
                RenderedManifestStoreRejectReason::CommitGenerationExhausted,
            );
        };
        self.publish_live_recovery_discontinuities(&rendered.segment_proxy_seqs);
        self.longest_rendered_playlist_duration_ms =
            self.longest_rendered_playlist_duration_ms.max(rendered.playlist_duration_ms);
        for proxy_seq in &rendered.segment_proxy_seqs {
            if let Some(segment) = self.segments.get_mut(proxy_seq) {
                segment.last_rendered_at_ms = Some(rendered.rendered_at_ms);
            }
        }
        for (proxy_seq, identity) in rendered_identities {
            self.published_resource_history.record(identity, proxy_seq);
        }
        if self.published_live_origin_baseline.is_none() && published_origin_evidence.is_some() {
            self.published_live_origin_baseline = published_origin_evidence;
            // The first render can include queued segments that still need ordered requests.
            self.startup_segment_order_tail = self.segment_prefetch_queue.last_render_window_proxy_seq();
        }
        self.record_normal_manifest_commit_identity(commit_identity);
        self.last_rendered_manifest = Some(rendered);
        RenderedManifestStoreOutcome::Stored
    }

    // Each distinct recovery contributes to the segment number. If a window
    // skips several, only its last transition can be attached to the head.
    fn project_recovery_at_head(&self, first: u64, previous: Option<HlsPublishedWindow>) -> bool {
        let Some((&boundary, published)) = self.live_recovery_discontinuities.range(..first).next_back() else {
            return false;
        };
        // A tag already at the selected head belongs to that sequence. A
        // recovery at the same sequence is stored as part of that tag.
        let independent_head_tag = self.segments.get(&first).is_some_and(|entry| entry.discontinuity_before);
        !independent_head_tag
            && previous.map_or_else(
                || published.is_none_or(|head| head >= first),
                |previous| (previous.first == first && previous.tagged_head) || previous.last < boundary,
            )
    }

    pub(crate) fn rendered_discontinuity_before(
        &self,
        first: u64,
        entry: &SegmentEntry,
        previous: Option<HlsPublishedWindow>,
    ) -> bool {
        // The selected window may begin after the accepted origin head.
        entry.discontinuity_before || (entry.proxy_seq == first && self.project_recovery_at_head(first, previous))
    }

    pub(crate) fn rendered_discontinuity_sequence(&self, first: u64, previous: Option<HlsPublishedWindow>) -> u64 {
        self.discontinuity_sequence
            .saturating_add(hidden_discontinuities_before(self, first))
            .saturating_sub(u64::from(self.project_recovery_at_head(first, previous)))
    }

    pub(crate) fn publish_live_recovery_discontinuities(&mut self, visible: &[u64]) {
        let Some(&first) = visible.first() else { return };
        self.live_discontinuity_published_through =
            self.live_discontinuity_published_through.max(visible.last().copied());
        if let Some(through) = self.live_discontinuity_published_through {
            self.retired_live_mappings.retain(|_, last| *last > through);
        }
        for (_, published) in self.live_recovery_discontinuities.range_mut(..first) {
            *published = Some(published.map_or(first, |head| head.min(first)));
        }
        for seq in visible {
            if let Some(published) = self.live_recovery_discontinuities.get_mut(seq) {
                *published = Some(*seq);
            }
        }
    }
}

pub fn renderer_candidate_window_proxy_seqs(session: &HlsSession) -> Vec<u64> {
    if session.uses_fast_start_live_window() {
        return session
            .required_live_windows()
            .into_iter()
            .flat_map(|plan| plan.visible)
            .chain(session.live_window_demands.values().flat_map(|demand| demand.published.iter().copied()))
            .collect();
    }
    if session.startup.is_some() {
        let Some((head, tail)) =
            session.publishable_origin_head_proxy_seq.zip(session.publishable_origin_tail_proxy_seq)
        else {
            return Vec::new();
        };
        return (head..=tail.min(head.saturating_add(super::startup_policy::STARTUP_WINDOW_SUCCESSORS)))
            .take_while(|seq| {
                session.segments.get(seq).is_some_and(|entry| {
                    entry.origin_fetch_ref.is_some()
                        && !matches!(
                            entry.status,
                            SegmentCacheStatus::FailedPermanent { .. } | SegmentCacheStatus::Expired
                        )
                })
            })
            .collect();
    }
    for render_gap_segments in 0..=session.render_policy.initial_render_gap_segments {
        if let Some(window) = select_window(session, render_gap_segments, RenderTailPolicy::RequirePlannedTail) {
            return window;
        }
    }
    Vec::new()
}

fn is_renderable(entry: &SegmentEntry, session: &HlsSession) -> bool {
    if let Some(startup) = session.startup.as_ref().filter(|_| {
        entry.is_fast_start_eligible()
            || session.group_member_start_proxy_seq.is_none_or(|start| entry.proxy_seq < start)
    }) {
        return entry.is_fast_start_eligible()
            && startup
                .revisions
                .get(&entry.proxy_seq)
                .is_some_and(|revision| revision.revision().is_window_publishable(startup.config.mode));
    }
    match entry.status {
        SegmentCacheStatus::Ready { .. } => {}
        SegmentCacheStatus::Queued { .. } | SegmentCacheStatus::Fetching { .. } => {
            if entry.origin_fetch_ref.is_none() {
                return false;
            }
        }
        SegmentCacheStatus::Discovered
        | SegmentCacheStatus::CapacityDeferred { .. }
        | SegmentCacheStatus::FailedRetryable { .. }
        | SegmentCacheStatus::FailedPermanent { .. }
        | SegmentCacheStatus::Expired => {
            return false;
        }
    }
    let Some(map_ref) = entry.map_ref else {
        return true;
    };
    session.maps.get(&map_ref).is_some_and(|map| matches!(map.status, MapCacheStatus::Ready { .. }))
}

fn select_window(session: &HlsSession, render_gap_segments: usize, tail_policy: RenderTailPolicy) -> Option<Vec<u64>> {
    let head_seq = session.publishable_origin_head_proxy_seq?;
    let minimum_visible_segments = if session.allows_short_origin_windows() { 1 } else { MIN_VISIBLE_SEGMENTS };
    if let Some(startup) = session.startup.as_ref().filter(|_| {
        session.group_member_start_proxy_seq.is_none_or(|start| head_seq < start)
            || session.segments.get(&head_seq).is_some_and(SegmentEntry::is_fast_start_eligible)
    }) {
        // The head always needs body bytes; only optimistic successors may still be waiting for theirs.
        if !startup.revisions.get(&head_seq).is_some_and(|revision| revision.revision().is_publishable()) {
            return None;
        }
        let planned_tail =
            session.publishable_origin_tail_proxy_seq?.checked_sub(u64::try_from(render_gap_segments).ok()?)?;
        let wanted = if session.published_live_origin_baseline.is_none() {
            MIN_VISIBLE_SEGMENTS
        } else {
            TARGET_VISIBLE_SEGMENTS
        };
        let mut window = Vec::new();
        for seq in head_seq..=planned_tail.min(head_seq.saturating_add(u64::try_from(wanted).ok()?.saturating_sub(1))) {
            if !session.segments.get(&seq).is_some_and(|entry| is_renderable(entry, session)) {
                break;
            }
            window.push(seq);
        }
        if window.len() < minimum_visible_segments {
            return None;
        }
        // Fast-start state lives as long as the session, so live slides render here too.
        retain_published_predecessors(session, &mut window);
        return Some(window);
    }
    let requested_tail_seq =
        session.publishable_origin_tail_proxy_seq?.checked_sub(u64::try_from(render_gap_segments).ok()?)?;
    if requested_tail_seq < head_seq {
        return None;
    }

    let tail_seq = match tail_policy {
        RenderTailPolicy::RequirePlannedTail => requested_tail_seq,
        RenderTailPolicy::TruncateUnavailableTail => {
            // Capacity and permanent failures truncate only the unpublished
            // tail. Never skip a hole and never regress media sequence.
            let mut tail_seq = None;
            for current in head_seq..=requested_tail_seq {
                let entry = session.segments.get(&current)?;
                if !is_renderable(entry, session) {
                    if matches!(
                        entry.status,
                        SegmentCacheStatus::CapacityDeferred { .. }
                            | SegmentCacheStatus::FailedRetryable { .. }
                            | SegmentCacheStatus::FailedPermanent { .. }
                            | SegmentCacheStatus::Expired
                    ) {
                        break;
                    }
                    return None;
                }
                tail_seq = Some(current);
            }
            tail_seq?
        }
    };

    let current_origin_window_len = tail_seq.saturating_sub(head_seq).saturating_add(1);
    let target_window_len = current_origin_window_len.min(u64::try_from(TARGET_VISIBLE_SEGMENTS).ok()?);
    let start_seq = tail_seq.saturating_add(1).saturating_sub(target_window_len).max(head_seq);
    let mut window = Vec::with_capacity(TARGET_VISIBLE_SEGMENTS);
    let mut not_ready_count = 0_usize;

    for current in start_seq..=tail_seq {
        let entry = session.segments.get(&current)?;
        if !is_renderable(entry, session) {
            return None;
        }
        if !matches!(entry.status, SegmentCacheStatus::Ready { .. }) {
            not_ready_count = not_ready_count.saturating_add(1);
            if not_ready_count > session.render_policy.max_not_ready_render_segments {
                return None;
            }
        }
        window.push(current);
    }

    retain_published_predecessors(session, &mut window);
    if window.len() < minimum_visible_segments {
        return None;
    }
    // Lease admission enforces the READY-duration reserve. The renderer only
    // selects a structurally renderable window and must not duplicate that
    // policy from its already-selected duration.
    Some(window)
}

/// RFC 8216 section 6.2.2: removing segments must not shrink a live playlist below three target
/// durations. After an idle gap one origin refresh can move the head several segments, so a client
/// that started on the previous window would find its next segment gone and skip media. Keep the
/// contiguous, still-cached predecessors instead, never reaching before the previously published head.
fn retain_published_predecessors(session: &HlsSession, window: &mut Vec<u64>) {
    let Some(previous_first_proxy_seq) =
        session.last_rendered_manifest.as_ref().map(|rendered| rendered.first_proxy_seq)
    else {
        return;
    };
    retain_published_predecessors_from(session, window, previous_first_proxy_seq);
}

pub(crate) fn retain_published_predecessors_from(
    session: &HlsSession,
    window: &mut Vec<u64>,
    previous_first_proxy_seq: u64,
) {
    let Some(&first_proxy_seq) = window.first() else {
        return;
    };
    let min_duration_ms = u64::from(resolve_target_duration(session, window)).saturating_mul(3_000);
    let mut duration_ms = playlist_duration_ms(session, window);
    let mut start_proxy_seq = first_proxy_seq;
    let mut visible_segments = window.len();
    while duration_ms < min_duration_ms && visible_segments < TARGET_VISIBLE_SEGMENTS {
        let Some(candidate) = start_proxy_seq.checked_sub(1).filter(|seq| *seq >= previous_first_proxy_seq) else {
            break;
        };
        let Some(entry) = session.segments.get(&candidate) else {
            break;
        };
        if !matches!(entry.status, SegmentCacheStatus::Ready { .. })
            || is_local_provisioning_segment(entry)
            || !is_renderable(entry, session)
        {
            break;
        }
        duration_ms = duration_ms.saturating_add(entry.duration_ms);
        start_proxy_seq = candidate;
        visible_segments += 1;
    }
    if start_proxy_seq < first_proxy_seq {
        window.splice(0..0, start_proxy_seq..first_proxy_seq);
    }
}

fn render_window(
    session: &HlsSession,
    window: &[u64],
    render_gap_segments: usize,
    rendered_at_ms: u64,
    previous_window: Option<HlsPublishedWindow>,
) -> Result<RenderedManifest, RenderError> {
    let Some((&first_proxy_seq, &last_proxy_seq)) = window.first().zip(window.last()) else {
        return Err(RenderError::InvalidState);
    };
    if let Some(start) = session
        .group_member_start_proxy_seq
        .filter(|start| last_proxy_seq >= *start && previous_window.is_none_or(|previous| previous.last < *start))
    {
        let first_new = first_proxy_seq.max(start);
        let ready = session.ready_timeline_snapshot(first_new, rendered_at_ms);
        let new_segments = &window[window.partition_point(|seq| *seq < start)..];
        if ready.units.len() < new_segments.len()
            || !new_segments.iter().zip(&*ready.units).all(|(seq, unit)| {
                unit.proxy_seq == *seq
                    && unit.state == crate::media_reserve::HlsReadyMediaState::Ready
                    && unit.required_map_ready
                    && unit.required_key_ready
            })
        {
            return Err(RenderError::NoRenderableWindow);
        }
    }
    let playlist_duration_ms = playlist_duration_ms(session, window);
    let target_duration = resolve_target_duration(session, window);
    let hls_version = resolve_hls_version(session, window);
    let discontinuity_sequence = session.rendered_discontinuity_sequence(first_proxy_seq, previous_window);
    let mut body = String::new();

    body.push_str("#EXTM3U\n");
    writeln!(body, "#EXT-X-VERSION:{hls_version}").map_err(|_| RenderError::InvalidState)?;
    if session.independent_segments || window_contains_provisioning_segment(session, window) {
        body.push_str("#EXT-X-INDEPENDENT-SEGMENTS\n");
    }
    writeln!(body, "#EXT-X-TARGETDURATION:{target_duration}").map_err(|_| RenderError::InvalidState)?;
    writeln!(body, "#EXT-X-MEDIA-SEQUENCE:{first_proxy_seq}").map_err(|_| RenderError::InvalidState)?;
    writeln!(body, "#EXT-X-DISCONTINUITY-SEQUENCE:{discontinuity_sequence}").map_err(|_| RenderError::InvalidState)?;

    let mut current_map_ref = None;
    let mut current_encryption: Option<HlsSegmentEncryption> = None;
    let contains_provisioning = window_contains_provisioning_segment(session, window);
    let mut media_units_rendered = 0_usize;
    for proxy_seq in window {
        let entry = session.segments.get(proxy_seq).ok_or(RenderError::InvalidState)?;
        if contains_provisioning && media_units_rendered == 0 {
            append_manifest_block_separator(&mut body);
        }
        for daterange in &entry.daterange_tags_before {
            body.push_str(daterange);
            body.push('\n');
        }
        if let Some(program_date_time) = &entry.program_date_time {
            writeln!(body, "#EXT-X-PROGRAM-DATE-TIME:{program_date_time}").map_err(|_| RenderError::InvalidState)?;
        }
        if session.rendered_discontinuity_before(first_proxy_seq, entry, previous_window) {
            if contains_provisioning {
                append_manifest_block_separator(&mut body);
            }
            body.push_str("#EXT-X-DISCONTINUITY\n");
        }
        if is_hls_provisioning_gap_segment(entry) {
            if contains_provisioning {
                append_manifest_block_separator(&mut body);
            }
            body.push_str("#EXT-X-GAP\n");
        }
        if entry.map_ref != current_map_ref {
            if let Some(map_ref) = entry.map_ref {
                let map = session.maps.get(&map_ref).ok_or(RenderError::InvalidState)?;
                writeln!(
                    body,
                    "#EXT-X-MAP:URI=\"/hls/shared/live/{}/{}/map/{:06}.{}\"",
                    session.proxy_session_id.0, HLS_ACCESS_LEASE_ID_PLACEHOLDER, map.proxy_map_id.0, map.proxy_file_ext
                )
                .map_err(|_| RenderError::InvalidState)?;
            }
            current_map_ref = entry.map_ref;
        }
        if entry.encryption != current_encryption {
            render_encryption_transition(&mut body, session, entry.encryption.as_ref())?;
            current_encryption.clone_from(&entry.encryption);
        }
        writeln!(body, "#EXTINF:{},", format_hls_duration_ms(entry.duration_ms))
            .map_err(|_| RenderError::InvalidState)?;
        if is_local_provisioning_segment(entry) {
            writeln!(
                body,
                "/hls/shared/live/{}/{}/{:06}.{}?pseq={}",
                proxy_session_id(session),
                HLS_ACCESS_LEASE_ID_PLACEHOLDER,
                entry.proxy_seq,
                entry.proxy_file_ext,
                entry.proxy_seq
            )
            .map_err(|_| RenderError::InvalidState)?;
        } else {
            writeln!(
                body,
                "/hls/shared/live/{}/{}/{:06}.{}",
                proxy_session_id(session),
                HLS_ACCESS_LEASE_ID_PLACEHOLDER,
                entry.proxy_seq,
                entry.proxy_file_ext
            )
            .map_err(|_| RenderError::InvalidState)?;
        }
        media_units_rendered = media_units_rendered.saturating_add(1);
    }

    Ok(RenderedManifest {
        body,
        first_proxy_seq,
        last_proxy_seq,
        discontinuity_sequence,
        target_duration_ms: u64::from(target_duration).saturating_mul(1_000),
        playlist_duration_ms,
        valid_until_ms: rendered_at_ms.saturating_add(playlist_duration_ms),
        render_gap_segments,
        rendered_at_ms,
        segment_proxy_seqs: window.to_vec(),
    })
}

fn render_encryption_transition(
    body: &mut String,
    session: &HlsSession,
    encryption: Option<&HlsSegmentEncryption>,
) -> Result<(), RenderError> {
    let Some(encryption) = encryption else {
        body.push_str("#EXT-X-KEY:METHOD=NONE\n");
        return Ok(());
    };
    write!(
        body,
        "#EXT-X-KEY:METHOD=AES-128,URI=\"/hls/shared/live/{}/{}/r/{}.{}\"",
        proxy_session_id(session),
        HLS_ACCESS_LEASE_ID_PLACEHOLDER,
        encryption.resource_id.0,
        encryption.resource_extension
    )
    .map_err(|_| RenderError::InvalidState)?;
    if let Some(iv) = &encryption.iv {
        write!(body, ",IV={iv}").map_err(|_| RenderError::InvalidState)?;
    }
    if let Some(key_format) = &encryption.key_format {
        write!(body, ",KEYFORMAT=\"{key_format}\"").map_err(|_| RenderError::InvalidState)?;
    }
    if let Some(versions) = &encryption.key_format_versions {
        write!(body, ",KEYFORMATVERSIONS=\"{versions}\"").map_err(|_| RenderError::InvalidState)?;
    }
    body.push('\n');
    Ok(())
}

fn append_manifest_block_separator(body: &mut String) {
    if body.is_empty() || body.ends_with("\n\n") {
        return;
    }
    if !body.ends_with('\n') {
        body.push('\n');
    }
    body.push('\n');
}

fn proxy_session_id(session: &HlsSession) -> &str {
    let ProxySessionId(value) = &session.proxy_session_id;
    value
}

fn resolve_hls_version(session: &HlsSession, window: &[u64]) -> u16 {
    if window_contains_provisioning_segment(session, window) {
        return 7;
    }
    let needs_map_version =
        window.iter().filter_map(|proxy_seq| session.segments.get(proxy_seq)).any(|entry| entry.map_ref.is_some());
    let needs_key_format_version = window
        .iter()
        .filter_map(|proxy_seq| session.segments.get(proxy_seq))
        .filter_map(|entry| entry.encryption.as_ref())
        .any(|encryption| encryption.key_format.is_some() || encryption.key_format_versions.is_some());
    let feature_version = if needs_map_version {
        6
    } else if needs_key_format_version {
        5
    } else {
        3
    };
    session.origin_version.unwrap_or(3).max(feature_version)
}

fn resolve_target_duration(session: &HlsSession, window: &[u64]) -> u32 {
    if window_contains_provisioning_segment(session, window) && !window_contains_origin_segment(session, window) {
        return HLS_PROVISIONING_TARGET_DURATION_SECS;
    }
    session.target_duration.unwrap_or_else(|| {
        window
            .iter()
            .filter_map(|proxy_seq| session.segments.get(proxy_seq))
            .map(|entry| hls_target_duration_secs(entry.duration_ms))
            .max()
            .unwrap_or(1)
            .max(1)
            .try_into()
            .unwrap_or(u32::MAX)
    })
}

fn window_contains_provisioning_segment(session: &HlsSession, window: &[u64]) -> bool {
    window.iter().filter_map(|proxy_seq| session.segments.get(proxy_seq)).any(is_local_provisioning_segment)
}

fn window_contains_origin_segment(session: &HlsSession, window: &[u64]) -> bool {
    window
        .iter()
        .filter_map(|proxy_seq| session.segments.get(proxy_seq))
        .any(|entry| !is_local_provisioning_segment(entry))
}

fn is_local_provisioning_segment(entry: &SegmentEntry) -> bool {
    is_hls_provisioning_segment(entry) || is_hls_provisioning_gap_segment(entry)
}

fn playlist_duration_ms(session: &HlsSession, window: &[u64]) -> u64 {
    window
        .iter()
        .filter_map(|proxy_seq| session.segments.get(proxy_seq))
        .fold(0_u64, |duration_ms, entry| duration_ms.saturating_add(entry.duration_ms))
}

fn hidden_discontinuities_before(session: &HlsSession, first_proxy_seq: u64) -> u64 {
    let retained = session
        .segments
        .range(..first_proxy_seq)
        .filter(|(seq, entry)| entry.discontinuity_before && !session.live_recovery_discontinuities.contains_key(seq))
        .count();
    let recovery = session.live_recovery_discontinuities.range(..first_proxy_seq).count();
    retained
        .saturating_add(recovery)
        .saturating_add(session.evicted_discontinuities.range(..first_proxy_seq).count())
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::{
        HlsManifestRenderer, HlsPublishedLiveOriginBaseline, RenderError, RenderPolicy, RenderedManifest,
        RenderedManifestStoreOutcome, RenderedManifestStoreRejectReason,
    };
    use crate::{
        manifest_origin_binding::HlsManifestOriginBinding,
        timeline::{HLS_PROVISIONING_GAP_ORIGIN_EPOCH, HLS_PROVISIONING_ORIGIN_EPOCH},
        HlsSession, HlsSessionKey, MapCacheStatus, SegmentCacheStatus, SegmentFetchPriority,
    };
    use shared::model::HlsStripMode;
    use tuliprox_core::model::StripConfig;
    use tuliprox_parser::hls::origin_manifest::{parse_origin_media_manifest, OriginManifestParseOutcome};
    use url::Url;

    const BASE_URL: &str = "http://origin.example.com/live/final/index.m3u8";

    fn session() -> HlsSession { HlsSession::new(HlsSessionKey::new(1, "12345"), b"secret", 0) }

    fn normal_manifest(body: &str) -> tuliprox_parser::hls::origin_manifest::ParsedOriginManifest {
        match parse_origin_media_manifest(body, BASE_URL) {
            OriginManifestParseOutcome::Normal(manifest) => manifest,
            OriginManifestParseOutcome::TransientPassthrough { reason } => {
                panic!("expected normal manifest: {reason:?}")
            }
        }
    }

    fn six_segment_manifest() -> tuliprox_parser::hls::origin_manifest::ParsedOriginManifest {
        normal_manifest(
            "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:120\n#EXTINF:4.0,\norigin-name-120.ts\n#EXTINF:4.0,\norigin-name-121.ts\n#EXTINF:4.0,\norigin-name-122.ts\n#EXTINF:4.0,\norigin-name-123.ts\n#EXTINF:4.0,\norigin-name-124.ts\n#EXTINF:4.0,\norigin-name-125.ts\n",
        )
    }

    fn optimistic_startup_session(mode: shared::model::HlsStartupMode) -> std::io::Result<HlsSession> {
        let mut session = session();
        session
            .apply_origin_manifest(&normal_manifest(
                "#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:2,\n0.ts\n#EXTINF:2,\n1.ts\n#EXTINF:2,\n2.ts\n",
            ))
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        install_startup(&mut session, mode)?;
        Ok(session)
    }

    fn install_startup(session: &mut HlsSession, mode: shared::model::HlsStartupMode) -> std::io::Result<()> {
        let config = tuliprox_core::model::HlsStartupConfig { mode, ..Default::default() };
        let worker = std::sync::Arc::new(crate::HlsSegmentWorkerPool::default());
        let store = std::sync::Arc::new(crate::SegmentRevisionStore::default());
        let mut revisions = std::collections::BTreeMap::new();
        for seq in session.segments.keys().copied().collect::<Vec<_>>() {
            revisions
                .insert(seq, store.create(session.proxy_session_id.clone(), seq, crate::SegmentRevisionKind::Raw)?);
        }
        session.startup = Some(crate::HlsSessionStartup {
            config: config.clone(),
            worker: std::sync::Arc::downgrade(&worker),
            access_leases: std::sync::Arc::new(tokio::sync::RwLock::new(crate::HlsAccessLeaseStore::default())),
            revisions,
            policy_fixed: true,
            store,
            budget: crate::ProgressiveBudgetManager::new(config),
        });
        Ok(())
    }

    fn startup_revision(session: &HlsSession, offset: usize) -> std::io::Result<crate::SegmentRevisionGuard> {
        session
            .startup
            .as_ref()
            .and_then(|startup| startup.revisions.values().nth(offset).cloned())
            .ok_or_else(|| std::io::Error::other("revision"))
    }

    #[test]
    fn optimistic_startup_publishes_pending_successors_once_the_head_has_bytes() -> std::io::Result<()> {
        for (mode, renders) in
            [(shared::model::HlsStartupMode::Optimistic, true), (shared::model::HlsStartupMode::Progressive, false)]
        {
            let mut session = optimistic_startup_session(mode)?;
            assert!(matches!(session.render_and_store_manifest(10), Err(RenderError::NoRenderableWindow)));
            startup_revision(&session, 0)?.revision().prefix_available.store(1, std::sync::atomic::Ordering::Release);
            let rendered = session.render_and_store_manifest(20);
            assert_eq!(rendered.is_ok(), renders, "mode={mode}");
            if let Ok(rendered) = rendered {
                assert_eq!(rendered.segment_proxy_seqs.len(), 3);
            }
        }
        Ok(())
    }

    #[test]
    fn optimistic_startup_window_stops_before_a_failed_successor() -> std::io::Result<()> {
        let mut session = optimistic_startup_session(shared::model::HlsStartupMode::Optimistic)?;
        startup_revision(&session, 0)?.revision().prefix_available.store(1, std::sync::atomic::Ordering::Release);
        startup_revision(&session, 1)?.revision().fail();
        assert!(matches!(session.render_and_store_manifest(10), Err(RenderError::NoRenderableWindow)));
        Ok(())
    }

    fn mark_all_segments_ready(session: &mut HlsSession) {
        for segment in session.segments.values_mut() {
            segment.status = SegmentCacheStatus::Ready { content_length: 1024, ready_at_ms: 10 };
        }
    }

    fn install_recovery_gate_fields(session: &mut HlsSession) {
        session.origin_control.manifest_origin_binding = Some(
            HlsManifestOriginBinding::new(
                Url::parse("https://origin.example/live/index.m3u8?token=test").expect("binding URL"),
                Some(0),
            )
            .expect("binding"),
        );
        session.origin_control.pinned_host = Some("origin.example".to_string());
        session.origin_seq_highwater = Some(125);
        session.origin_control.record_media_progress(10, 4_000);
    }

    fn rendered_manifest(first_proxy_seq: u64, last_proxy_seq: u64) -> RenderedManifest {
        RenderedManifest {
            body: format!("#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:{first_proxy_seq}\n"),
            first_proxy_seq,
            last_proxy_seq,
            discontinuity_sequence: 0,
            target_duration_ms: 4_000,
            playlist_duration_ms: 4_000,
            valid_until_ms: 4_000,
            render_gap_segments: 0,
            rendered_at_ms: first_proxy_seq,
            segment_proxy_seqs: (first_proxy_seq..=last_proxy_seq).collect(),
        }
    }

    #[test]
    fn rendering_an_empty_internal_window_does_not_panic() {
        let session = session();

        let rendered =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| super::render_window(&session, &[], 0, 0, None)));

        assert!(rendered.is_ok());
    }

    #[test]
    fn renderer_does_not_emit_origin_uri_or_file_name() {
        let mut session = session();
        session.apply_origin_manifest(&six_segment_manifest()).expect("manifest should map");
        mark_all_segments_ready(&mut session);

        let rendered = HlsManifestRenderer::render(&session, 10).expect("manifest should render");

        assert!(!rendered.body.contains("origin.example.com"));
        assert!(!rendered.body.contains("origin-name-120.ts"));
        assert!(rendered.body.contains("/hls/shared/live/"));
    }

    #[test]
    fn renderer_emits_segment_local_key_rotation_and_explicit_clear_transition() {
        let mut manifest = normal_manifest(
            "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:1\n\
             #EXT-X-KEY:METHOD=AES-128,URI=\"origin-a.key\",KEYFORMAT=\"identity\",KEYFORMATVERSIONS=\"1\"\n\
             #EXTINF:4,\n1.ts\n#EXTINF:4,\n2.ts\n\
             #EXT-X-KEY:METHOD=NONE\n#EXTINF:4,\n3.ts\n#EXTINF:4,\n4.ts\n\
             #EXT-X-KEY:METHOD=AES-128,URI=\"origin-b.key\"\n#EXTINF:4,\n5.ts\n#EXTINF:4,\n6.ts\n",
        );
        for encryption in manifest.segments.iter_mut().filter_map(|segment| segment.encryption.as_mut()) {
            encryption.proxy_resource_id = Some(
                if encryption.resolved_origin_uri.ends_with("origin-a.key") { "opaque-a" } else { "opaque-b" }
                    .to_string(),
            );
            encryption.proxy_resource_extension = Some("key".to_string());
        }
        let mut session = session();
        session.apply_origin_manifest(&manifest).expect("encrypted timeline maps");
        mark_all_segments_ready(&mut session);

        let rendered = HlsManifestRenderer::render(&session, 10).expect("manifest renders").body;
        let key_a = rendered.find("/r/opaque-a.key").expect("first key");
        let clear = rendered.find("#EXT-X-KEY:METHOD=NONE").expect("clear boundary");
        let key_b = rendered.find("/r/opaque-b.key").expect("rotated key");

        assert!(key_a < clear && clear < key_b);
        assert!(rendered.contains("#EXT-X-MEDIA-SEQUENCE:0\n"));
        for iv in [
            "0x00000000000000000000000000000001",
            "0x00000000000000000000000000000002",
            "0x00000000000000000000000000000005",
            "0x00000000000000000000000000000006",
        ] {
            assert!(rendered.contains(&format!(",IV={iv}")), "missing materialized IV {iv}");
        }
        assert_eq!(rendered.matches("#EXT-X-KEY:METHOD=AES-128").count(), 4);
        assert!(rendered.contains("#EXT-X-VERSION:5\n"));
        assert!(!rendered.contains("origin-a.key"));
        assert!(!rendered.contains("origin-b.key"));
    }

    #[test]
    fn renderer_emits_six_digit_proxy_sequence_urls() {
        let mut session = session();
        session.apply_origin_manifest(&six_segment_manifest()).expect("manifest should map");
        mark_all_segments_ready(&mut session);

        let rendered = HlsManifestRenderer::render(&session, 10).expect("manifest should render");

        assert!(rendered.body.contains("/000000.ts"));
    }

    #[test]
    fn media_sequence_is_first_visible_proxy_sequence() {
        let mut session = session();
        session.render_policy = RenderPolicy::new(1);
        session.apply_origin_manifest(&six_segment_manifest()).expect("manifest should map");
        mark_all_segments_ready(&mut session);

        let rendered = HlsManifestRenderer::render(&session, 10).expect("manifest should render");

        assert_eq!(rendered.first_proxy_seq, 0);
        assert!(rendered.body.contains("#EXT-X-MEDIA-SEQUENCE:0\n"));
        assert_eq!(rendered.render_gap_segments, 0);
    }

    #[test]
    fn discontinuity_and_sequence_are_rendered_for_visible_window() {
        let mut session = session();
        let first = normal_manifest(
            "#EXTM3U\n#EXT-X-DISCONTINUITY-SEQUENCE:3\n#EXT-X-MEDIA-SEQUENCE:1\n#EXT-X-DISCONTINUITY\n#EXTINF:4.0,\n1.ts\n#EXTINF:4.0,\n2.ts\n#EXT-X-DISCONTINUITY\n#EXTINF:4.0,\n3.ts\n#EXTINF:4.0,\n4.ts\n#EXTINF:4.0,\n5.ts\n#EXTINF:4.0,\n6.ts\n#EXTINF:4.0,\n7.ts\n",
        );
        session.apply_origin_manifest(&first).expect("manifest should map");
        mark_all_segments_ready(&mut session);

        let rendered = HlsManifestRenderer::render(&session, 10).expect("manifest should render");

        assert_eq!(rendered.first_proxy_seq, 1);
        assert_eq!(rendered.discontinuity_sequence, 4);
        assert_eq!(rendered.target_duration_ms, 4_000);
        assert!(rendered.body.contains("#EXT-X-DISCONTINUITY-SEQUENCE:4\n"));
        assert!(rendered.body.contains("#EXT-X-DISCONTINUITY\n#EXTINF:4.000,\n/hls/shared/live/"));
    }

    #[test]
    fn provisioning_handoff_discontinuity_is_rendered_for_first_origin_segment() {
        let mut session = session();
        session.mark_pending_handoff_discontinuity(0);
        let manifest = normal_manifest(
            "#EXTM3U\n#EXT-X-DISCONTINUITY-SEQUENCE:7\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:10\n#EXTINF:4.0,\n10.ts\n#EXTINF:4.0,\n11.ts\n#EXTINF:4.0,\n12.ts\n",
        );
        session.apply_origin_manifest(&manifest).expect("manifest should map");
        mark_all_segments_ready(&mut session);

        let rendered = HlsManifestRenderer::render(&session, 10).expect("manifest should render");

        assert!(rendered.body.contains("#EXT-X-DISCONTINUITY-SEQUENCE:7\n"));
        assert!(rendered.body.contains("#EXT-X-DISCONTINUITY\n#EXTINF:4.000,\n/hls/shared/live/"));
        assert_eq!(rendered.body.matches("#EXT-X-DISCONTINUITY\n").count(), 1);
        assert_eq!(session.pending_handoff_discontinuity_sequence, None);
    }

    #[test]
    fn provisioning_handoff_does_not_duplicate_origin_first_segment_discontinuity() {
        let mut session = session();
        session.mark_pending_handoff_discontinuity(0);
        let manifest = normal_manifest(
            "#EXTM3U\n#EXT-X-DISCONTINUITY-SEQUENCE:7\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:10\n#EXT-X-DISCONTINUITY\n#EXTINF:4.0,\n10.ts\n#EXTINF:4.0,\n11.ts\n#EXTINF:4.0,\n12.ts\n",
        );
        session.apply_origin_manifest(&manifest).expect("manifest should map");
        mark_all_segments_ready(&mut session);

        let rendered = HlsManifestRenderer::render(&session, 10).expect("manifest should render");

        assert!(rendered.body.contains("#EXT-X-DISCONTINUITY-SEQUENCE:7\n"));
        assert_eq!(rendered.body.matches("#EXT-X-DISCONTINUITY\n").count(), 1);
    }

    #[test]
    fn byterange_is_not_rendered() {
        let mut session = session();
        let manifest = normal_manifest(
            "#EXTM3U\n#EXT-X-VERSION:4\n#EXT-X-BYTERANGE:500@1000\n#EXTINF:4.0,\nbig.m4s\n#EXT-X-BYTERANGE:500\n#EXTINF:4.0,\nbig.m4s\n#EXT-X-BYTERANGE:500\n#EXTINF:4.0,\nbig.m4s\n",
        );
        session.apply_origin_manifest(&manifest).expect("manifest should map");
        mark_all_segments_ready(&mut session);

        let rendered = HlsManifestRenderer::render(&session, 10).expect("manifest should render");

        assert!(!rendered.body.contains("#EXT-X-BYTERANGE"));
    }

    #[test]
    fn queued_and_fetching_segments_with_fetch_ref_are_renderable_with_limit() {
        let mut session = session();
        let manifest = normal_manifest(
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:1\n#EXTINF:4.0,\n1.ts\n#EXTINF:4.0,\n2.ts\n#EXTINF:4.0,\n3.ts\n#EXTINF:4.0,\n4.ts\n",
        );
        session.apply_origin_manifest(&manifest).expect("manifest should map");
        session.segments.get_mut(&0).expect("segment").status =
            SegmentCacheStatus::Ready { content_length: 100, ready_at_ms: 1 };
        session.segments.get_mut(&1).expect("segment").status =
            SegmentCacheStatus::Ready { content_length: 100, ready_at_ms: 1 };
        session.segments.get_mut(&2).expect("segment").status =
            SegmentCacheStatus::Queued { priority: SegmentFetchPriority::RenderWindow, queued_at_ms: 1 };
        session.segments.get_mut(&3).expect("segment").status =
            SegmentCacheStatus::Fetching { priority: SegmentFetchPriority::RenderWindow, started_at_ms: 1 };

        let rendered = HlsManifestRenderer::render(&session, 10).expect("manifest should render");

        assert_eq!(rendered.segment_proxy_seqs, vec![0, 1, 2, 3]);
    }

    #[test]
    fn max_not_ready_render_segments_blocks_too_many_queued_segments() {
        let mut session = session();
        let manifest = normal_manifest(
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:1\n#EXTINF:4.0,\n1.ts\n#EXTINF:4.0,\n2.ts\n#EXTINF:4.0,\n3.ts\n",
        );
        session.apply_origin_manifest(&manifest).expect("manifest should map");
        for segment in session.segments.values_mut() {
            segment.status =
                SegmentCacheStatus::Queued { priority: SegmentFetchPriority::RenderWindow, queued_at_ms: 1 };
        }

        assert_eq!(HlsManifestRenderer::render(&session, 10), Err(RenderError::NoRenderableWindow));
    }

    #[test]
    fn map_not_ready_prevents_rendering_affected_segments() {
        let mut session = session();
        let manifest = normal_manifest(
            "#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4.0,\n1.m4s\n#EXTINF:4.0,\n2.m4s\n#EXTINF:4.0,\n3.m4s\n",
        );
        session.apply_origin_manifest(&manifest).expect("manifest should map");
        mark_all_segments_ready(&mut session);

        assert_eq!(HlsManifestRenderer::render(&session, 10), Err(RenderError::NoRenderableWindow));
    }

    #[test]
    fn current_origin_manifest_head_blocks_suffix_fallback() {
        let mut session = session();
        session.apply_origin_manifest(&six_segment_manifest()).expect("manifest should map");
        mark_all_segments_ready(&mut session);
        session.segments.get_mut(&0).expect("segment 0").status = SegmentCacheStatus::Discovered;
        session.segments.get_mut(&1).expect("segment 1").status = SegmentCacheStatus::Discovered;

        assert_eq!(HlsManifestRenderer::render(&session, 10), Err(RenderError::NoRenderableWindow));
    }

    #[test]
    fn first_group_handoff_window_waits_for_the_new_members_map_and_key() {
        let source_a = crate::HlsOriginSource::new(
            1,
            std::sync::Arc::from("member-a"),
            "411",
            crate::origin::HlsOriginSourceKind::DirectMediaPlaylist,
        );
        let mut session =
            HlsSession::new_with_origin_source(crate::HlsSessionKey::for_group(1, 1), source_a, b"group", 0);
        session.apply_origin_manifest(&six_segment_manifest()).unwrap();
        mark_all_segments_ready(&mut session);
        let previous = session.render_and_store_manifest(10).unwrap();
        let start = session.proxy_next_seq.unwrap();

        let source_b = crate::HlsOriginSource::new(
            2,
            std::sync::Arc::from("member-b"),
            "722",
            crate::origin::HlsOriginSourceKind::DirectMediaPlaylist,
        );
        let binding = crate::HlsOriginAccountBinding::rebound(
            std::sync::Arc::from("member-b"),
            std::sync::Arc::from("account-b"),
            crate::origin::build_hls_origin_session_owner(&session.proxy_session_id),
            1,
            20,
        );
        session.handoff_origin_member(source_b, binding);
        let mut origin = normal_manifest(
            r#"#EXTM3U
#EXT-X-VERSION:6
#EXT-X-TARGETDURATION:4
#EXT-X-MEDIA-SEQUENCE:9000
#EXT-X-KEY:METHOD=AES-128,URI="new.key",IV=0x00000000000000000000000000000001
#EXT-X-MAP:URI="new-init.mp4"
#EXTINF:4.0,
9000.m4s
#EXTINF:4.0,
9001.m4s
#EXTINF:4.0,
9002.m4s
"#,
        );
        let mut key = crate::TransientResourceRef::new(
            crate::TransientResourceKind::Key,
            "http://member-b.example/live/new.key",
            b"group",
            20,
            100,
            Some("key".to_string()),
        );
        key.scope_to_origin_account(session.transient.origin_account_generation(), b"group");
        for encryption in origin.segments.iter_mut().filter_map(|segment| segment.encryption.as_mut()) {
            encryption.proxy_resource_id = Some(key.id.0.clone());
            encryption.proxy_resource_extension = Some("key".to_string());
        }
        session.apply_origin_manifest(&origin).unwrap();
        mark_all_segments_ready(&mut session);
        assert_eq!(session.render_and_store_manifest(25), Err(RenderError::NoRenderableWindow));
        let map_id = session.segments[&start].map_ref.expect("B's init map");
        session.maps.get_mut(&map_id).unwrap().status = MapCacheStatus::Ready { content_length: 128, ready_at_ms: 25 };
        assert_eq!(session.render_and_store_manifest(26), Err(RenderError::NoRenderableWindow));
        assert_eq!(session.last_rendered_manifest.as_ref(), Some(&previous));

        session.transient.upsert_resources([key.clone()]);
        let crate::TransientObjectFetchDecision::Fetch(token) =
            session.transient.begin_object_fetch(&session.proxy_session_id, &key, "key", 26, 50)
        else {
            panic!("new member key starts a fetch");
        };
        assert!(session.transient.mark_object_ready_if_current(&token, "application/octet-stream".into(), 16, 27, 120));
        let rendered = session.render_and_store_manifest(28).expect("B has READY media and dependencies");
        assert_eq!(rendered.first_proxy_seq, start);
        assert_eq!(rendered.body.matches("#EXT-X-DISCONTINUITY\n").count(), 1);
        assert!(rendered.body.contains(&format!("/r/{}.key", key.id.0)));
        assert!(rendered.body.contains(&format!("/map/{:06}.mp4", map_id.0)));
        assert!(matches!(session.segments[&previous.first_proxy_seq].status, SegmentCacheStatus::Ready { .. }));
    }

    #[test]
    fn map_ready_renders_only_proxy_map_uri() {
        let mut session = session();
        let manifest = normal_manifest(
            "#EXTM3U\n#EXT-X-MAP:URI=\"origin-init-name.mp4\"\n#EXTINF:4.0,\n1.m4s\n#EXTINF:4.0,\n2.m4s\n#EXTINF:4.0,\n3.m4s\n",
        );
        session.apply_origin_manifest(&manifest).expect("manifest should map");
        mark_all_segments_ready(&mut session);
        for map in session.maps.values_mut() {
            map.status = MapCacheStatus::Ready { content_length: 128, ready_at_ms: 10 };
        }

        let rendered = HlsManifestRenderer::render(&session, 10).expect("manifest should render");

        assert!(rendered.body.contains("/hls/shared/live/"));
        assert!(rendered.body.contains("/map/000000.mp4"));
        assert!(!rendered.body.contains("origin-init-name.mp4"));
    }

    #[test]
    fn map_tag_is_rendered_once_for_unchanged_map_ref() {
        let mut session = session();
        let manifest = normal_manifest(
            "#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4.0,\n1.m4s\n#EXTINF:4.0,\n2.m4s\n#EXTINF:4.0,\n3.m4s\n#EXTINF:4.0,\n4.m4s\n",
        );
        session.apply_origin_manifest(&manifest).expect("manifest should map");
        mark_all_segments_ready(&mut session);
        for map in session.maps.values_mut() {
            map.status = MapCacheStatus::Ready { content_length: 128, ready_at_ms: 10 };
        }

        let rendered = HlsManifestRenderer::render(&session, 10).expect("manifest should render");

        assert_eq!(rendered.body.matches("#EXT-X-MAP").count(), 1);
    }

    #[test]
    fn map_tag_is_rendered_again_when_map_ref_changes() {
        let mut session = session();
        let manifest = normal_manifest(
            "#EXTM3U\n#EXT-X-MAP:URI=\"init-a.mp4\"\n#EXTINF:4.0,\n1.m4s\n#EXTINF:4.0,\n2.m4s\n#EXTINF:4.0,\n3.m4s\n#EXT-X-MAP:URI=\"init-b.mp4\"\n#EXTINF:4.0,\n4.m4s\n#EXTINF:4.0,\n5.m4s\n#EXTINF:4.0,\n6.m4s\n",
        );
        session.apply_origin_manifest(&manifest).expect("manifest should map");
        mark_all_segments_ready(&mut session);
        for map in session.maps.values_mut() {
            map.status = MapCacheStatus::Ready { content_length: 128, ready_at_ms: 10 };
        }

        let rendered = HlsManifestRenderer::render(&session, 10).expect("manifest should render");

        assert_eq!(rendered.body.matches("#EXT-X-MAP").count(), 2);
        assert!(rendered.body.contains("/map/000000.mp4"));
        assert!(rendered.body.contains("/map/000001.mp4"));
    }

    #[test]
    fn renderer_gap_is_relative_to_publishable_tail() {
        let mut session = session();
        session.render_policy = RenderPolicy::new(3);
        let manifest = normal_manifest(
            "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:1\n#EXTINF:4.0,\n1.ts\n#EXTINF:4.0,\n2.ts\n#EXTINF:4.0,\n3.ts\n#EXTINF:4.0,\n4.ts\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4.0,\n5.m4s\n#EXTINF:4.0,\n6.m4s\n#EXTINF:4.0,\n7.m4s\n",
        );
        session.apply_origin_manifest(&manifest).expect("manifest should map");
        mark_all_segments_ready(&mut session);

        let rendered = HlsManifestRenderer::render(&session, 10).expect("older valid window should render");

        assert_eq!(session.publishable_origin_tail_proxy_seq, Some(6));
        assert_eq!(rendered.last_proxy_seq, 3);
        assert_eq!(rendered.render_gap_segments, 3);
        assert_eq!(rendered.segment_proxy_seqs, vec![0, 1, 2, 3]);
    }

    #[test]
    fn store_rendered_manifest_rejects_regressive_media_sequence() {
        let mut session = session();
        session.apply_origin_manifest(&six_segment_manifest()).expect("manifest should map");
        mark_all_segments_ready(&mut session);
        let previous = rendered_manifest(2, 7);
        session.last_rendered_manifest = Some(previous.clone());
        let candidate = HlsManifestRenderer::render(&session, 10).expect("real-origin candidate should render");

        assert_eq!(
            session.store_rendered_manifest(candidate),
            RenderedManifestStoreOutcome::Rejected(RenderedManifestStoreRejectReason::RegressiveMediaSequence {
                previous_first_proxy_seq: 2,
                candidate_first_proxy_seq: 0,
            })
        );
        assert_eq!(session.last_rendered_manifest, Some(previous));
        assert!(session.published_live_origin_baseline.is_none());
    }

    #[test]
    fn store_rendered_manifest_rejects_duplicate_media_identity_at_distinct_proxy_positions() {
        let mut session = session();
        session.apply_origin_manifest(&six_segment_manifest()).expect("manifest should map");
        mark_all_segments_ready(&mut session);
        let duplicated_identity = session.segments.get(&0).expect("first segment").occurrence_identity;
        session.segments.get_mut(&1).expect("second segment").occurrence_identity = duplicated_identity;
        let candidate = HlsManifestRenderer::render(&session, 10).expect("candidate renders before store validation");

        assert_eq!(
            session.store_rendered_manifest(candidate),
            RenderedManifestStoreOutcome::Rejected(RenderedManifestStoreRejectReason::DuplicateMediaResource {
                existing_proxy_seq: 0,
                candidate_proxy_seq: 1,
            })
        );
        assert!(session.last_rendered_manifest.is_none());
        assert!(session.published_live_origin_baseline.is_none());
    }

    #[test]
    fn provisioning_and_gap_only_manifests_do_not_set_live_baseline() {
        for synthetic_epoch in [HLS_PROVISIONING_ORIGIN_EPOCH, HLS_PROVISIONING_GAP_ORIGIN_EPOCH] {
            let mut session = session();
            session.apply_origin_manifest(&six_segment_manifest()).expect("manifest should map");
            mark_all_segments_ready(&mut session);
            for segment in session.segments.values_mut() {
                segment.origin_key.origin_epoch = synthetic_epoch;
            }

            session.render_and_store_manifest(10).expect("synthetic manifest should store");

            assert!(session.last_rendered_manifest.is_some());
            assert!(session.published_live_origin_baseline.is_none());
        }
    }

    #[test]
    fn mixed_provisioning_and_origin_manifest_enables_recovery_binding() {
        let mut session = session();
        session.apply_origin_manifest(&six_segment_manifest()).expect("manifest should map");
        mark_all_segments_ready(&mut session);
        session.segments.get_mut(&0).expect("first segment").origin_key.origin_epoch = HLS_PROVISIONING_ORIGIN_EPOCH;
        install_recovery_gate_fields(&mut session);

        let stored = session.render_and_store_manifest(10).expect("mixed manifest should store");

        assert_eq!(
            session.published_live_origin_baseline,
            Some(HlsPublishedLiveOriginBaseline {
                evidence_proxy_seq: 1,
                origin_epoch: 0,
                rendered_at_ms: stored.rendered_at_ms,
            })
        );
        assert!(session.established_manifest_recovery_binding().is_some());
    }

    #[test]
    fn real_origin_manifest_enables_recovery_binding() {
        let mut session = session();
        session.apply_origin_manifest(&six_segment_manifest()).expect("manifest should map");
        mark_all_segments_ready(&mut session);
        install_recovery_gate_fields(&mut session);

        let stored = session.render_and_store_manifest(10).expect("origin manifest should store");

        assert_eq!(
            session.published_live_origin_baseline,
            Some(HlsPublishedLiveOriginBaseline {
                evidence_proxy_seq: 0,
                origin_epoch: 0,
                rendered_at_ms: stored.rendered_at_ms,
            })
        );
        assert!(session.established_manifest_recovery_binding().is_some());
    }

    #[test]
    fn no_renderable_window_does_not_set_live_baseline() {
        let mut session = session();
        session.apply_origin_manifest(&six_segment_manifest()).expect("manifest should map");
        mark_all_segments_ready(&mut session);
        for segment in session.segments.values_mut() {
            segment.origin_key.origin_epoch = HLS_PROVISIONING_ORIGIN_EPOCH;
        }
        let provisioning = session.render_and_store_manifest(10).expect("provisioning manifest should store");
        for segment in session.segments.values_mut() {
            segment.origin_key.origin_epoch = 1;
            segment.status = SegmentCacheStatus::Discovered;
        }

        assert_eq!(session.render_and_store_manifest(20), Err(RenderError::NoRenderableWindow));
        assert_eq!(session.last_rendered_manifest, Some(provisioning));
        assert!(session.published_live_origin_baseline.is_none());
    }

    #[test]
    fn published_live_baseline_survives_gc_of_evidence_segment() {
        let mut session = session();
        session.apply_origin_manifest(&six_segment_manifest()).expect("manifest should map");
        mark_all_segments_ready(&mut session);
        install_recovery_gate_fields(&mut session);
        session.render_and_store_manifest(10).expect("origin manifest should store");
        let evidence = session.published_live_origin_baseline.expect("live-origin evidence");
        assert_eq!(session.store_rendered_manifest(rendered_manifest(1, 5)), RenderedManifestStoreOutcome::Stored);
        assert_eq!(session.published_live_origin_baseline, Some(evidence));
        assert!(!session
            .last_rendered_manifest
            .as_ref()
            .expect("advanced manifest remains stored")
            .segment_proxy_seqs
            .contains(&evidence.evidence_proxy_seq));

        session.segments.remove(&evidence.evidence_proxy_seq);

        assert_eq!(session.published_live_origin_baseline, Some(evidence));
        assert!(session.established_manifest_recovery_binding().is_some());
    }

    #[test]
    fn render_and_store_manifest_returns_error_on_store_rejection() {
        let mut session = session();
        session.apply_origin_manifest(&six_segment_manifest()).expect("manifest should map");
        mark_all_segments_ready(&mut session);
        session.last_rendered_manifest = Some(rendered_manifest(2, 7));

        assert_eq!(
            session.render_and_store_manifest(10),
            Err(RenderError::StoreRejected(RenderedManifestStoreRejectReason::RegressiveMediaSequence {
                previous_first_proxy_seq: 2,
                candidate_first_proxy_seq: 0,
            }))
        );
        assert_eq!(session.last_rendered_manifest, Some(rendered_manifest(2, 7)));
    }

    #[test]
    fn store_rendered_manifest_accepts_same_or_forward_media_sequence() {
        let mut session = session();
        let first = rendered_manifest(2, 7);
        let same = rendered_manifest(2, 8);
        let forward = rendered_manifest(3, 9);

        assert_eq!(session.store_rendered_manifest(first), RenderedManifestStoreOutcome::Stored);
        assert_eq!(session.store_rendered_manifest(same.clone()), RenderedManifestStoreOutcome::Stored);
        assert_eq!(session.last_rendered_manifest, Some(same));
        assert_eq!(session.store_rendered_manifest(forward.clone()), RenderedManifestStoreOutcome::Stored);
        assert_eq!(session.last_rendered_manifest, Some(forward));
    }

    #[test]
    fn renderer_does_not_exceed_initial_render_gap_to_find_valid_window() {
        let mut session = session();
        session.apply_origin_manifest(&six_segment_manifest()).expect("manifest should map");
        mark_all_segments_ready(&mut session);
        let first = session.render_and_store_manifest(10).expect("initial manifest should render");

        session.render_policy = RenderPolicy::new(2);
        let manifest = normal_manifest(
            "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:126\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4.0,\n126.m4s\n#EXTINF:4.0,\n127.m4s\n#EXTINF:4.0,\n128.m4s\n",
        );
        session.apply_origin_manifest(&manifest).expect("manifest should map");
        mark_all_segments_ready(&mut session);

        assert_eq!(HlsManifestRenderer::render(&session, 20), Err(RenderError::NoRenderableWindow));
        assert_eq!(session.render_and_store_manifest(20), Err(RenderError::NoRenderableWindow));
        assert_eq!(session.last_rendered_manifest, Some(first));
    }

    #[test]
    fn invalid_render_does_not_replace_last_rendered_manifest() {
        let mut session = session();
        session.apply_origin_manifest(&six_segment_manifest()).expect("manifest should map");
        mark_all_segments_ready(&mut session);
        let first = session.render_and_store_manifest(10).expect("manifest should render");
        session.segments.clear();

        assert_eq!(session.render_and_store_manifest(20), Err(RenderError::NoRenderableWindow));
        assert_eq!(session.last_rendered_manifest, Some(first));
    }

    #[test]
    fn render_policy_from_seconds_counts_tail_durations() {
        let policy = RenderPolicy::from_strip_config(
            &StripConfig { mode: HlsStripMode::Seconds, value: 9 },
            &[4_000, 4_000, 4_000, 4_000],
        );

        assert_eq!(policy.initial_render_gap_segments, 3);
    }

    #[test]
    fn hls_startup_policy_renderer_uses_shared_clamped_threshold() {
        let mut session = session();
        let manifest = normal_manifest(
            "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:1\n\
             #EXTINF:2.0,\n1.ts\n#EXTINF:2.0,\n2.ts\n#EXTINF:2.0,\n3.ts\n",
        );
        session.apply_origin_manifest(&manifest).expect("manifest should map");
        mark_all_segments_ready(&mut session);

        let rendered = HlsManifestRenderer::render(&session, 10).expect("short visible playlist should render");

        assert_eq!(rendered.target_duration_ms, 4_000);
        assert_eq!(rendered.playlist_duration_ms, 6_000);
        assert_eq!(
            crate::media_reserve::minimum_hls_startup_window_ms(
                rendered.target_duration_ms,
                rendered.playlist_duration_ms
            ),
            6_000
        );
    }

    #[test]
    fn hls_startup_policy_renderer_uses_advertised_duration_not_larger_origin_window() {
        let mut session = session();
        let manifest = normal_manifest(
            "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:1\n\
             #EXTINF:1.0,\n1.ts\n#EXTINF:1.0,\n2.ts\n#EXTINF:1.0,\n3.ts\n#EXTINF:1.0,\n4.ts\n\
             #EXTINF:1.0,\n5.ts\n#EXTINF:1.0,\n6.ts\n#EXTINF:1.0,\n7.ts\n",
        );
        session.apply_origin_manifest(&manifest).expect("manifest should map");
        mark_all_segments_ready(&mut session);

        let rendered = HlsManifestRenderer::render(&session, 10).expect("six-second advertised playlist should render");

        assert_eq!(rendered.target_duration_ms, 4_000);
        assert_eq!(rendered.playlist_duration_ms, 6_000);
        assert_eq!(rendered.segment_proxy_seqs.len(), 6);
        assert_eq!(
            crate::media_reserve::minimum_hls_startup_window_ms(
                rendered.target_duration_ms,
                rendered.playlist_duration_ms
            ),
            6_000
        );
    }

    fn reentry_manifest(
        media_sequence: u64,
        durations: &[&str],
    ) -> tuliprox_parser::hls::origin_manifest::ParsedOriginManifest {
        use std::fmt::Write as _;

        let mut body = format!("#EXTM3U\n#EXT-X-TARGETDURATION:7\n#EXT-X-MEDIA-SEQUENCE:{media_sequence}\n");
        for (media_seq, duration) in (media_sequence..).zip(durations) {
            writeln!(body, "#EXTINF:{duration},\nsegment-{media_seq}.ts").expect("write to string");
        }
        normal_manifest(&body)
    }

    fn segment_not_found() -> SegmentCacheStatus {
        SegmentCacheStatus::FailedPermanent { failed_at_ms: 25, status: Some(axum::http::StatusCode::NOT_FOUND) }
    }

    fn discontinuity_numbers(rendered: &RenderedManifest) -> Vec<u64> {
        let mut number = rendered.discontinuity_sequence;
        let mut numbers = Vec::new();
        for line in rendered.body.lines() {
            if line == "#EXT-X-DISCONTINUITY" {
                number += 1;
            } else if line.starts_with("/hls/") {
                numbers.push(number);
            }
        }
        numbers
    }

    fn ready_recovery_window(session: &mut HlsSession) {
        mark_all_segments_ready(session);
        if let Some(startup) = &mut session.startup {
            for &seq in session.segments.keys() {
                startup.revisions.entry(seq).or_insert_with(|| {
                    let revision = startup
                        .store
                        .create(session.proxy_session_id.clone(), seq, crate::SegmentRevisionKind::Processed)
                        .unwrap();
                    assert!(revision
                        .revision()
                        .complete(crate::CachedSegmentMetadata { path: "fixture.ts".into(), size: 1024 }));
                    revision
                });
            }
        }
    }

    #[test]
    fn long_recovery_window_publishes_boundary_at_the_visible_head() {
        use shared::model::HlsStartupMode;

        for mode in [HlsStartupMode::Conservative, HlsStartupMode::FirstReady] {
            for (count, gap, origin_tag_at) in
                [(6, 1, None), (8, 1, None), (8, 1, Some(0)), (8, 1, Some(2)), (8, 0, None)]
            {
                let mut session = session();
                session.apply_origin_manifest(&reentry_manifest(100, &["7"; 6])).unwrap();
                if mode == HlsStartupMode::FirstReady {
                    install_startup(&mut session, mode).unwrap();
                    // FirstReady requires complete revisions, including the initial media.
                    session.startup.as_mut().unwrap().revisions.clear();
                }
                ready_recovery_window(&mut session);
                session.render_and_store_manifest(10).unwrap();
                let old = session.render_and_store_manifest(11).unwrap();
                assert_eq!(old.last_proxy_seq, 5);
                assert!(discontinuity_numbers(&old).iter().all(|&number| number == 0));
                let old_binding = session.segments[&5].clone();

                let mut recovery = reentry_manifest(106 + gap, &vec!["7"; count]);
                if let Some(index) = origin_tag_at {
                    recovery.segments[index].discontinuity_before = true;
                }
                session.apply_origin_manifest(&recovery).unwrap();
                ready_recovery_window(&mut session);
                let first = u64::try_from(count).unwrap();
                let prior_boundary = u64::from(origin_tag_at == Some(2));
                let expected_number = gap + prior_boundary;
                let window = session.render_and_store_manifest(20).unwrap();
                let tags = window.body.lines().filter(|line| *line == "#EXT-X-DISCONTINUITY").count();
                eprintln!("mode={mode:?} origin_count={count} gap={gap} origin_tag_at={origin_tag_at:?} first_proxy={first} header={} tags={tags}", window.discontinuity_sequence);
                assert_eq!(window.segment_proxy_seqs, (first..first + 6).collect::<Vec<_>>());
                assert_eq!(window.discontinuity_sequence, prior_boundary);
                assert_eq!(tags, usize::try_from(gap).unwrap());
                assert_eq!(discontinuity_numbers(&window), [expected_number; 6]);
                assert_eq!(session.segments[&5], old_binding);
                assert_eq!(session.render_and_store_manifest(21).unwrap().body, window.body);

                let slide = reentry_manifest(session.segments[&first].origin_key.host_local_sequence + 1, &["7"; 6]);
                session.apply_origin_manifest(&slide).unwrap();
                ready_recovery_window(&mut session);
                let slid = session.render_and_store_manifest(30).unwrap();
                assert_eq!(slid.first_proxy_seq, first + 1);
                assert_eq!(slid.discontinuity_sequence, expected_number);
                assert!(!slid.body.lines().any(|line| line == "#EXT-X-DISCONTINUITY"));
                assert_eq!(discontinuity_numbers(&slid), [expected_number; 6]);
            }
        }
    }

    #[test]
    fn recovery_boundary_merges_origin_tag_on_failed_head() {
        for (failed, tagged, mapped_jump) in
            [(false, false, false), (false, true, false), (true, false, false), (true, true, false), (true, true, true)]
        {
            let mut session = session();
            session.apply_origin_manifest(&reentry_manifest(100, &["7"; 6])).unwrap();
            mark_all_segments_ready(&mut session);
            let old = session.render_and_store_manifest(10).unwrap();
            assert_eq!(old.last_proxy_seq, 5);
            let old_binding = session.segments[&5].clone();

            let origin_head = 106 + u64::from(mapped_jump);
            let mut recovery = reentry_manifest(origin_head, &["7"; 6]);
            recovery.segments[0].discontinuity_before = tagged;
            session.apply_origin_manifest(&recovery).unwrap();
            mark_all_segments_ready(&mut session);
            let first_msn = origin_head + u64::from(failed);
            let first_proxy = 6 + u64::from(failed);
            if failed {
                session.segments.get_mut(&6).unwrap().status = segment_not_found();
                session.apply_origin_manifest(&reentry_manifest(first_msn, &["7"; 6])).unwrap();
                session.segments.get_mut(&12).unwrap().status =
                    SegmentCacheStatus::Ready { content_length: 1024, ready_at_ms: 20 };
            }
            let first = session.render_and_store_manifest(20).unwrap();
            let number = u64::from(failed || tagged);
            assert_eq!(first.first_proxy_seq, first_proxy);
            assert_eq!(first.discontinuity_sequence, 0);
            assert_eq!(discontinuity_numbers(&first), [number; 6]);
            assert_eq!(session.origin_discontinuity_before(&session.segments[&6]), tagged);
            assert_eq!(session.segments[&6].discontinuity_before, tagged && !failed);
            assert_eq!(session.render_and_store_manifest(21).unwrap().body, first.body);

            session.apply_origin_manifest(&reentry_manifest(first_msn + 1, &["7"; 6])).unwrap();
            session.segments.get_mut(&(first_proxy + 6)).unwrap().status =
                SegmentCacheStatus::Ready { content_length: 1024, ready_at_ms: 30 };
            let slid = session.render_and_store_manifest(30).unwrap();
            assert_eq!(slid.first_proxy_seq, first_proxy + 1);
            assert_eq!(slid.discontinuity_sequence, number);
            assert_eq!(discontinuity_numbers(&slid), [number; 6]);
            assert_eq!(session.segments[&5], old_binding);
            eprintln!("origin_tagged_head failed={failed} tagged={tagged} mapped_jump={mapped_jump} first_msn={first_msn} header=0 number={number} slid_header={}", slid.discontinuity_sequence);
        }
    }

    #[test]
    fn recovery_boundary_merges_mapped_jumps_without_playable_media_between_them() {
        for playable in [false, true] {
            for origin_tag in [false, true] {
                let mut session = session();
                session.apply_origin_manifest(&reentry_manifest(100, &["7"; 6])).unwrap();
                mark_all_segments_ready(&mut session);
                let old = session.render_and_store_manifest(10).unwrap();
                let old_binding = session.segments[&5].clone();
                let mut first_jump = reentry_manifest(107, &["7"; 6]);
                first_jump.segments[0].discontinuity_before = origin_tag;
                session.apply_origin_manifest(&first_jump).unwrap();
                for (&seq, entry) in session.segments.range_mut(6..) {
                    entry.status = if playable && seq == 7 {
                        SegmentCacheStatus::Ready { content_length: 1024, ready_at_ms: 20 }
                    } else {
                        segment_not_found()
                    };
                }
                assert!(session.render_and_store_manifest(20).is_err());
                assert_eq!(session.last_rendered_manifest.as_ref(), Some(&old));

                session.apply_origin_manifest(&reentry_manifest(114, &["7"; 6])).unwrap();
                for (_, entry) in session.segments.range_mut(12..) {
                    entry.status = SegmentCacheStatus::Ready { content_length: 1024, ready_at_ms: 30 };
                }
                let first = session.render_and_store_manifest(30).unwrap();
                let header = u64::from(playable);
                assert_eq!(first.segment_proxy_seqs, [12, 13, 14, 15, 16, 17]);
                assert_eq!(first.discontinuity_sequence, header);
                assert_eq!(discontinuity_numbers(&first), [header + 1; 6]);
                let positions = session.live_recovery_discontinuities.keys().copied().collect::<Vec<_>>();
                assert_eq!(positions, if playable { vec![7, 12] } else { vec![12] });
                assert_eq!(session.origin_discontinuity_before(&session.segments[&6]), origin_tag);
                assert_eq!(session.render_and_store_manifest(31).unwrap().body, first.body);

                session.apply_origin_manifest(&reentry_manifest(115, &["7"; 6])).unwrap();
                session.segments.get_mut(&18).unwrap().status =
                    SegmentCacheStatus::Ready { content_length: 1024, ready_at_ms: 40 };
                let slid = session.render_and_store_manifest(40).unwrap();
                assert_eq!(slid.first_proxy_seq, 13);
                assert_eq!(slid.discontinuity_sequence, header + 1);
                assert_eq!(discontinuity_numbers(&slid), [header + 1; 6]);
                assert_eq!(session.segments[&5], old_binding);
                eprintln!("mapped_jumps playable={playable} origin_tag={origin_tag} positions={positions:?} first_header={header} slid_header={}", slid.discontinuity_sequence);
            }
        }
    }

    #[test]
    fn recovery_boundary_keeps_retained_ready_media_between_gaps_after_origin_slide() {
        use shared::model::HlsStartupMode;

        for (early_404, before_slide, late_404) in [
            (false, false, false),
            (false, true, false),
            (true, true, false),
            (true, false, false),
            (false, false, true),
        ] {
            let mut session = session();
            session.apply_origin_manifest(&reentry_manifest(100, &["7"; 6])).unwrap();
            install_startup(&mut session, HlsStartupMode::FirstReady).unwrap();
            session.startup.as_mut().unwrap().revisions.clear();
            ready_recovery_window(&mut session);
            let old = session.render_and_store_manifest(10).unwrap();
            assert_eq!(old.last_proxy_seq, 5);
            let old_binding = session.segments[&5].clone();

            session.apply_origin_manifest(&reentry_manifest(106, &["7"; 12])).unwrap();
            ready_recovery_window(&mut session);
            session.segments.get_mut(&10).unwrap().status = segment_not_found();
            session.segments.get_mut(&6).unwrap().status = if early_404 {
                segment_not_found()
            } else {
                SegmentCacheStatus::Fetching { started_at_ms: 15, priority: SegmentFetchPriority::Prefetch }
            };
            if before_slide {
                session.reconcile_live_discontinuities();
            }
            session.apply_origin_manifest(&reentry_manifest(111, &["7"; 7])).unwrap();
            if late_404 {
                session.segments.get_mut(&6).unwrap().status = segment_not_found();
            }
            let recovered = session.render_and_store_manifest(30).unwrap();
            let header = u64::from(early_404 || late_404);
            assert_eq!(recovered.segment_proxy_seqs, [12, 13, 14, 15, 16, 17]);
            assert_eq!(recovered.discontinuity_sequence, header);
            assert_eq!(discontinuity_numbers(&recovered), [header + 1; 6]);
            assert_eq!(session.segments[&5], old_binding);
            let positions = session.live_recovery_discontinuities.keys().copied().collect::<Vec<_>>();
            assert_eq!(positions, if header == 1 { vec![7, 11] } else { vec![11] });
            assert_eq!(session.render_and_store_manifest(31).unwrap().body, recovered.body);
            eprintln!("retained_ready early_404={early_404} before_slide={before_slide} late_404={late_404} positions={positions:?} header={header} number={}", header + 1);
        }
    }

    #[test]
    fn recovery_boundary_moves_failed_unpublished_handoff_head_once() {
        for (failed_head, origin_tag) in [(false, false), (false, true), (true, false), (true, true)] {
            let mut session = session();
            session.apply_origin_manifest(&reentry_manifest(100, &["7"; 6])).unwrap();
            mark_all_segments_ready(&mut session);
            let old = session.render_and_store_manifest(10).unwrap();
            assert_eq!(old.last_proxy_seq, 5);
            let old_binding = session.segments[&5].clone();
            let new_host = old_binding.origin_key.effective_host_id ^ 1;
            let mut handoff = reentry_manifest(200, &["7"; 6]);
            handoff.segments[0].discontinuity_before = origin_tag;
            let preview = session.preview_origin_handoff_manifest(&handoff, new_host, 0).unwrap();
            session.apply_origin_handoff_manifest_if_preview_matches(&handoff, new_host, 0, &preview).unwrap();
            assert_eq!(session.origin_epoch, 1);
            mark_all_segments_ready(&mut session);

            let (first_msn, first_proxy) = if failed_head {
                session.segments.get_mut(&6).unwrap().status = segment_not_found();
                session.apply_origin_manifest_for_host(&reentry_manifest(201, &["7"; 6]), new_host).unwrap();
                session.segments.get_mut(&12).unwrap().status =
                    SegmentCacheStatus::Ready { content_length: 1024, ready_at_ms: 20 };
                (201, 7)
            } else {
                (200, 6)
            };
            let first = session.render_and_store_manifest(20).unwrap();
            assert_eq!(first.first_proxy_seq, first_proxy);
            assert_eq!(first.discontinuity_sequence, 0);
            assert_eq!(discontinuity_numbers(&first), [1; 6]);
            assert_eq!(session.origin_discontinuity_before(&session.segments[&6]), origin_tag);
            assert_eq!(session.segments[&6].discontinuity_before, !failed_head);
            assert_eq!(session.render_and_store_manifest(21).unwrap().body, first.body);
            session.apply_origin_manifest_for_host(&reentry_manifest(first_msn + 1, &["7"; 6]), new_host).unwrap();
            session.segments.get_mut(&(first_proxy + 6)).unwrap().status =
                SegmentCacheStatus::Ready { content_length: 1024, ready_at_ms: 30 };
            let slid = session.render_and_store_manifest(30).unwrap();
            assert_eq!(slid.first_proxy_seq, first_proxy + 1);
            assert_eq!(slid.discontinuity_sequence, 1);
            assert!(!slid.body.lines().any(|line| line == "#EXT-X-DISCONTINUITY"));
            assert_eq!(session.segments[&5], old_binding);
            eprintln!("handoff failed_head={failed_head} origin_tag={origin_tag} head_msn={first_msn} header=0 number=1 slid_header=1");
        }
    }

    #[test]
    fn recovery_boundary_covers_first_ready_window_prefix() {
        use shared::model::HlsStartupMode;

        let ready = SegmentCacheStatus::Ready { content_length: 1024, ready_at_ms: 20 };
        let retryable = SegmentCacheStatus::FailedRetryable { failed_at_ms: 20, retry_after_ms: 1_000 };
        let cases = [
            (None, SegmentCacheStatus::Discovered, 0),
            (Some(6), retryable, 0),
            (Some(7), ready, 0),
            (Some(6), segment_not_found(), 1),
            (Some(7), segment_not_found(), 1),
        ];
        for (changed, status, expected) in cases {
            let mut session = session();
            session.apply_origin_manifest(&reentry_manifest(100, &["7"; 6])).unwrap();
            install_startup(&mut session, HlsStartupMode::FirstReady).unwrap();
            session.startup.as_mut().unwrap().revisions.clear();
            ready_recovery_window(&mut session);
            let old = session.render_and_store_manifest(10).unwrap();
            assert_eq!(old.last_proxy_seq, 5);
            assert!(discontinuity_numbers(&old).iter().all(|&number| number == 0));
            let old_binding = session.segments[&5].clone();

            session.apply_origin_manifest(&reentry_manifest(106, &["7"; 8])).unwrap();
            assert_eq!(session.publishable_origin_head_proxy_seq, Some(6));
            ready_recovery_window(&mut session);
            session.segments.get_mut(&6).unwrap().status = SegmentCacheStatus::Discovered;
            session.segments.get_mut(&7).unwrap().status = SegmentCacheStatus::Discovered;
            if let Some(seq) = changed {
                session.segments.get_mut(&seq).unwrap().status = status.clone();
            }
            let recovered = session.render_and_store_manifest(20).unwrap();
            assert_eq!(recovered.segment_proxy_seqs, [8, 9, 10, 11, 12, 13]);
            assert_eq!(session.segments[&8].origin_key.host_local_sequence, 108);
            assert_eq!(recovered.discontinuity_sequence, 0);
            let tags = recovered.body.lines().filter(|line| *line == "#EXT-X-DISCONTINUITY").count();
            assert_eq!(tags, expected);
            assert_eq!(discontinuity_numbers(&recovered), [expected as u64; 6]);
            assert_eq!(session.render_and_store_manifest(21).unwrap().body, recovered.body);
            let overlap = [9, 10, 11, 12, 13].map(|seq| {
                (
                    session.segments[&seq].cache_key.clone(),
                    session.startup.as_ref().unwrap().revisions[&seq].revision().id(),
                )
            });

            session.apply_origin_manifest(&reentry_manifest(109, &["7"; 6])).unwrap();
            ready_recovery_window(&mut session);
            let slid = session.render_and_store_manifest(30).unwrap();
            assert_eq!(slid.segment_proxy_seqs, [9, 10, 11, 12, 13, 14]);
            assert_eq!(slid.discontinuity_sequence, expected as u64);
            assert!(!slid.body.lines().any(|line| line == "#EXT-X-DISCONTINUITY"));
            assert_eq!(discontinuity_numbers(&slid), [expected as u64; 6]);
            assert!(slid.body.lines().filter(|line| line.starts_with("/hls/")).take(5).eq(recovered
                .body
                .lines()
                .filter(|line| line.starts_with("/hls/"))
                .skip(1)));
            assert_eq!(session.segments[&5], old_binding);
            for (seq, (key, revision)) in (9..=13).zip(overlap) {
                assert_eq!(session.segments[&seq].cache_key, key);
                assert_eq!(session.startup.as_ref().unwrap().revisions[&seq].revision().id(), revision);
            }
            eprintln!("first_ready_prefix changed_proxy={changed:?} status={status:?} first_proxy=8 header=0 tags={tags} slid_proxy=9 slid_header={}", slid.discontinuity_sequence);
        }
    }

    #[test]
    fn recovery_boundary_uses_predecessor_state_at_first_publication() {
        let pending = SegmentCacheStatus::Fetching { started_at_ms: 15, priority: SegmentFetchPriority::RenderWindow };
        let ready = SegmentCacheStatus::Ready { content_length: 1024, ready_at_ms: 25 };
        for (status, expected_number) in [(pending.clone(), 1), (ready, 1), (segment_not_found(), 2)] {
            let mut session = session();
            let mut old = reentry_manifest(100, &["7"; 6]);
            old.segments[1].discontinuity_before = true;
            session.apply_origin_manifest(&old).unwrap();
            mark_all_segments_ready(&mut session);
            session.segments.get_mut(&5).unwrap().status = pending.clone();
            session.render_policy = RenderPolicy { initial_render_gap_segments: 1, max_not_ready_render_segments: 0 };
            let previous = session.render_and_store_manifest(20).unwrap();
            assert_eq!(previous.segment_proxy_seqs, [0, 1, 2, 3, 4]);
            assert_eq!(discontinuity_numbers(&previous), [0, 1, 1, 1, 1]);
            let old_binding = session.segments[&4].clone();

            let recovery = reentry_manifest(106, &["7"; 6]);
            session.apply_origin_manifest(&recovery).unwrap();
            assert!(matches!(session.segments[&5].status, SegmentCacheStatus::Fetching { .. }));
            assert!(!session.segments[&6].discontinuity_before);
            for (_, entry) in session.segments.range_mut(6..) {
                entry.status = SegmentCacheStatus::Ready { content_length: 1024, ready_at_ms: 30 };
            }
            session.segments.get_mut(&5).unwrap().status = status.clone();

            let recovered = session.render_and_store_manifest(30).unwrap();
            assert_eq!(recovered.segment_proxy_seqs, [6, 7, 8, 9, 10, 11]);
            assert_eq!(recovered.discontinuity_sequence, 1);
            eprintln!(
                "predecessor={status:?} first_proxy=6 first_msn=106 numbers={:?}",
                discontinuity_numbers(&recovered)
            );
            assert_eq!(discontinuity_numbers(&recovered), [expected_number; 6]);
            assert_eq!(session.segments[&4], old_binding);

            session.segments.get_mut(&5).unwrap().status = segment_not_found();
            session.apply_origin_manifest(&recovery).unwrap();
            assert_eq!(session.render_and_store_manifest(40).unwrap().body, recovered.body);
        }
    }

    #[test]
    fn recovery_boundary_finds_missing_media_inside_skipped_mappings() {
        let discovered = SegmentCacheStatus::Discovered;
        let ready = SegmentCacheStatus::Ready { content_length: 1024, ready_at_ms: 25 };
        let pending = SegmentCacheStatus::Fetching { started_at_ms: 15, priority: SegmentFetchPriority::RenderWindow };
        let retryable = SegmentCacheStatus::FailedRetryable { failed_at_ms: 25, retry_after_ms: 1_000 };
        for (missing, predecessor, late, advance_before_poll) in [
            (Some(5), discovered.clone(), false, false),
            (Some(5), ready.clone(), false, false),
            (Some(5), pending.clone(), true, false),
            (Some(5), retryable.clone(), false, true),
            (None, discovered, false, false),
            (None, ready, false, false),
            (None, pending, false, false),
            (None, retryable, false, false),
            (Some(4), SegmentCacheStatus::Discovered, false, false),
        ] {
            let mut session = session();
            session.apply_origin_manifest(&reentry_manifest(100, &["7"; 7])).unwrap();
            for (_, entry) in session.segments.range_mut(..5) {
                entry.status = SegmentCacheStatus::Ready { content_length: 1024, ready_at_ms: 10 };
            }
            session.render_policy.initial_render_gap_segments = 2;
            let old = session.render_and_store_manifest(10).unwrap();
            assert_eq!(old.segment_proxy_seqs, [0, 1, 2, 3, 4]);
            assert_eq!(discontinuity_numbers(&old), [0; 5]);
            session.segments.get_mut(&6).unwrap().status = predecessor.clone();
            if let Some(seq) = missing.filter(|_| !late) {
                session.segments.get_mut(&seq).unwrap().status = segment_not_found();
            }
            let old_bindings = session.segments.range(..5).map(|(_, entry)| entry.clone()).collect::<Vec<_>>();

            let recovery = reentry_manifest(107, &["7"; 6]);
            session.apply_origin_manifest(&recovery).unwrap();
            assert_eq!(session.publishable_origin_head_proxy_seq, Some(7));
            if late {
                assert!(!session.segments[&7].discontinuity_before);
                session.segments.get_mut(&5).unwrap().status = segment_not_found();
            }
            let first = if advance_before_poll {
                session.apply_origin_manifest(&reentry_manifest(108, &["7"; 6])).unwrap();
                8
            } else {
                7
            };
            for (_, entry) in session.segments.range_mut(7..) {
                entry.status = SegmentCacheStatus::Ready { content_length: 1024, ready_at_ms: 30 };
            }
            let recovered = session.render_and_store_manifest(30).unwrap();
            let number = u64::from(missing == Some(5));
            let tags = recovered.body.lines().filter(|line| *line == "#EXT-X-DISCONTINUITY").count();
            eprintln!("skipped_missing_proxy={missing:?} predecessor={predecessor:?} late={late} first_proxy={first} header={} tags={tags}", recovered.discontinuity_sequence);
            assert_eq!(recovered.segment_proxy_seqs, (first..first + 6).collect::<Vec<_>>());
            assert_eq!(recovered.discontinuity_sequence, 0);
            assert_eq!(tags as u64, number);
            assert_eq!(discontinuity_numbers(&recovered), [number; 6]);
            assert_eq!(session.render_and_store_manifest(31).unwrap().body, recovered.body);

            let recovery_bindings = session
                .segments
                .range(first + 1..first + 6)
                .map(|(_, entry)| entry.cache_key.clone())
                .collect::<Vec<_>>();
            session.apply_origin_manifest(&reentry_manifest(101 + first, &["7"; 6])).unwrap();
            session.segments.get_mut(&(first + 6)).unwrap().status =
                SegmentCacheStatus::Ready { content_length: 1024, ready_at_ms: 40 };
            let slid = session.render_and_store_manifest(40).unwrap();
            assert_eq!(slid.segment_proxy_seqs, (first + 1..=first + 6).collect::<Vec<_>>());
            assert_eq!(slid.discontinuity_sequence, number);
            assert!(!slid.body.lines().any(|line| line == "#EXT-X-DISCONTINUITY"));
            assert_eq!(discontinuity_numbers(&slid), [number; 6]);
            assert!(session.segments.range(..5).map(|(_, entry)| entry).eq(&old_bindings));
            assert!(session
                .segments
                .range(first + 1..first + 6)
                .map(|(_, entry)| &entry.cache_key)
                .eq(&recovery_bindings));
        }
    }

    #[test]
    fn recovery_boundary_follows_unpublished_missing_head_and_counts_once() {
        for (source_tag, successor_tag, failed_count) in
            [(false, false, 1), (false, false, 2), (true, false, 2), (true, true, 2)]
        {
            let mut session = session();
            session.apply_origin_manifest(&reentry_manifest(100, &["7"; 6])).unwrap();
            mark_all_segments_ready(&mut session);
            let previous = session.render_and_store_manifest(10).unwrap();
            assert_eq!(discontinuity_numbers(&previous), [0; 6]);
            let old_binding = session.segments[&5].clone();

            let mut recovery = reentry_manifest(107, &["7"; 7]);
            recovery.segments[0].discontinuity_before = source_tag;
            recovery.segments[failed_count].discontinuity_before = successor_tag;
            session.apply_origin_manifest(&recovery).unwrap();
            mark_all_segments_ready(&mut session);
            assert!(session.segments[&6].discontinuity_before);
            assert!(session.segments[&6].last_rendered_at_ms.is_none());
            let first = 6 + u64::try_from(failed_count).unwrap();
            for (_, entry) in session.segments.range_mut(6..first) {
                entry.status = segment_not_found();
            }
            let recovered = session.render_and_store_manifest(30).unwrap();
            assert_eq!(recovered.segment_proxy_seqs, (first..=12).collect::<Vec<_>>());
            eprintln!(
                "source_tag={source_tag} successor_tag={successor_tag} first_proxy={first} first_msn={} header={} numbers={:?}",
                session.segments[&first].origin_key.host_local_sequence,
                recovered.discontinuity_sequence,
                discontinuity_numbers(&recovered),
            );
            assert_eq!(recovered.discontinuity_sequence, 0);
            assert_eq!(discontinuity_numbers(&recovered), vec![1; recovered.segment_proxy_seqs.len()]);
            assert!(recovered.body.contains("#EXT-X-DISCONTINUITY\n#EXTINF:7.000,\n"));
            assert_eq!(session.origin_discontinuity_before(&session.segments[&6]), source_tag);
            assert_eq!(session.origin_discontinuity_before(&session.segments[&first]), successor_tag);
            assert_eq!(session.segments[&5], old_binding);

            session.apply_origin_manifest(&recovery).unwrap();
            assert_eq!(session.render_and_store_manifest(40).unwrap().body, recovered.body);
            let next_msn = session.segments[&first].origin_key.host_local_sequence + 1;
            session.apply_origin_manifest(&reentry_manifest(next_msn, &["7"; 6])).unwrap();
            for (_, entry) in session.segments.range_mut(first + 1..) {
                entry.status = SegmentCacheStatus::Ready { content_length: 1024, ready_at_ms: 50 };
            }
            let slid = session.render_and_store_manifest(50).unwrap();
            assert_eq!(slid.segment_proxy_seqs, (first + 1..=first + 6).collect::<Vec<_>>());
            assert_eq!(slid.discontinuity_sequence, 1);
            assert_eq!(discontinuity_numbers(&slid), [1; 6]);
        }
    }

    #[test]
    fn recovery_boundary_does_not_renumber_published_successors_of_an_unpublished_head() {
        let mut session = session();
        session.apply_origin_manifest(&reentry_manifest(100, &["7"; 6])).unwrap();
        mark_all_segments_ready(&mut session);
        session.segments.get_mut(&5).unwrap().status =
            SegmentCacheStatus::Fetching { started_at_ms: 15, priority: SegmentFetchPriority::RenderWindow };
        session.render_policy = RenderPolicy { initial_render_gap_segments: 1, max_not_ready_render_segments: 0 };
        assert_eq!(session.render_and_store_manifest(20).unwrap().last_proxy_seq, 4);

        let recovery = reentry_manifest(106, &["7"; 8]);
        session.apply_origin_manifest(&recovery).unwrap();
        mark_all_segments_ready(&mut session);
        let published = session.render_and_store_manifest(30).unwrap();
        assert_eq!(published.segment_proxy_seqs, [8, 9, 10, 11, 12, 13]);
        assert!(session.segments[&6].last_rendered_at_ms.is_none());

        session.segments.get_mut(&5).unwrap().status = segment_not_found();
        session.apply_origin_manifest(&recovery).unwrap();
        assert_eq!(session.render_and_store_manifest(40).unwrap().body, published.body);
        assert_eq!(discontinuity_numbers(&published), [0; 6]);
        assert!(!session.segments[&6].discontinuity_before);
    }

    #[test]
    fn origin_jump_after_idle_keeps_cached_predecessors_up_to_three_target_durations() {
        // v3.3.151 validation: retained session served [1,2,3,4], the next refresh rendered [3,4,5]
        // (18 s < 3 x 7 s) and the client that had fetched segment 1 skipped segment 2.
        let mut session = session();
        session.apply_origin_manifest(&reentry_manifest(100, &["6.0", "6.0", "6.0", "5.16"])).expect("map");
        mark_all_segments_ready(&mut session);
        session.render_and_store_manifest(10).expect("first window");
        session.apply_origin_manifest(&reentry_manifest(101, &["6.0", "6.0", "5.16", "6.84"])).expect("map");
        mark_all_segments_ready(&mut session);
        let previous = session.render_and_store_manifest(20).expect("second window");
        assert_eq!(previous.segment_proxy_seqs, vec![1, 2, 3, 4]);

        session.apply_origin_manifest(&reentry_manifest(103, &["5.16", "6.84", "6.0"])).expect("map");
        mark_all_segments_ready(&mut session);
        let rendered = session.render_and_store_manifest(30).expect("window after origin jump");

        assert_eq!(rendered.segment_proxy_seqs, vec![2, 3, 4, 5]);
        assert!(rendered.body.contains("#EXT-X-MEDIA-SEQUENCE:2\n"));
        assert!(rendered.playlist_duration_ms >= 21_000);
    }

    fn publish_startup_revisions(session: &mut HlsSession) -> std::io::Result<()> {
        let proxy_session_id = session.proxy_session_id.clone();
        let seqs = session.segments.keys().copied().collect::<Vec<_>>();
        let startup = session.startup.as_mut().ok_or_else(|| std::io::Error::other("startup"))?;
        for seq in seqs {
            if !startup.revisions.contains_key(&seq) {
                let revision = startup.store.create(proxy_session_id.clone(), seq, crate::SegmentRevisionKind::Raw)?;
                revision.revision().prefix_available.store(1, std::sync::atomic::Ordering::Release);
                startup.revisions.insert(seq, revision);
            }
        }
        Ok(())
    }

    #[test]
    fn optimistic_startup_session_keeps_cached_predecessors_up_to_three_target_durations() -> std::io::Result<()> {
        // v3.3.153 validation: optimistic sessions keep `startup` for their whole lifetime, so a
        // 3-segment origin window was served as-is (17.96 s with target 7 s) on return entries.
        let mut session = session();
        install_startup(&mut session, shared::model::HlsStartupMode::Optimistic)?;
        for (media_sequence, durations) in [
            (100, &["6.0", "6.0", "6.0", "5.16"][..]),
            (101, &["6.0", "6.0", "5.16", "6.84"][..]),
            (103, &["5.16", "6.84", "6.0"][..]),
        ] {
            session
                .apply_origin_manifest(&reentry_manifest(media_sequence, durations))
                .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
            mark_all_segments_ready(&mut session);
            publish_startup_revisions(&mut session)?;
            session
                .render_and_store_manifest(media_sequence * 10)
                .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        }

        let rendered = session.last_rendered_manifest.as_ref().ok_or_else(|| std::io::Error::other("rendered"))?;
        assert!(session.startup.is_some());
        assert_eq!(rendered.segment_proxy_seqs, vec![2, 3, 4, 5]);
        assert!(rendered.playlist_duration_ms >= 21_000);
        Ok(())
    }

    #[test]
    fn origin_jump_never_republishes_segments_before_the_previous_window() {
        let mut session = session();
        session.apply_origin_manifest(&reentry_manifest(100, &["6.0", "6.0", "6.0", "5.16"])).expect("map");
        mark_all_segments_ready(&mut session);
        session.render_and_store_manifest(10).expect("first window");
        session.apply_origin_manifest(&reentry_manifest(101, &["6.0", "6.0", "5.16", "6.84"])).expect("map");
        mark_all_segments_ready(&mut session);
        session.render_and_store_manifest(20).expect("second window");

        session.apply_origin_manifest(&reentry_manifest(104, &["6.84", "6.0"])).expect("map");
        mark_all_segments_ready(&mut session);
        session.segments.get_mut(&2).expect("segment 2").status = SegmentCacheStatus::Expired;
        let rendered = session.render_and_store_manifest(30).expect("window after origin jump");

        // Segment 2 is gone, so the contiguous predecessor chain stops at 3; 0 is never revisited.
        assert_eq!(rendered.segment_proxy_seqs, vec![3, 4, 5]);
    }

    #[test]
    fn first_render_does_not_prepend_historical_segments() {
        let mut session = session();
        session.apply_origin_manifest(&reentry_manifest(100, &["6.0", "6.0", "6.0", "5.16"])).expect("map");
        session.apply_origin_manifest(&reentry_manifest(103, &["5.16", "6.84", "6.0"])).expect("map");
        mark_all_segments_ready(&mut session);

        let rendered = session.render_and_store_manifest(10).expect("first window");

        assert_eq!(rendered.segment_proxy_seqs, vec![3, 4, 5]);
    }

    #[test]
    fn member_handoff_drops_orphaned_unpublished_failure_and_renders_new_epoch_window() {
        let mut session = session();
        let old_manifest = normal_manifest(
            "#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXT-X-MEDIA-SEQUENCE:0\n\
             #EXTINF:2,\n0.ts\n#EXTINF:2,\n1.ts\n#EXTINF:2,\n2.ts\n\
             #EXTINF:2,\n3.ts\n#EXTINF:2,\n4.ts\n#EXTINF:2,\n5.ts\n\
             #EXTINF:2,\n6.ts\n",
        );
        session.apply_origin_manifest(&old_manifest).expect("manifest maps");
        session.publishable_origin_head_proxy_seq = Some(3);
        session.publishable_origin_tail_proxy_seq = Some(6);
        for seq in 3..=5 {
            if let Some(entry) = session.segments.get_mut(&seq) {
                entry.status = SegmentCacheStatus::Ready { content_length: 1024, ready_at_ms: 10 };
                entry.last_rendered_at_ms = Some(10);
            }
        }
        session.last_rendered_manifest = Some(rendered_manifest(3, 5));
        if let Some(entry) = session.segments.get_mut(&6) {
            entry.status = SegmentCacheStatus::FailedRetryable { failed_at_ms: 20, retry_after_ms: 100 };
            entry.last_rendered_at_ms = None;
        }

        let new_source = crate::origin::HlsOriginSource::new(
            2,
            std::sync::Arc::from("member-b"),
            "member-b-stream",
            crate::origin::HlsOriginSourceKind::DirectMediaPlaylist,
        );
        let binding = crate::origin::HlsOriginAccountBinding::rebound(
            std::sync::Arc::from("member-b"),
            std::sync::Arc::from("account-b"),
            crate::origin::build_hls_origin_session_owner(&session.proxy_session_id),
            1,
            10,
        );
        session.handoff_origin_member(new_source, binding);

        let new_manifest = normal_manifest(
            "#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXT-X-MEDIA-SEQUENCE:100\n\
             #EXTINF:2,\n100.ts\n#EXTINF:2,\n101.ts\n#EXTINF:2,\n102.ts\n",
        );
        session.apply_origin_manifest(&new_manifest).expect("new member manifest maps");
        for seq in 7..=9 {
            if let Some(entry) = session.segments.get_mut(&seq) {
                entry.status = SegmentCacheStatus::Ready { content_length: 1024, ready_at_ms: 30 };
            }
        }

        let rendered = HlsManifestRenderer::render(&session, 30).expect("manifest should render");
        assert!(rendered.body.contains("#EXT-X-DISCONTINUITY"));
        assert_eq!(rendered.first_proxy_seq, 7);
        assert_eq!(rendered.last_proxy_seq, 9);
        assert_eq!(rendered.segment_proxy_seqs, vec![7, 8, 9]);
        assert!(session.segments.contains_key(&3));
        assert!(session.segments.contains_key(&4));
        assert!(session.segments.contains_key(&5));
        assert!(!session.segments.contains_key(&6));
    }

    #[test]
    fn provisioning_member_handoff_prune_preserves_monotonic_discontinuity_sequence() {
        let mut session = session();
        let old_manifest = normal_manifest(
            "#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXT-X-MEDIA-SEQUENCE:0\n\
             #EXTINF:2,\n0.ts\n\
             #EXT-X-DISCONTINUITY\n\
             #EXTINF:2,\n1.ts\n\
             #EXTINF:2,\n2.ts\n\
             #EXTINF:2,\n3.ts\n\
             #EXTINF:2,\n4.ts\n\
             #EXTINF:2,\n5.ts\n\
             #EXTINF:2,\n6.ts\n",
        );
        session.apply_origin_manifest(&old_manifest).expect("manifest maps");
        session.publishable_origin_head_proxy_seq = Some(3);
        session.publishable_origin_tail_proxy_seq = Some(6);
        for seq in 3..=5 {
            if let Some(entry) = session.segments.get_mut(&seq) {
                entry.status = SegmentCacheStatus::Ready { content_length: 1024, ready_at_ms: 10 };
                entry.last_rendered_at_ms = Some(10);
            }
        }
        if let Some(entry) = session.segments.get_mut(&6) {
            entry.status = SegmentCacheStatus::FailedRetryable { failed_at_ms: 20, retry_after_ms: 100 };
            entry.last_rendered_at_ms = None;
        }
        let rendered_before = HlsManifestRenderer::render(&session, 10).expect("manifest should render before handoff");
        assert_eq!(rendered_before.first_proxy_seq, 3);
        assert_eq!(rendered_before.discontinuity_sequence, 1);
        session.last_rendered_manifest = Some(rendered_before.clone());

        let new_source = crate::origin::HlsOriginSource::new(
            2,
            std::sync::Arc::from("member-b"),
            "member-b-stream",
            crate::origin::HlsOriginSourceKind::DirectMediaPlaylist,
        );
        let binding = crate::origin::HlsOriginAccountBinding::rebound(
            std::sync::Arc::from("member-b"),
            std::sync::Arc::from("account-b"),
            crate::origin::build_hls_origin_session_owner(&session.proxy_session_id),
            1,
            10,
        );
        session.handoff_origin_member(new_source, binding);

        let new_manifest = normal_manifest(
            "#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXT-X-MEDIA-SEQUENCE:100\n\
             #EXTINF:2,\n100.ts\n#EXTINF:2,\n101.ts\n#EXTINF:2,\n102.ts\n",
        );
        session.apply_origin_manifest(&new_manifest).expect("new member manifest maps");
        for seq in 7..=9 {
            if let Some(entry) = session.segments.get_mut(&seq) {
                entry.status = SegmentCacheStatus::Ready { content_length: 1024, ready_at_ms: 30 };
            }
        }

        let rendered_after = HlsManifestRenderer::render(&session, 30).expect("manifest should render after handoff");
        assert!(
            rendered_after.discontinuity_sequence >= rendered_before.discontinuity_sequence,
            "discontinuity sequence must be non-decreasing: before={}, after={}",
            rendered_before.discontinuity_sequence,
            rendered_after.discontinuity_sequence
        );
    }

    #[test]
    fn without_handoff_failed_retryable_head_blocks_render() {
        let mut session = session();
        let manifest = normal_manifest(
            "#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXT-X-MEDIA-SEQUENCE:0\n\
             #EXTINF:2,\n0.ts\n#EXTINF:2,\n1.ts\n#EXTINF:2,\n2.ts\n",
        );
        session.apply_origin_manifest(&manifest).expect("manifest maps");
        if let Some(entry) = session.segments.get_mut(&0) {
            entry.status = SegmentCacheStatus::FailedRetryable { failed_at_ms: 20, retry_after_ms: 100 };
            entry.last_rendered_at_ms = None;
        }
        for seq in 1..=2 {
            if let Some(entry) = session.segments.get_mut(&seq) {
                entry.status = SegmentCacheStatus::Ready { content_length: 1024, ready_at_ms: 10 };
            }
        }

        let render_result = HlsManifestRenderer::render(&session, 30);
        assert!(matches!(render_result, Err(RenderError::NoRenderableWindow)));
    }
}
