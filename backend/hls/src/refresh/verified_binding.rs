//! Content-qualified bindings retain the original published occurrence and revision.

use crate::{resource_identity::HlsVerifiedOccurrenceBinding, HlsSession, SegmentEntry};
use std::collections::VecDeque;

const VERIFIED_BINDING_LIMIT: usize = 1_024;

#[derive(Debug)]
struct VerifiedBinding {
    binding: HlsVerifiedOccurrenceBinding,
    entry: SegmentEntry,
    generation: u64,
    account: u64,
    digest: [u8; 32],
}

impl VerifiedBinding {
    fn is_current(&self, session: &HlsSession) -> bool {
        self.generation == session.activity.origin_work_generation
            && self.account == session.transient.origin_account_generation()
            && self.binding.proxy_seq >= session.published_resource_history.restart_proxy_floor()
            && session.segments.get(&self.binding.proxy_seq).map_or_else(
                || {
                    // Publication history may outlive cache GC. The immutable original
                    // binding and its complete raw digest still qualify prefix trimming.
                    session.published_resource_history.binding_for(self.binding.original)
                        == Some((self.binding.original, self.binding.proxy_seq))
                        && session.origin_content.digest(&self.entry, self.generation) == Some(self.digest)
                },
                |entry| {
                    entry.origin_key == self.entry.origin_key
                        && entry.occurrence_identity == Some(self.binding.original)
                        && entry.cache_key == self.entry.cache_key
                        && entry.status == self.entry.status
                        && entry.origin_fetch_ref == self.entry.origin_fetch_ref
                        && entry.map_ref == self.entry.map_ref
                        && entry.encryption == self.entry.encryption
                        && session.origin_content.digest(entry, self.generation) == Some(self.digest)
                },
            )
    }
}

#[derive(Debug, Default)]
pub(super) struct HlsVerifiedOriginBindings {
    records: VecDeque<VerifiedBinding>,
}

impl HlsVerifiedOriginBindings {
    pub(super) fn confirm(
        &mut self,
        binding: HlsVerifiedOccurrenceBinding,
        entry: SegmentEntry,
        generation: u64,
        account: u64,
        digest: [u8; 32],
    ) {
        self.records.retain(|record| record.binding != binding);
        if self.records.len() == VERIFIED_BINDING_LIMIT {
            self.records.pop_front();
        }
        self.records.push_back(VerifiedBinding { binding, entry, generation, account, digest });
    }

    pub(super) fn current(&self, session: &HlsSession) -> Vec<HlsVerifiedOccurrenceBinding> {
        self.records.iter().filter(|record| record.is_current(session)).map(|record| record.binding).collect()
    }

    pub(super) fn known(&self) -> impl Iterator<Item = HlsVerifiedOccurrenceBinding> + '_ {
        self.records.iter().map(|record| record.binding)
    }
}
