//! Fixed-size retention evidence attached to an existing cache binding.
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

#[derive(Debug, Default)]
pub(crate) struct HlsPublicationRetention {
    publications: AtomicU32,
    segment_duration_ms: AtomicU64,
    longest_playlist_ms: AtomicU64,
    retained_until_ms: AtomicU64,
}

impl HlsPublicationRetention {
    pub(crate) fn publish(&self, duration_ms: u64, playlist_ms: u64, entering: bool) {
        self.segment_duration_ms.fetch_max(duration_ms, Ordering::AcqRel);
        self.longest_playlist_ms.fetch_max(playlist_ms, Ordering::AcqRel);
        if entering {
            self.publications.fetch_add(1, Ordering::AcqRel);
        }
    }

    pub(crate) fn remove(&self, now_ms: u64) {
        let previous =
            self.publications.try_update(Ordering::AcqRel, Ordering::Acquire, |count| Some(count.saturating_sub(1)));
        if matches!(previous, Ok(count) if count > 0) {
            let until = now_ms
                .saturating_add(self.segment_duration_ms.load(Ordering::Acquire))
                .saturating_add(self.longest_playlist_ms.load(Ordering::Acquire));
            self.retained_until_ms.fetch_max(until, Ordering::AcqRel);
        }
    }

    pub(crate) fn was_published(&self) -> bool { self.longest_playlist_ms.load(Ordering::Acquire) > 0 }

    pub(crate) fn protects(&self, now_ms: u64) -> bool {
        self.publications.load(Ordering::Acquire) > 0 || now_ms < self.retained_until_ms.load(Ordering::Acquire)
    }
}

impl PartialEq for HlsPublicationRetention {
    fn eq(&self, other: &Self) -> bool {
        self.publications.load(Ordering::Acquire) == other.publications.load(Ordering::Acquire)
            && self.segment_duration_ms.load(Ordering::Acquire) == other.segment_duration_ms.load(Ordering::Acquire)
            && self.longest_playlist_ms.load(Ordering::Acquire) == other.longest_playlist_ms.load(Ordering::Acquire)
            && self.retained_until_ms.load(Ordering::Acquire) == other.retained_until_ms.load(Ordering::Acquire)
    }
}
impl Eq for HlsPublicationRetention {}

impl crate::HlsSession {
    pub(crate) fn update_publication_retention(
        &self,
        previous_segments: &[u64],
        segments: &[u64],
        previous_resources: &crate::HlsPublishedTransientResourceIds,
        resources: &crate::HlsPublishedTransientResourceIds,
        snapshot: &crate::media_reserve::HlsLeaseManifestSnapshot,
    ) {
        let now = snapshot.delivered_at_ms;
        for seq in previous_segments.iter().filter(|seq| !segments.contains(seq)) {
            if let Some(entry) = self.segments.get(seq) {
                entry.access.retention.remove(now);
            }
        }
        for seq in segments {
            if let Some(entry) = self.segments.get(seq) {
                entry.access.retention.publish(
                    entry.duration_ms,
                    snapshot.playlist_duration_ms,
                    !previous_segments.contains(seq),
                );
            }
        }
        let maximum_duration = snapshot.visible_segments.iter().map(|unit| unit.duration_ms).max().unwrap_or(0);
        for (id, resource) in &self.transient.resources {
            if previous_resources.contains(id) && !resources.contains(id) {
                resource.access.retention.remove(now);
            }
            if resources.contains(id) {
                resource.access.retention.publish(
                    maximum_duration,
                    snapshot.playlist_duration_ms,
                    !previous_resources.contains(id),
                );
            }
        }
    }

    pub(crate) fn release_publication_retention(&mut self, lease_id: &crate::HlsAccessLeaseId, now_ms: u64) {
        if let Some(demand) = self.live_window_demands.remove(lease_id).filter(|demand| demand.retention_recorded) {
            if demand.retention_normal {
                for seq in demand.published {
                    if let Some(entry) = self.segments.get(&seq) {
                        entry.access.retention.remove(now_ms);
                    }
                }
            }
            for (id, resource) in &self.transient.resources {
                if demand.retention_resources.contains(id) {
                    resource.access.retention.remove(now_ms);
                }
            }
        }
    }
}
