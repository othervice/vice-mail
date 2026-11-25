//! Stage 19, task A (decisions 0061 and 0062): sorting a list view, its table presentation and
//! the quick filter. Types and the pure sort keys; the store keeps the keys next to every
//! location, the engine resolves the view and compiles the filter.
//!
//! **Sort keys.** What a list sorts by, computed once per location when it is stored (the store
//! calls [`sender_sort_key`] and [`subject_sort_key`] from SQL): the name a row shows (display
//! name, else the address), the first recipient the same way, the subject without reply and
//! forward prefixes, each folded with [`sort_text`]; and the message's size. Folding follows
//! DIN 5007-1 (`ä` sorts as `a`, `ß` as `ss`): lower case, accents of Latin letters removed,
//! leading punctuation dropped, at most [`SORT_KEY_CHARS`] characters. Ties sort by date in the
//! same direction.

use serde::{Deserialize, Serialize};
use specta_typescript::Number;

use crate::{InboxScope, MessageId, MessageListItem, ThreadListItem};

// ---------------------------------------------------------------------------------------------
// Sort keys

/// Longest sort key: longer names and subjects tie on their first 40 characters and then sort
/// by date. Keeps the four sort indexes small (decision 0061).
pub const SORT_KEY_CHARS: usize = 40;

/// The base letter(s) of a Latin letter with an accent or a ligature, lower case; `None` for
/// every other character. Covers Latin-1 Supplement and Latin Extended-A, which is every
/// letter of the Western, Central and Northern European languages.
fn fold_latin(c: char) -> Option<&'static str> {
    let folded = match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' | 'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å'
        | 'Ā' | 'Ă' | 'Ą' => "a",
        'æ' | 'Æ' => "ae",
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' | 'Ç' | 'Ć' | 'Ĉ' | 'Ċ' | 'Č' => "c",
        'ď' | 'đ' | 'ð' | 'Ď' | 'Đ' | 'Ð' => "d",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' | 'È' | 'É' | 'Ê' | 'Ë' | 'Ē' | 'Ĕ'
        | 'Ė' | 'Ę' | 'Ě' => "e",
        'ĝ' | 'ğ' | 'ġ' | 'ģ' | 'Ĝ' | 'Ğ' | 'Ġ' | 'Ģ' => "g",
        'ĥ' | 'ħ' | 'Ĥ' | 'Ħ' => "h",
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' | 'Ì' | 'Í' | 'Î' | 'Ï' | 'Ĩ' | 'Ī'
        | 'Ĭ' | 'Į' | 'İ' => "i",
        'ĳ' | 'Ĳ' => "ij",
        'ĵ' | 'Ĵ' => "j",
        'ķ' | 'Ķ' => "k",
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' | 'Ĺ' | 'Ļ' | 'Ľ' | 'Ŀ' | 'Ł' => "l",
        'ñ' | 'ń' | 'ņ' | 'ň' | 'ŉ' | 'Ñ' | 'Ń' | 'Ņ' | 'Ň' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' | 'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø'
        | 'Ō' | 'Ŏ' | 'Ő' => "o",
        'œ' | 'Œ' => "oe",
        'ŕ' | 'ŗ' | 'ř' | 'Ŕ' | 'Ŗ' | 'Ř' => "r",
        'ś' | 'ŝ' | 'ş' | 'š' | 'ș' | 'ſ' | 'Ś' | 'Ŝ' | 'Ş' | 'Š' | 'Ș' => "s",
        'ß' | 'ẞ' => "ss",
        'ţ' | 'ť' | 'ŧ' | 'ț' | 'Ţ' | 'Ť' | 'Ŧ' | 'Ț' => "t",
        'þ' | 'Þ' => "th",
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' | 'Ù' | 'Ú' | 'Û' | 'Ü' | 'Ũ'
        | 'Ū' | 'Ŭ' | 'Ů' | 'Ű' | 'Ų' => "u",
        'ŵ' | 'Ŵ' => "w",
        'ý' | 'ÿ' | 'ŷ' | 'Ý' | 'Ÿ' | 'Ŷ' => "y",
        'ź' | 'ż' | 'ž' | 'Ź' | 'Ż' | 'Ž' => "z",
        _ => return None,
    };
    Some(folded)
}

/// Combining diacritical marks (U+0300–U+036F): a decomposed `u` + `¨` sorts as `u`.
fn is_combining_mark(c: char) -> bool {
    ('\u{0300}'..='\u{036F}').contains(&c)
}

