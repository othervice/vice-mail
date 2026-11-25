//! Stage 19, task A (decision 0061): the sort keys of a location, computed in Rust
//! (`mail_model::sender_sort_key`, `subject_sort_key`) and called from SQL, so the migration that
//! fills them and every statement that inserts a location compute them the same way.
//!
//! Registered on the write connection only (the readers never write a location): deterministic
//! and innocuous, as SQLite requires for functions in an `UPDATE … FROM` and in indexes' input.

use mail_model::{Address, sender_sort_key, subject_sort_key};
use rusqlite::Connection;
use rusqlite::functions::{Context, FunctionFlags};
use rusqlite::types::ValueRef;

/// `vm_sort_name(name, address)`: the sort key of a person.
pub(crate) const SORT_NAME: &str = "vm_sort_name";
/// `vm_sort_recipient(to_addrs)`: the sort key of the first recipient of `message.to_addrs`
/// (JSON `[{"name", "email"}]`); empty for none or malformed JSON.
pub(crate) const SORT_RECIPIENT: &str = "vm_sort_recipient";
/// `vm_sort_subject(subject)`: the sort key of a subject.
pub(crate) const SORT_SUBJECT: &str = "vm_sort_subject";

/// Text of argument `index`; numbers as text, blobs and NULL as none.
fn text_arg(ctx: &Context<'_>, index: usize) -> Option<String> {
    match ctx.get_raw(index) {
        ValueRef::Text(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
        ValueRef::Integer(n) => Some(n.to_string()),
        ValueRef::Real(n) => Some(n.to_string()),
        ValueRef::Null | ValueRef::Blob(_) => None,
    }
}

/// The key of the first address of a JSON address list.
pub(crate) fn first_recipient_key(json: Option<&str>) -> String {
    let Some(json) = json else {
        return String::new();
    };
    let Ok(addresses) = serde_json::from_str::<Vec<Address>>(json) else {
        return String::new();
    };
    addresses.first().map_or_else(String::new, |a| {
        sender_sort_key(a.name.as_deref(), Some(a.email.as_str()))
    })
}

/// Registers the three functions on `conn`.
pub(crate) fn register(conn: &Connection) -> rusqlite::Result<()> {
    let flags = FunctionFlags::SQLITE_UTF8
        | FunctionFlags::SQLITE_DETERMINISTIC
        | FunctionFlags::SQLITE_INNOCUOUS;
    conn.create_scalar_function(SORT_NAME, 2, flags, |ctx| {
        Ok(sender_sort_key(
            text_arg(ctx, 0).as_deref(),
            text_arg(ctx, 1).as_deref(),
        ))
    })?;
    conn.create_scalar_function(SORT_RECIPIENT, 1, flags, |ctx| {
        Ok(first_recipient_key(text_arg(ctx, 0).as_deref()))
    })?;
    conn.create_scalar_function(SORT_SUBJECT, 1, flags, |ctx| {
        Ok(subject_sort_key(text_arg(ctx, 0).as_deref().unwrap_or("")))
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn functions_compute_the_model_keys() {
        let conn = Connection::open_in_memory().expect("open");
        register(&conn).expect("register");
        let row: (String, String, String, String, String) = conn
            .query_row(
                "SELECT vm_sort_name('Ärger, Anna', 'z@x.org'), vm_sort_name(NULL, 'Bob@X.org'),
                        vm_sort_recipient('[{\"name\": null, \"email\": \"Carl@x.org\"}, {\"name\": \"A\", \"email\": \"a@x.org\"}]'),
                        vm_sort_recipient('not json'), vm_sort_subject('AW: Öl')",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .expect("query");
        assert_eq!(
            row,
            (
                "arger, anna".into(),
                "bob@x.org".into(),
                "carl@x.org".into(),
                String::new(),
                "ol".into()
            )
        );
        let empty: (String, String, String) = conn
            .query_row(
                "SELECT vm_sort_name(NULL, NULL), vm_sort_recipient(NULL), vm_sort_subject(NULL)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("query");
        assert_eq!(empty, (String::new(), String::new(), String::new()));
    }
}
