//! Cancellation owns the same work counter and refresh token as normal completion.
use super::OriginRefreshRequest;
use crate::{manifest_fetch::HlsManifestAttemptContext, HlsSessionHandle};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    Arc,
};

pub struct RefreshWorkGuard {
    counter: Option<Arc<AtomicUsize>>,
    session: HlsSessionHandle,
    identity: Option<Arc<()>>,
    attempt: Option<Arc<HlsManifestAttemptContext>>,
    generation: Option<u64>,
    origin_epoch: AtomicU64,
    now_ms: u64,
    armed: AtomicBool,
    counter_finished: AtomicBool,
    origin_io: Option<crate::HlsOriginIoContext>,
    retained_poll_origin: Option<crate::manager::HlsRetainedPollOrigin>,
    cancellation: tokio_util::sync::CancellationToken,
}

impl RefreshWorkGuard {
    pub(super) fn new(request: &OriginRefreshRequest, session: &crate::HlsSession) -> Self {
        let generation = request.origin_io.as_ref().and_then(|io| io.started_generation);
        let counter =
            if generation.is_some() { Some(Arc::clone(&session.activity.active_origin_work_count)) } else { None };
        Self {
            counter,
            session: Arc::clone(&request.session),
            identity: request.refresh_identity.clone(),
            attempt: request.attempt.clone(),
            generation,
            origin_epoch: AtomicU64::new(session.origin_epoch),
            now_ms: request.hls_proxy.now_ms(),
            armed: AtomicBool::new(true),
            counter_finished: AtomicBool::new(false),
            origin_io: request.origin_io.clone(),
            retained_poll_origin: request.retained_poll_origin.clone(),
            cancellation: session.origin_work_cancellation.clone(),
        }
    }

    pub(super) fn disarm(&self) { self.armed.store(false, Ordering::Release); }
    pub(super) fn finish_counter(&self) { self.counter_finished.store(true, Ordering::Release); }

    pub(super) fn cancellation(&self) -> tokio_util::sync::CancellationToken { self.cancellation.clone() }

    pub(super) fn matches_epoch(&self, epoch: u64) -> bool { self.origin_epoch.load(Ordering::Acquire) == epoch }

    pub(super) fn record_committed_epoch(&self, epoch: u64) { self.origin_epoch.store(epoch, Ordering::Release); }
}

impl Drop for RefreshWorkGuard {
    fn drop(&mut self) {
        if !self.armed.load(Ordering::Acquire) {
            return;
        }
        if let Some(io) = self.origin_io.as_ref() {
            super::release_unused_preacquired_handle(io, self.retained_poll_origin.as_ref());
        }
        if let Some(counter) = self.counter.as_ref().filter(|_| !self.counter_finished.load(Ordering::Acquire)) {
            counter.fetch_sub(1, Ordering::AcqRel);
        }
        let Some(identity) = self.identity.clone() else { return };
        let session = Arc::clone(&self.session);
        let attempt = self.attempt.clone();
        let generation = self.generation;
        let now_ms = self.now_ms;
        // The provider I/O guard releases its own handle independently of this lock.
        tokio::spawn(async move {
            let mut session = session.write().await;
            if session.origin_refresh.refresh_identity.as_ref().is_some_and(|current| Arc::ptr_eq(&identity, current)) {
                if generation.is_none_or(|generation| generation == session.activity.origin_work_generation) {
                    if let Some(attempt) = attempt {
                        attempt.finish(true);
                    }
                }
                session.origin_refresh.mark_invalidated(now_ms);
            }
        });
    }
}
