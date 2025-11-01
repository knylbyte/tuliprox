use std::sync::Arc;
use crate::api::model::{ConnectionGuardUserManager, ProviderAllocation, ProviderConnectionGuard};

#[derive(Clone)]
pub enum ReleaseTask {
    ForceProvider(Arc<ProviderConnectionGuard>),
    ProviderAllocation(ProviderAllocation),
    UserConnection(Arc<ConnectionGuardUserManager>, String)
}