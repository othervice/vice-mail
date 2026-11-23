//! Stage 9: local rules (SPEC "Lokale Regeln", decision 0020). Class sync; the conditions and
//! actions are one JSON document per rule, the order is `position`.

use mail_model::{
    MAX_RULE_PARTS, MAX_RULE_TEXT, MAX_RULES, MailRule, RuleAction, RuleCondition, RuleId,
    RuleInput,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use crate::StoreError;

/// The stored JSON of a rule: `{"matchAll", "conditions", "actions", "stop"}`.
struct Definition {
    match_all: bool,
    conditions: Vec<RuleCondition>,
    actions: Vec<RuleAction>,
    stop: bool,
}

impl Definition {
    fn parse(text: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(text).ok()?;
        Some(Self {
            match_all: value.get("matchAll")?.as_bool()?,
            conditions: serde_json::from_value(value.get("conditions")?.clone()).ok()?,
            actions: serde_json::from_value(value.get("actions")?.clone()).ok()?,
            stop: value.get("stop")?.as_bool()?,
        })
    }

    fn to_json(&self) -> Result<String, serde_json::Error> {
        Ok(json!({
            "matchAll": self.match_all,
            "conditions": serde_json::to_value(&self.conditions)?,
            "actions": serde_json::to_value(&self.actions)?,
            "stop": self.stop,
        })
        .to_string())
    }
}

const RULES: &str =
    "SELECT uuid, name, enabled, definition, position FROM rule ORDER BY position, id";

fn rule_at(r: &rusqlite::Row<'_>) -> rusqlite::Result<Option<MailRule>> {
    let definition: String = r.get(3)?;
    let Some(definition) = Definition::parse(&definition) else {
        // Written by a newer version (an unknown condition or action): not applied here.
        return Ok(None);
    };
    Ok(Some(MailRule {
        id: RuleId(r.get(0)?),
        name: r.get(1)?,
        enabled: r.get(2)?,
        match_all: definition.match_all,
        conditions: definition.conditions,
        actions: definition.actions,
        stop: definition.stop,
        position: r.get(4)?,
    }))
}

/// All rules this version understands, in order.
pub(crate) fn rules(conn: &Connection) -> Result<Vec<MailRule>, StoreError> {
    let rows = conn
        .prepare_cached(RULES)?
        .query_map([], rule_at)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows.into_iter().flatten().collect())
}

fn check(input: &RuleInput) -> Result<String, StoreError> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > MAX_RULE_TEXT {
        return Err(StoreError::InvalidArgument(format!(
            "a rule name needs 1 to {MAX_RULE_TEXT} characters"
        )));
    }
    if input.actions.is_empty() {
        return Err(StoreError::InvalidArgument("a rule needs an action".into()));
    }
    if input.conditions.len() > MAX_RULE_PARTS || input.actions.len() > MAX_RULE_PARTS {
        return Err(StoreError::InvalidArgument(format!(
            "a rule has at most {MAX_RULE_PARTS} conditions and {MAX_RULE_PARTS} actions"
        )));
    }
    for condition in &input.conditions {
        let value = match condition {
            RuleCondition::From { value, .. }
            | RuleCondition::To { value, .. }
            | RuleCondition::Subject { value, .. }
            | RuleCondition::List { value, .. } => value,
            RuleCondition::Account { .. } => continue,
        };
        if value.trim().is_empty() || value.chars().count() > MAX_RULE_TEXT {
            return Err(StoreError::InvalidArgument(format!(
                "a condition value needs 1 to {MAX_RULE_TEXT} characters"
            )));
        }
    }
    for action in &input.actions {
        if let RuleAction::Move { path, .. } = action
            && path.trim().is_empty()
        {
            return Err(StoreError::InvalidArgument(
                "a move needs a target folder".into(),
            ));
        }
    }
    let definition = Definition {
        match_all: input.match_all,
        conditions: input.conditions.clone(),
        actions: input.actions.clone(),
        stop: input.stop,
    };
    definition
        .to_json()
        .map_err(|err| StoreError::InvalidArgument(format!("rule: {err}")))
}

fn one(conn: &Connection, id: &RuleId) -> Result<MailRule, StoreError> {
    conn.prepare_cached(
        "SELECT uuid, name, enabled, definition, position FROM rule WHERE uuid = ?1",
    )?
    .query_row([&id.0], rule_at)
    .optional()?
    .flatten()
    .ok_or_else(|| StoreError::NotFound(format!("rule {id}")))
}

pub(crate) fn create(
    conn: &Connection,
    uuid: &str,
    input: &RuleInput,
    now: i64,
) -> Result<MailRule, StoreError> {
    let definition = check(input)?;
    let count: i64 = conn.query_row("SELECT count(*) FROM rule", [], |r| r.get(0))?;
    if usize::try_from(count).unwrap_or(usize::MAX) >= MAX_RULES {
        return Err(StoreError::InvalidArgument(format!(
            "at most {MAX_RULES} rules"
        )));
    }
    conn.execute(
        "INSERT INTO rule (uuid, name, position, enabled, definition, created_at, updated_at)
         VALUES (?1, ?2, (SELECT coalesce(max(position), 0) + 1 FROM rule), ?3, ?4, ?5, ?5)",
        params![uuid, input.name.trim(), input.enabled, definition, now],
    )?;
    one(conn, &RuleId(uuid.to_owned()))
}

pub(crate) fn update(
    conn: &Connection,
    id: &RuleId,
    input: &RuleInput,
    now: i64,
) -> Result<MailRule, StoreError> {
    let definition = check(input)?;
    let changed = conn.execute(
        "UPDATE rule SET name = ?2, enabled = ?3, definition = ?4, updated_at = ?5 WHERE uuid = ?1",
        params![id.0, input.name.trim(), input.enabled, definition, now],
    )?;
    if changed == 0 {
        return Err(StoreError::NotFound(format!("rule {id}")));
    }
    one(conn, id)
}

pub(crate) fn delete(conn: &Connection, id: &RuleId) -> Result<bool, StoreError> {
    Ok(conn.execute("DELETE FROM rule WHERE uuid = ?1", [&id.0])? > 0)
}

/// Puts the rules in the order of `ids` (rules not named keep their relative order after them).
pub(crate) fn reorder(
    conn: &Connection,
    ids: &[RuleId],
    now: i64,
) -> Result<Vec<MailRule>, StoreError> {
    let all = rules(conn)?;
    let mut ordered: Vec<&MailRule> = Vec::with_capacity(all.len());
    for id in ids {
        match all.iter().find(|r| &r.id == id) {
            Some(rule) if !ordered.iter().any(|o| o.id == rule.id) => ordered.push(rule),
            Some(_) => {}
            None => return Err(StoreError::NotFound(format!("rule {id}"))),
        }
    }
    for rule in &all {
        if !ordered.iter().any(|o| o.id == rule.id) {
            ordered.push(rule);
        }
    }
    let mut stmt =
        conn.prepare_cached("UPDATE rule SET position = ?2, updated_at = ?3 WHERE uuid = ?1")?;
    for (position, rule) in ordered.iter().enumerate() {
        let position = i64::try_from(position)
            .unwrap_or(i64::MAX)
            .saturating_add(1);
        stmt.execute(params![rule.id.0, position, now])?;
    }
    rules(conn)
}
