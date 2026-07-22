use crate::core::{
    error::{ErrorCode, RehomeError},
    models::RestorePlan,
};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};
use uuid::Uuid;

fn plans() -> &'static Mutex<HashMap<Uuid, RestorePlan>> {
    static PLANS: OnceLock<Mutex<HashMap<Uuid, RestorePlan>>> = OnceLock::new();
    PLANS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn store(plan: &RestorePlan) -> Result<(), RehomeError> {
    plans()
        .lock()
        .map_err(|_| restore_failed("server-held restore plan store is unavailable"))?
        .insert(plan.plan_id, plan.clone());
    Ok(())
}

pub(crate) fn load(plan_id: Uuid) -> Result<RestorePlan, RehomeError> {
    plans()
        .lock()
        .map_err(|_| restore_failed("server-held restore plan store is unavailable"))?
        .get(&plan_id)
        .cloned()
        .ok_or_else(|| restore_failed("server-held restore plan was not found or has expired"))
}

pub(crate) fn load_exact(provided: &RestorePlan) -> Result<RestorePlan, RehomeError> {
    let stored = load(provided.plan_id)?;
    if stored != *provided {
        return Err(restore_failed(
            "restore plan does not match the server-held plan",
        ));
    }
    Ok(stored)
}

fn restore_failed(message: impl Into<String>) -> RehomeError {
    RehomeError::new(ErrorCode::RestoreFailed, message)
}
