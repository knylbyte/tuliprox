use super::{
    identity::TransientResourceValidityScope, HlsAccessLeaseId, HlsPublishedTransientResourceIds, ProxySessionId,
    TransientObjectCacheKey, TransientObjectCacheStatus, TransientObjectUnavailableState, TransientPassthroughState,
    TransientResourceId, TransientResourceRef, TransientResourceRevision,
};
use crate::transient_manifest::occurrence::TransientOccurrenceBinding;
use std::{collections::HashSet, sync::Arc};

impl TransientPassthroughState {
    /// Reuse immutable published fetch/dependency bindings; never retarget an old route.
    pub(super) fn bind_manifest_resources(
        &self,
        resources: Vec<TransientResourceRef>,
    ) -> Result<Vec<TransientResourceRef>, super::HlsTransientManifestCommitError> {
        let positions = self
            .resources
            .values()
            .filter_map(|resource| {
                resource.occurrence_binding.as_deref()?.position().map(|position| (position, resource))
            })
            .collect::<std::collections::HashMap<_, _>>();
        resources
            .into_iter()
            .map(|resource| {
                let previous = resource
                    .occurrence_binding
                    .as_deref()
                    .copied()
                    .and_then(TransientOccurrenceBinding::position)
                    .and_then(|position| positions.get(&position).copied())
                    .or_else(|| self.resources.get(&resource.id));
                let Some(previous) = previous.filter(|_| resource.occurrence_binding.is_some()) else {
                    return Ok(resource);
                };
                if previous.id != resource.id
                    || previous.occurrence_binding != resource.occurrence_binding
                    || previous.kind != resource.kind
                    || previous.encrypted_media != resource.encrypted_media
                    || previous.file_ext_hint != resource.file_ext_hint
                {
                    return Err(super::HlsTransientManifestCommitError::OccurrenceConflict);
                }
                let mut retained = previous.clone();
                retained.expires_at_ms = retained.expires_at_ms.max(resource.expires_at_ms);
                Ok(retained)
            })
            .collect()
    }

    pub(crate) fn invalidate_origin_resources(&mut self) {
        self.invalid_origin_resources.extend(self.resources.keys().cloned());
        self.origin_account_generation = self.origin_account_generation.saturating_add(1);
        self.rolling_event_rewrite_seed = None;
    }

    pub(crate) fn origin_account_generation(&self) -> u64 { self.origin_account_generation }

    /// Whether this mapping may initiate origin work for the current account.
    pub fn origin_resource_is_fetchable(&self, resource: &TransientResourceRef, now_ms: u64) -> bool {
        (resource.is_valid_at(now_ms) || self.protected_finalized_resource_refcounts.contains_key(&resource.id))
            && resource.origin_account_generation == self.origin_account_generation
            && !self.invalid_origin_resources.contains(&resource.id)
            && self.resource_matches_current(resource, now_ms)
    }

    pub(crate) fn upsert_normal_key_resources(
        &mut self,
        proxy_session_id: &ProxySessionId,
        mut resources: Vec<TransientResourceRef>,
        now_ms: u64,
    ) {
        for resource in &mut resources {
            let Some(previous) = self.resources.get(&resource.id) else { continue };
            resource.expires_at_ms = resource.expires_at_ms.max(previous.expires_at_ms);
            // A usable key keeps its cache binding. An unfetched key can take
            // a fresh transport URL without changing its published resource ID.
            if resource.file_ext_hint.as_deref().is_some_and(|extension| {
                let key = Self::transient_object_key(proxy_session_id, &resource.id, extension);
                self.object_cache.get(&key).is_some_and(|entry| {
                    entry.ready_content_length() == Some(16)
                        && entry.binding.matches_resource_identity(previous)
                        && (entry.expires_at_ms >= now_ms || previous.access.retention.protects(now_ms))
                })
            }) {
                resource.resolved_origin_uri.clone_from(&previous.resolved_origin_uri);
            }
        }
        self.upsert_resources(resources);
    }

    pub fn upsert_resources<I>(&mut self, resources: I)
    where
        I: IntoIterator<Item = TransientResourceRef>,
    {
        for mut resource in resources {
            if resource.origin_account_generation == self.origin_account_generation {
                self.invalid_origin_resources.remove(&resource.id);
            }
            let preserve_revision = self
                .resources
                .get(&resource.id)
                .filter(|existing| existing.has_same_cache_identity(&resource))
                .map(|existing| existing.revision);
            resource.revision = if let Some(revision) = preserve_revision {
                revision
            } else {
                let revision = TransientResourceRevision(self.next_resource_revision);
                self.next_resource_revision = self.next_resource_revision.saturating_add(1);
                revision
            };
            match self.resources.get_mut(&resource.id) {
                Some(existing) => existing.refresh_from(resource),
                None => {
                    let _previous = self.resources.insert(resource.id.clone(), resource);
                }
            }
        }
    }

    pub(crate) fn retained_cached_resource(&self, resource: &TransientResourceRef, now_ms: u64) -> bool {
        resource.access.retention.protects(now_ms)
            && self.object_cache.values().any(|entry| {
                entry.key.transient_resource_id() == &resource.id
                    && entry.ready_content_length().is_some()
                    && entry.binding.matches_resource_identity(resource)
            })
    }