/// `text` as a sort key: lower case, accents of Latin letters removed (`Müller` → `muller`,
/// `Groß` → `gross`), whitespace and control characters collapsed to one space, punctuation and
/// symbols before the first letter or digit dropped (`"Bob"` sorts under b), at most
/// [`SORT_KEY_CHARS`] characters. Other scripts keep their letters in lower case.
pub fn sort_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len().min(SORT_KEY_CHARS * 4));
    let mut chars = 0usize;
    let mut started = false;
    let mut space = false;
    let push = |out: &mut String, s: &str, chars: &mut usize| {
        for c in s.chars() {
            if *chars < SORT_KEY_CHARS {
                out.push(c);
                *chars += 1;
            }
        }
    };
    for c in text.chars() {
        if chars >= SORT_KEY_CHARS {
            break;
        }
        if c.is_whitespace() || c.is_control() {
            space = started;
            continue;
        }
        if is_combining_mark(c) || crate::text::is_bidi_control(c) {
            continue;
        }
        if !started && !c.is_alphanumeric() {
            continue;
        }
        if space {
            push(&mut out, " ", &mut chars);
            space = false;
        }
        started = true;
        if let Some(folded) = fold_latin(c) {
            push(&mut out, folded, &mut chars);
        } else {
            for lower in c.to_lowercase() {
                let mut buffer = [0u8; 4];
                push(&mut out, lower.encode_utf8(&mut buffer), &mut chars);
            }
        }
    }
    out
}

/// The sort key of a person as a row names them: the display name, else the address. A name of
/// nothing but punctuation counts as none.
pub fn sender_sort_key(name: Option<&str>, address: Option<&str>) -> String {
    let by_name = name.map(sort_text).unwrap_or_default();
    if by_name.is_empty() {
        address.map(sort_text).unwrap_or_default()
    } else {
        by_name
    }
}

/// The sort key of a subject: without reply and forward prefixes (`Re:`, `AW:`, `Fwd:`, …,
/// [`crate::threading::display_subject`]), folded with [`sort_text`]. Empty for no subject.
pub fn subject_sort_key(subject: &str) -> String {
    sort_text(crate::threading::display_subject(subject))
}

// ---------------------------------------------------------------------------------------------
// Order

/// What a list view sorts by (decision 0061).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "camelCase")]
pub enum SortKey {
    /// The sort date (the default).
    #[default]
    Date,
    /// The name the row shows for the sender (display name, else address).
    Sender,
    /// The first recipient, the same way (offered in Sent and Drafts).
    Recipient,
    /// The subject without reply and forward prefixes.
    Subject,
    /// The message's size in bytes.
    Size,
}

/// The order of a list view: a key and its direction. The default is newest first.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "camelCase")]
pub struct ListOrder {
    pub key: SortKey,
    /// Ascending (A to Z, oldest first, smallest first); false: descending.
    pub ascending: bool,
}

impl ListOrder {
    /// Newest first: every list's own order before stage 19.
    pub const fn is_default(self) -> bool {
        matches!(self.key, SortKey::Date) && !self.ascending
    }
}

/// Most views that keep an order of their own (`setListOrder`).
pub const MAX_LIST_ORDERS: usize = 1000;

/// The order the user chose for one view (its key as for "Unread first": `mailbox:<id>`,
/// `unified:all`, `group:<id>`). Views not listed are newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ViewOrder {
    pub view: String,
    pub order: ListOrder,
}

// ---------------------------------------------------------------------------------------------
// Quick filter

/// Longest text of a quick filter, as for a search query (bytes).
pub const MAX_QUICK_FILTER_TEXT: usize = 2048;

/// Stage 19 (decision 0062): the quick filter above a list. Every part that is set narrows the
/// list (AND). `text` is search language (decision 0013): words match sender, recipients,
/// subject and text as in the search, operators such as `from:` work too; its last word matches
/// as a prefix (search as you type).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct QuickFilter {
    pub unread: bool,
    pub flagged: bool,
    pub attachment: bool,
    /// The sender is one of the user's contacts: a person the user has written to, or whose
    /// contact the user has named, noted or merged (decision 0062).
    pub contacts: bool,
    /// A tag (IMAP keyword) of the message in this view, compared without ASCII case.
    pub tag: Option<String>,
    pub text: String,
    /// The user's region writes dates month first (as in [`crate::SearchRequest`]).
    pub month_first: bool,
}

impl QuickFilter {
    /// Nothing is set: the list is the view's own.
    pub fn is_empty(&self) -> bool {
        !self.unread
            && !self.flagged
            && !self.attachment
            && !self.contacts
            && self.tag.is_none()
            && self.text.trim().is_empty()
    }

