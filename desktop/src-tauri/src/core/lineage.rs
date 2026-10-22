//! A paginated fork stores a reference, not a copy of its inherited history.
use crate::core::error::{ErrorCode, RehomeError};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use uuid::Uuid;

pub(crate) fn required_source(fields: &Value) -> Result<Option<Uuid>, RehomeError> {
    if ["history_base", "forked_from_ordinal_exclusive"]
        .iter()
        .any(|key| fields.get(*key).is_some_and(|v| !v.is_null()))
        && fields.get("history_mode").and_then(Value::as_str) != Some("paginated")
    {
        return Err(invalid(
            "paginated fork reference has an incompatible history_mode",
        ));
    }
    if let Some(base) = fields.get("history_base").filter(|v| !v.is_null()) {
        let id = base
            .get("thread_id")
            .and_then(Value::as_str)
            .and_then(|id| id.parse::<Uuid>().ok())
            .ok_or_else(|| invalid("invalid paginated history_base source ID"))?;
        let ordinal = base
            .get("end_ordinal_exclusive")
            .and_then(Value::as_u64)
            .ok_or_else(|| invalid("invalid paginated history_base ordinal cutoff"))?;
        if base
            .get("end_byte_offset")
            .and_then(Value::as_u64)
            .is_none()
        {
            return Err(invalid("invalid paginated history_base byte offset"));
        }
        if fields
            .get("forked_from_ordinal_exclusive")
            .filter(|v| !v.is_null())
            .is_some_and(|v| v.as_u64() != Some(ordinal))
            || fields
                .get("forked_from_id")
                .filter(|v| !v.is_null())
                .is_some_and(|v| v.as_str().and_then(|s| s.parse::<Uuid>().ok()) != Some(id))
        {
            return Err(invalid(
                "inconsistent paginated history_base and fork reference",
            ));
        }
        return Ok(Some(id));
    }
    let cutoff = fields
        .get("forked_from_ordinal_exclusive")
        .filter(|v| !v.is_null());
    if cutoff.is_none() {
        // Legacy forks already contain their inherited transcript. forked_from_id
        // alone is provenance, and must not force exporting private parent chats.
        return Ok(None);
    }
    if cutoff.and_then(Value::as_u64).is_none() {
        return Err(invalid("invalid paginated fork ordinal cutoff"));
    }
    fields
        .get("forked_from_id")
        .and_then(Value::as_str)
        .and_then(|id| id.parse().ok())
        .map(Some)
        .ok_or_else(|| invalid("paginated fork cutoff has no valid required source rollout ID"))
}

/// Parents first, bounded and iterative even for deeply nested user histories.
pub(crate) fn ordered(graph: &BTreeMap<Uuid, Option<Uuid>>) -> Result<Vec<Uuid>, RehomeError> {
    let mut done = HashSet::new();
    let mut result = Vec::with_capacity(graph.len());
    for start in graph.keys() {
        let mut path = Vec::new();
        let mut visiting = HashSet::new();
        let mut next = Some(*start);
        while let Some(id) = next {
            if done.contains(&id) {
                break;
            }
            if !visiting.insert(id) {
                return Err(invalid("paginated history dependency cycle"));
            }
            path.push(id);
            next = *graph.get(&id).ok_or_else(|| invalid(format!(
                "required source rollout {id} is missing from the selection/package; select this source conversation and its ancestors on the old computer and export them together. An incomplete package cannot recreate missing history"
            )))?;
        }
        for id in path.into_iter().rev() {
            done.insert(id);
            result.push(id);
        }
    }
    Ok(result)
}

fn invalid(message: impl Into<String>) -> RehomeError {
    RehomeError::new(ErrorCode::PackageInvalid, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn legacy_provenance_is_not_a_history_dependency() {
        assert_eq!(
            required_source(&json!({"forked_from_id":Uuid::nil(),"history_mode":"legacy"}))
                .unwrap(),
            None
        );
        assert!(required_source(&json!({"forked_from_ordinal_exclusive":1})).is_err());
        assert!(required_source(&json!({"history_base":{"thread_id":Uuid::nil()}})).is_err());
    }

    #[test]
    fn cycles_missing_sources_and_deep_lineage_are_bounded() {
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        assert!(ordered(&BTreeMap::from([(a, Some(b))])).is_err());
        assert!(ordered(&BTreeMap::from([(a, Some(b)), (b, Some(a))])).is_err());
        assert_eq!(
            ordered(&BTreeMap::from([(a, Some(b)), (b, None)])).unwrap(),
            vec![b, a]
        );
        let graph = (1..10_001)
            .map(|n| (Uuid::from_u128(n), (n > 1).then(|| Uuid::from_u128(n - 1))))
            .collect();
        assert_eq!(ordered(&graph).unwrap().len(), 10_000);
    }

    #[test]
    fn modern_reference_requires_consistent_typed_bounds() {
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        let mut fields = json!({"history_mode":"paginated","forked_from_id":a,
            "forked_from_ordinal_exclusive":4,"history_base":{"thread_id":a,"end_ordinal_exclusive":4,"end_byte_offset":120}});
        assert_eq!(required_source(&fields).unwrap(), Some(a));
        fields["history_base"]["thread_id"] = b.to_string().into();
        assert!(required_source(&fields).is_err());
        fields["history_base"]["thread_id"] = a.to_string().into();
        fields["history_base"]["end_byte_offset"] = (-1).into();
        assert!(required_source(&fields).is_err());
    }
}