    pub(super) fn resource_is_valid_at_for(
        &self,
        resource: &TransientResourceRef,
        now_ms: u64,
        scope: TransientResourceValidityScope<'_>,
    ) -> bool {
        match scope {
            TransientResourceValidityScope::PublishedSession => {
                resource.is_valid_at(now_ms)
                    || self.retained_cached_resource(resource, now_ms)
                    || self.protected_finalized_resource_refcounts.contains_key(&resource.id)
            }
            TransientResourceValidityScope::AccessLease { lease_id, lease_issued_at_ms, published_resource_ids } => {
                (published_resource_ids.contains(&resource.id)
                    && (resource.is_valid_at(now_ms) || self.retained_cached_resource(resource, now_ms)))
                    || self.finalized_manifest_lease_bindings.iter().any(|binding| {
                        binding.lease_id == *lease_id
                            && binding.lease_issued_at_ms == lease_issued_at_ms
                            && self
                                .finalized_manifest_generations
                                .get(&binding.manifest_generation)
                                .is_some_and(|generation| generation.resource_ids.contains(&resource.id))
                    })
            }
        }
    }

    pub(crate) fn resource_is_valid_at(&self, resource: &TransientResourceRef, now_ms: u64) -> bool {
        self.resource_is_valid_at_for(resource, now_ms, TransientResourceValidityScope::PublishedSession)
    }

    pub(crate) fn resource_matches_current(&self, resource: &TransientResourceRef, now_ms: u64) -> bool {
        self.resources.get(&resource.id).is_some_and(|current| {
            Arc::ptr_eq(&current.access, &resource.access)
                && current.has_same_cache_identity(resource)
                && self.resource_is_valid_at(current, now_ms)
        })
    }

    pub fn resolve_current_resource(
        &self,
        resource_id: &TransientResourceId,
        now_ms: u64,
    ) -> Option<TransientResourceRef> {
        self.resources.get(resource_id).filter(|resource| self.resource_is_valid_at(resource, now_ms)).cloned()
    }

    pub(crate) fn resolve_resource_for_lease(
        &self,
        resource_id: &TransientResourceId,
        lease_id: &HlsAccessLeaseId,
        lease_issued_at_ms: u64,
        published_resource_ids: &HlsPublishedTransientResourceIds,
        now_ms: u64,
    ) -> Option<TransientResourceRef> {
        self.resources
            .get(resource_id)
            .filter(|resource| {
                self.resource_is_valid_at_for(
                    resource,
                    now_ms,
                    TransientResourceValidityScope::AccessLease {
                        lease_id,
                        lease_issued_at_ms,
                        published_resource_ids,
                    },
                )
            })
            .cloned()
    }

    pub fn get_valid_resource(
        &mut self,
        resource_id: &TransientResourceId,
        now_ms: u64,
    ) -> Option<TransientResourceRef> {
        self.prune_expired(now_ms);
        self.resolve_current_resource(resource_id, now_ms)
    }

    pub fn prune_expired(&mut self, now_ms: u64) { self.prune_expired_except(now_ms, &HashSet::new()); }

    pub fn prune_expired_except(&mut self, now_ms: u64, protected: &HashSet<TransientResourceId>) {
        let expired_resource_ids = self
            .resources
            .iter()
            .filter(|(id, resource)| {
                !(protected.contains(*id)
                    || (resource.occurrence_binding.is_some() && self.last_manifest_resource_ids.contains(*id))
                    || self.resource_is_valid_at(resource, now_ms)
                    || resource.active_readers() != 0)
            })
            .map(|(id, _)| id.clone())
            .collect::<HashSet<_>>();
        self.resources.retain(|id, _| !expired_resource_ids.contains(id));
        self.invalid_origin_resources.retain(|id| !expired_resource_ids.contains(id));
    }

    pub fn active_resource_readers(&self) -> u32 {
        self.resources.values().map(TransientResourceRef::active_readers).sum()
    }

    pub fn object_unavailable_state(
        &self,
        key: &TransientObjectCacheKey,
        now_ms: u64,
    ) -> TransientObjectUnavailableState {
        let Some(entry) = self.object_cache.get(key) else {
            return TransientObjectUnavailableState::Missing;
        };
        if entry.expires_at_ms < now_ms || !self.binding_is_current(&entry.binding, now_ms) {
            return TransientObjectUnavailableState::Missing;
        }
        match entry.status {
            TransientObjectCacheStatus::Fetching { .. } => TransientObjectUnavailableState::Fetching,
            TransientObjectCacheStatus::FailedRetryable { retry_after_ms, .. } => {
                TransientObjectUnavailableState::FailedRetryable { retry_after_ms }
            }
            TransientObjectCacheStatus::FailedPermanent { .. } => TransientObjectUnavailableState::FailedPermanent,
            TransientObjectCacheStatus::Ready { .. } => TransientObjectUnavailableState::Missing,
        }
    }
}