    /// Checks what the UI sent: the text's length, and that a tag is a keyword.
    pub fn validate(&self) -> Result<(), String> {
        if self.text.len() > MAX_QUICK_FILTER_TEXT {
            return Err(format!(
                "quick filter text is longer than {MAX_QUICK_FILTER_TEXT} bytes"
            ));
        }
        if let Some(tag) = &self.tag
            && !crate::is_valid_keyword(tag)
        {
            return Err(format!("{tag:?} is not a tag (IMAP keyword)"));
        }
        Ok(())
    }
}

/// One page request of a sorted or filtered list view: which view, in what order, with which
/// quick filter. The view's own list (newest first, no filter) has its own commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ListViewRequest {
    /// A mailbox (a folder or an account's inbox) or a unified view. The inbox rules (waiting
    /// snoozes and blocked senders hidden, decision 0036) apply to inboxes and unified views.
    pub scope: InboxScope,
    pub order: ListOrder,
    #[serde(default)]
    pub filter: QuickFilter,
}

/// Where a sorted list view continues: after the row with this key and message id. Pass the
/// page's `nextCursor` back unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ViewCursor {
    /// The row's sort key (sender, recipient, subject); null for date and size.
    pub text: Option<String>,
    /// The row's size (size order); null otherwise.
    #[specta(type = Option<Number>)]
    pub number: Option<i64>,
    pub id: MessageId,
}

/// One page of a sorted or filtered list view, one row per message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ViewMessagePage {
    pub items: Vec<MessageListItem>,
    /// Null on the last page.
    pub next_cursor: Option<ViewCursor>,
}

/// One page of a sorted or filtered list view, one row per conversation (decision 0039): the
/// conversation's newest message in the view that matches the filter stands for it and gives it
/// its place in the order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ViewThreadPage {
    pub items: Vec<ThreadListItem>,
    /// Null on the last page.
    pub next_cursor: Option<ViewCursor>,
}

/// Most tags `listViewTags` names.
pub const MAX_VIEW_TAGS: usize = 200;

// ---------------------------------------------------------------------------------------------
// Table presentation

/// How the wide list is drawn (decision 0061): rows (the default) or a table with columns.
/// Narrow lists (beside the reading pane) always draw rows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ListPresentation {
    #[default]
    Rows,
    Table,
}

/// A column of the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum TableColumnId {
    /// Flag mark.
    Flag,
    /// Attachment mark.
    Attachment,
    /// Who the row names: the sender, in Sent and Drafts the recipients.
    Correspondent,
    /// The recipients.
    Recipient,
    /// Subject (takes the width that is left).
    Subject,
    /// The account, in lists that mix accounts.
    Account,
    Date,
    Size,
}

impl TableColumnId {
    /// Every column in the table's order.
    pub const ALL: [Self; 8] = [
        Self::Flag,
        Self::Attachment,
        Self::Correspondent,
        Self::Recipient,
        Self::Subject,
        Self::Account,
        Self::Date,
        Self::Size,
    ];

    /// Shown until the user chooses.
    pub const fn shown_by_default(self) -> bool {
        !matches!(self, Self::Recipient)
    }

    /// Width until the user drags it, CSS pixels at 100 % text size. The subject takes what is
    /// left; its value is its smallest width.
    pub const fn default_width(self) -> u16 {
        match self {
            Self::Flag | Self::Attachment => 32,
            Self::Correspondent | Self::Recipient => 200,
            Self::Subject => 160,
            Self::Account => 120,
            Self::Date => 136,
            Self::Size => 88,
        }
    }

    /// Whether the user can change the width (the mark columns are as wide as their symbol).
    pub const fn resizable(self) -> bool {
        !matches!(self, Self::Flag | Self::Attachment)
    }
}

/// Narrowest column the user can drag.
pub const MIN_TABLE_COLUMN_WIDTH: u16 = 48;
/// Widest column the user can drag.
pub const MAX_TABLE_COLUMN_WIDTH: u16 = 800;

/// One column of the table: shown or not, and its width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TableColumn {
    pub id: TableColumnId,
    pub visible: bool,
    /// CSS pixels at 100 % text size, [`MIN_TABLE_COLUMN_WIDTH`] to [`MAX_TABLE_COLUMN_WIDTH`];
    /// the mark columns keep their own.
    pub width: u16,
}

