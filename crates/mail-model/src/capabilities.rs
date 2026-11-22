//! What an IMAP server can do, as announced in CAPABILITY after login.

use serde::Serialize;

/// Server capabilities the engine and the UI ask about. Nothing is faked: what a server lacks
/// is hidden or explained (`docs/ARCHITECTURE.md`). Custom keywords are a per-mailbox property
/// (`PERMANENTFLAGS \*`), see [`crate::SelectedMailbox::keywords_allowed`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub imap4rev1: bool,
    /// RFC 2177. Without it, new mail is found by polling.
    pub idle: bool,
    /// RFC 7162. Without it, flag changes are found by re-reading flags window by window.
    pub condstore: bool,
    pub qresync: bool,
    pub enable: bool,
    /// RFC 6851 (`MOVE`), stage 4.
    #[serde(rename = "move")]
    pub move_: bool,
    /// RFC 4315, stage 4 and 5.
    pub uidplus: bool,
    /// RFC 6154, folder roles.
    pub special_use: bool,
    /// RFC 5819, all folder counters in one round trip.
    pub list_status: bool,
    /// RFC 7888, saves a round trip per APPEND.
    pub literal_plus: bool,
    /// RFC 4731, compact `UID SEARCH` results.
    pub esearch: bool,
    /// Every announced capability, upper case, sorted, without duplicates.
    pub names: Vec<String>,
}

impl Capabilities {
    /// Parses capability names as the server sent them (any case, `AUTH=` entries included).
    pub fn from_names<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut names: Vec<String> = names
            .into_iter()
            .map(|n| n.as_ref().trim().to_ascii_uppercase())
            .filter(|n| !n.is_empty())
            .collect();
        names.sort();
        names.dedup();
        let has = |n: &str| names.binary_search_by(|x| x.as_str().cmp(n)).is_ok();
        Self {
            imap4rev1: has("IMAP4REV1"),
            idle: has("IDLE"),
            // QRESYNC implies CONDSTORE (RFC 7162, section 3.2.3).
            condstore: has("CONDSTORE") || has("QRESYNC"),
            qresync: has("QRESYNC"),
            enable: has("ENABLE"),
            move_: has("MOVE"),
            uidplus: has("UIDPLUS"),
            special_use: has("SPECIAL-USE"),
            list_status: has("LIST-STATUS"),
            literal_plus: has("LITERAL+"),
            esearch: has("ESEARCH"),
            names,
        }
    }

    /// Whether the server announced `name` (case-insensitive).
    pub fn has(&self, name: &str) -> bool {
        let upper = name.to_ascii_uppercase();
        self.names.binary_search(&upper).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_names_case_insensitively() {
        let caps = Capabilities::from_names([
            "IMAP4rev1",
            "idle",
            "Condstore",
            "MOVE",
            "UIDPLUS",
            "SPECIAL-USE",
            "LIST-STATUS",
            "AUTH=PLAIN",
            "literal+",
            "idle",
        ]);
        assert!(caps.imap4rev1 && caps.idle && caps.condstore && caps.move_ && caps.uidplus);
        assert!(caps.special_use && caps.list_status && caps.literal_plus);
        assert!(!caps.qresync && !caps.enable && !caps.esearch);
        assert!(caps.has("auth=plain"));
        assert_eq!(caps.names.iter().filter(|n| *n == "IDLE").count(), 1);
        assert!(caps.names.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn qresync_implies_condstore() {
        let caps = Capabilities::from_names(["IMAP4REV1", "QRESYNC"]);
        assert!(caps.qresync && caps.condstore);
        assert!(!Capabilities::from_names(["IMAP4REV1"]).condstore);
    }

    #[test]
    fn move_is_named_move_on_the_wire() {
        let json = serde_json::to_value(Capabilities::from_names(["MOVE"])).expect("serialize");
        assert_eq!(json["move"], true);
        assert_eq!(json["specialUse"], false);
    }
}