/// The list's presentation and the table's columns, stored on this device like the window's
/// layout sizes (decision 0025: screens differ).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ListTableSettings {
    pub presentation: ListPresentation,
    /// Every column once, in the table's order ([`TableColumnId::ALL`]).
    pub columns: Vec<TableColumn>,
}

impl Default for ListTableSettings {
    fn default() -> Self {
        Self {
            presentation: ListPresentation::Rows,
            columns: TableColumnId::ALL
                .iter()
                .map(|id| TableColumn {
                    id: *id,
                    visible: id.shown_by_default(),
                    width: id.default_width(),
                })
                .collect(),
        }
    }
}

impl ListTableSettings {
    /// Checks what the UI sent: every column at most once, widths within the limits, the subject
    /// shown (the table needs a column that takes the rest).
    pub fn validate(&self) -> Result<(), String> {
        for (index, column) in self.columns.iter().enumerate() {
            if self.columns[..index].iter().any(|c| c.id == column.id) {
                return Err(format!("column {:?} is listed twice", column.id));
            }
            if column.id.resizable()
                && !(MIN_TABLE_COLUMN_WIDTH..=MAX_TABLE_COLUMN_WIDTH).contains(&column.width)
            {
                return Err(format!(
                    "column {:?}: width {} is outside {MIN_TABLE_COLUMN_WIDTH} to {MAX_TABLE_COLUMN_WIDTH} pixels",
                    column.id, column.width
                ));
            }
            if column.id == TableColumnId::Subject && !column.visible {
                return Err("the subject column is always shown".into());
            }
        }
        Ok(())
    }

    /// A stored value as the UI may use it: unknown or repeated columns dropped, missing ones
    /// added with their defaults, widths outside the limits back at their default, the subject
    /// shown, every column in the table's order.
    #[must_use]
    pub fn normalized(self) -> Self {
        let columns = TableColumnId::ALL
            .iter()
            .map(|id| {
                let stored = self.columns.iter().find(|c| c.id == *id);
                let mut column = stored.copied().unwrap_or(TableColumn {
                    id: *id,
                    visible: id.shown_by_default(),
                    width: id.default_width(),
                });
                if !id.resizable()
                    || !(MIN_TABLE_COLUMN_WIDTH..=MAX_TABLE_COLUMN_WIDTH).contains(&column.width)
                {
                    column.width = id.default_width();
                }
                if *id == TableColumnId::Subject {
                    column.visible = true;
                }
                column
            })
            .collect();
        Self {
            presentation: self.presentation,
            columns,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_text_folds_case_accents_and_leading_punctuation() {
        assert_eq!(sort_text("Müller"), "muller");
        assert_eq!(sort_text("MÜLLER"), "muller");
        assert_eq!(sort_text("Mu\u{308}ller"), "muller", "decomposed");
        assert_eq!(sort_text("Groß"), "gross");
        assert_eq!(sort_text("Łukasz Wiśniewski"), "lukasz wisniewski");
        assert_eq!(sort_text("Søren Ærø"), "soren aero");
        assert_eq!(sort_text("Zoë  Ångström\t"), "zoe angstrom");
        assert_eq!(sort_text("\"Bob\" (Work)"), "bob\" (work)");
        assert_eq!(sort_text("  ...   "), "");
        assert_eq!(sort_text(""), "");
        assert_eq!(
            sort_text("Иванова"),
            "иванова",
            "other scripts in lower case"
        );
        assert_eq!(sort_text("İstanbul"), "istanbul");
        assert_eq!(sort_text("a\u{202E}b"), "ab", "bidi controls do not count");
        assert_eq!(sort_text("Ünal\u{0}\nX"), "unal x");
    }

    #[test]
    fn sort_text_is_cut_at_forty_characters() {
        let long = "x".repeat(100);
        assert_eq!(sort_text(&long).chars().count(), SORT_KEY_CHARS);
        // A ligature that spells two letters is cut inside, never over the limit.
        let edge = format!("{}ß", "a".repeat(SORT_KEY_CHARS - 1));
        assert_eq!(
            sort_text(&edge),
            format!("{}s", "a".repeat(SORT_KEY_CHARS - 1))
        );
        let multi = "ü".repeat(60);
        assert_eq!(sort_text(&multi), "u".repeat(SORT_KEY_CHARS));
    }

    #[test]
    fn umlauts_sort_with_their_base_letter() {
        let mut names = ["Zander", "Österreich", "Otto", "ozelot", "Bär", "Äpfel"];
        names.sort_by_key(|n| sort_text(n));
        assert_eq!(
            names,
            ["Äpfel", "Bär", "Österreich", "Otto", "ozelot", "Zander"]
        );
        // `Apfel` and `Äpfel` are the same key: the date decides between them.
        assert_eq!(sort_text("Äpfel"), sort_text("Apfel"));
    }

    #[test]
    fn sender_key_prefers_the_name() {
        assert_eq!(
            sender_sort_key(Some("Anna Weber"), Some("z@example.org")),
            "anna weber"
        );
        assert_eq!(
            sender_sort_key(Some("  "), Some("Zed@Example.org")),
            "zed@example.org"
        );
        assert_eq!(
            sender_sort_key(Some("\"\""), Some("zed@example.org")),
            "zed@example.org"
        );
        assert_eq!(
            sender_sort_key(None, Some("bob@example.org")),
            "bob@example.org"
        );
        assert_eq!(sender_sort_key(None, None), "");
    }

    #[test]
    fn subject_key_drops_reply_prefixes() {
        assert_eq!(subject_sort_key("Re: AW: Angebot"), "angebot");
        assert_eq!(
            subject_sort_key("WG: [EXTERN] Rechnung"),
            "extern] rechnung"
        );
        assert_eq!(subject_sort_key("Re:"), "");
        assert_eq!(subject_sort_key(""), "");
        assert_eq!(subject_sort_key("Über uns"), "uber uns");
    }

    #[test]
    fn default_order_is_newest_first() {
        assert!(ListOrder::default().is_default());
        assert!(
            !ListOrder {
                key: SortKey::Date,
                ascending: true
            }
            .is_default()
        );
        let json = serde_json::to_value(ListOrder::default()).expect("json");
        assert_eq!(json, serde_json::json!({"key": "date", "ascending": false}));
    }

    #[test]
    fn quick_filter_checks_text_and_tag() {
        assert!(QuickFilter::default().is_empty());
        assert!(QuickFilter::default().validate().is_ok());
        let text = QuickFilter {
            text: "  ".into(),
            ..QuickFilter::default()
        };
        assert!(text.is_empty(), "blank text filters nothing");
        let tag = QuickFilter {
            tag: Some("$label1".into()),
            ..QuickFilter::default()
        };
        assert!(!tag.is_empty());
        assert!(tag.validate().is_ok());
        let bad = QuickFilter {
            tag: Some("two words".into()),
            ..QuickFilter::default()
        };
        assert!(bad.validate().is_err());
        let long = QuickFilter {
            text: "x".repeat(MAX_QUICK_FILTER_TEXT + 1),
            ..QuickFilter::default()
        };
        assert!(long.validate().is_err());
        // Fields the UI leaves out take their defaults.
        let parsed: QuickFilter = serde_json::from_str(r#"{"unread": true}"#).expect("json");
        assert!(parsed.unread && !parsed.flagged && parsed.text.is_empty());
    }

    #[test]
    fn table_settings_are_checked_and_normalized() {
        let defaults = ListTableSettings::default();
        assert!(defaults.validate().is_ok());
        assert_eq!(defaults.columns.len(), TableColumnId::ALL.len());
        assert_eq!(defaults.clone().normalized(), defaults);

        let mut twice = defaults.clone();
        twice.columns.push(twice.columns[0]);
        assert!(twice.validate().is_err());

        let mut narrow = defaults.clone();
        narrow.columns[2].width = MIN_TABLE_COLUMN_WIDTH - 1;
        assert!(narrow.validate().is_err());
        let fixed = narrow.normalized();
        assert_eq!(
            fixed.columns[2].width,
            TableColumnId::Correspondent.default_width()
        );

        let mut no_subject = defaults.clone();
        if let Some(subject) = no_subject
            .columns
            .iter_mut()
            .find(|c| c.id == TableColumnId::Subject)
        {
            subject.visible = false;
        }
        assert!(no_subject.validate().is_err());
        assert!(
            no_subject
                .normalized()
                .columns
                .iter()
                .any(|c| c.id == TableColumnId::Subject && c.visible)
        );

        // Missing and reordered columns come back in the table's order.
        let partial = ListTableSettings {
            presentation: ListPresentation::Table,
            columns: vec![TableColumn {
                id: TableColumnId::Size,
                visible: false,
                width: 120,
            }],
        }
        .normalized();
        assert_eq!(partial.presentation, ListPresentation::Table);
        let ids: Vec<_> = partial.columns.iter().map(|c| c.id).collect();
        assert_eq!(ids, TableColumnId::ALL);
        let size = partial.columns.last().expect("size");
        assert!(!size.visible);
        assert_eq!(size.width, 120);
    }
}
