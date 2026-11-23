//! Templates: a subject and allowlisted HTML with placeholders such as `{{first_name}}`.
//!
//! Placeholders are `{{name}}` or `{{name|fallback}}` (spaces inside allowed). They are replaced
//! in the text of the document model, never in the HTML source and never in link targets: a
//! value is text, escaped when the HTML is written, so a name like `<script>` stays visible text.
//! A placeholder split by formatting (`{{first_` in bold) is not recognised.

use mail_model::Address;

use super::snippet::{SanitizeNotes, SnippetError, check_size, sanitize};
use crate::compose::{Block, Inline, QuoteLanguage, html_blocks, text_blocks, trim_paragraphs};
use crate::contacts::{clean_display_name, split_name};
use crate::text::html_to_text;

/// Largest template body as editor HTML (a sync record holds 256 KiB, `docs/SYNC.md`).
pub const MAX_TEMPLATE_BYTES: usize = 96 * 1024;
/// Longest template subject.
pub const MAX_TEMPLATE_SUBJECT_CHARS: usize = 255;
/// Longest text between `{{` and `}}` that is read as a placeholder.
const MAX_PLACEHOLDER_CHARS: usize = 64;

/// A template as stored.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Template {
    /// Without control characters; may contain placeholders.
    pub subject: String,
    /// Allowlisted editor HTML.
    pub html: String,
}

/// The placeholders a template can use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Placeholder {
    /// First name of the first recipient (`Hans` of `Dr. Hans Müller` or `Müller, Hans`).
    FirstName,
    LastName,
    /// The recipient's display name as it is.
    Name,
    Email,
    /// Today's date in the user's time zone.
    Date,
    /// The sending identity's name.
    MyName,
    MyFirstName,
    MyEmail,
}

impl Placeholder {
    pub const ALL: [Self; 8] = [
        Self::FirstName,
        Self::LastName,
        Self::Name,
        Self::Email,
        Self::Date,
        Self::MyName,
        Self::MyFirstName,
        Self::MyEmail,
    ];

    /// The name written between the braces.
    pub const fn name(self) -> &'static str {
        match self {
            Self::FirstName => "first_name",
            Self::LastName => "last_name",
            Self::Name => "name",
            Self::Email => "email",
            Self::Date => "date",
            Self::MyName => "my_name",
            Self::MyFirstName => "my_first_name",
            Self::MyEmail => "my_email",
        }
    }

    /// Case is ignored.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|p| p.name().eq_ignore_ascii_case(name))
    }
}

/// A calendar date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalDate {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

impl LocalDate {
    /// `23.11.2026` or `23 November 2026`.
    pub fn format(self, language: QuoteLanguage) -> String {
        const MONTHS: [&str; 12] = [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ];
        match language {
            QuoteLanguage::German => format!("{:02}.{:02}.{}", self.day, self.month, self.year),
            QuoteLanguage::English => {
                let month = MONTHS
                    .get(usize::from(self.month).saturating_sub(1))
                    .copied()
                    .unwrap_or("");
                format!("{} {month} {}", self.day, self.year)
            }
        }
    }
}

/// What the placeholders are filled with.
#[derive(Debug, Clone, Copy)]
pub struct TemplateValues<'a> {
    /// The first recipient (To).
    pub recipient: Option<&'a Address>,
    /// The sending identity.
    pub sender: Option<&'a Address>,
    pub date: Option<LocalDate>,
    pub language: QuoteLanguage,
}

/// Something to point out after filling a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateProblem {
    /// `{{name}}` is no known placeholder; it stays as written.
    Unknown(String),
    /// No value (a recipient without a name) and no fallback: replaced by nothing.
    Missing(Placeholder),
}

/// A filled template for the composer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilledTemplate {
    pub subject: String,
    /// Editor HTML.
    pub html: String,
    pub text: String,
    pub problems: Vec<TemplateProblem>,
}

impl Template {
    /// From the template editor's subject and HTML, sanitised with the mail HTML allowlist.
    pub fn from_html(subject: &str, html: &str) -> Result<(Self, SanitizeNotes), SnippetError> {
        let (blocks, notes) = sanitize(html, MAX_TEMPLATE_BYTES)?;
        let html = check_size(&blocks, MAX_TEMPLATE_BYTES)?;
        Ok((
            Self {
                subject: clean_subject(subject),
                html,
            },
            notes,
        ))
    }

    /// From a subject and plain text.
    pub fn from_text(subject: &str, text: &str) -> Result<Self, SnippetError> {
        let mut blocks = text_blocks(text);
        trim_paragraphs(&mut blocks);
        let html = check_size(&blocks, MAX_TEMPLATE_BYTES)?;
        Ok(Self {
            subject: clean_subject(subject),
            html,
        })
    }

    /// The placeholders the template uses, and unknown ones (for the editor).
    pub fn placeholders(&self) -> (Vec<Placeholder>, Vec<String>) {
        let mut known = Vec::new();
        let mut unknown = Vec::new();
        let mut collect = |text: &str| {
            for token in tokens(text) {
                match token {
                    Token::Known(p, _) if !known.contains(&p) => known.push(p),
                    Token::Unknown(name) if !unknown.contains(&name) => unknown.push(name),
                    _ => {}
                }
            }
        };
        collect(&self.subject);
        for_each_text(&mut html_blocks(&self.html), &mut |text| {
            collect(text);
        });
        (known, unknown)
    }
}

/// Fills the placeholders (see module docs).
pub fn fill_template(template: &Template, values: &TemplateValues<'_>) -> FilledTemplate {
    let resolved = Resolved::new(values);
    let mut problems = Vec::new();
    let subject = clean_subject(&replace(&template.subject, &resolved, &mut problems));
    let mut blocks = html_blocks(&template.html);
    for_each_text(&mut blocks, &mut |text| {
        *text = replace(text, &resolved, &mut problems);
    });
    let html = crate::compose::fragment(&blocks);
    let text = html_to_text(&html).trim_end().to_owned();
    problems.dedup();
    let mut unique = Vec::new();
    for problem in problems {
        if !unique.contains(&problem) {
            unique.push(problem);
        }
    }
    FilledTemplate {
        subject,
        html,
        text,
        problems: unique,
    }
}

struct Resolved {
    first_name: Option<String>,
    last_name: Option<String>,
    name: Option<String>,
    email: Option<String>,
    date: Option<String>,
    my_name: Option<String>,
    my_first_name: Option<String>,
    my_email: Option<String>,
}

impl Resolved {
    fn new(values: &TemplateValues<'_>) -> Self {
        let name_of = |a: Option<&Address>| {
            a.and_then(|a| {
                a.name
                    .as_deref()
                    .and_then(|n| clean_display_name(n, &a.email))
            })
        };
        let email_of = |a: Option<&Address>| {
            a.map(|a| a.email.trim().to_owned())
                .filter(|e| !e.is_empty())
        };
        let recipient = name_of(values.recipient);
        let split = recipient.as_deref().map(split_name).unwrap_or_default();
        let me = name_of(values.sender);
        Self {
            first_name: split.first,
            last_name: split.last,
            name: recipient,
            email: email_of(values.recipient),
            date: values.date.map(|d| d.format(values.language)),
            my_first_name: me.as_deref().and_then(|n| split_name(n).first),
            my_name: me,
            my_email: email_of(values.sender),
        }
    }

    fn get(&self, placeholder: Placeholder) -> Option<&str> {
        match placeholder {
            Placeholder::FirstName => self.first_name.as_deref(),
            Placeholder::LastName => self.last_name.as_deref(),
            Placeholder::Name => self.name.as_deref(),
            Placeholder::Email => self.email.as_deref(),
            Placeholder::Date => self.date.as_deref(),
            Placeholder::MyName => self.my_name.as_deref(),
            Placeholder::MyFirstName => self.my_first_name.as_deref(),
            Placeholder::MyEmail => self.my_email.as_deref(),
        }
    }
}

enum Token<'t> {
    Text(&'t str),
    /// A known placeholder and its fallback.
    Known(Placeholder, Option<&'t str>),
    /// Something name-like in braces that is no placeholder.
    Unknown(String),
    /// Braces that hold no name; kept as written.
    Literal(&'t str),
}

/// Splits text into plain text and `{{…}}` placeholders.
fn tokens(text: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(len) = after.find("}}") else {
            break;
        };
        if start > 0 {
            out.push(Token::Text(&rest[..start]));
        }
        let whole = &rest[start..start + 2 + len + 2];
        let inner = &after[..len];
        let (name, fallback) = match inner.split_once('|') {
            Some((name, fallback)) => (name.trim(), Some(fallback.trim())),
            None => (inner.trim(), None),
        };
        let name_like = !name.is_empty()
            && inner.chars().count() <= MAX_PLACEHOLDER_CHARS
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        out.push(match Placeholder::parse(name) {
            Some(p) if name_like => Token::Known(p, fallback),
            _ if name_like => Token::Unknown(name.to_ascii_lowercase()),
            _ => Token::Literal(whole),
        });
        rest = &after[len + 2..];
    }
    if !rest.is_empty() {
        out.push(Token::Text(rest));
    }
    out
}

fn replace(text: &str, values: &Resolved, problems: &mut Vec<TemplateProblem>) -> String {
    let mut out = String::with_capacity(text.len());
    for token in tokens(text) {
        match token {
            Token::Text(t) | Token::Literal(t) => out.push_str(t),
            Token::Known(placeholder, fallback) => match (values.get(placeholder), fallback) {
                (Some(value), _) => out.extend(value.chars().filter(|c| !c.is_control())),
                (None, Some(fallback)) => out.push_str(fallback),
                (None, None) => problems.push(TemplateProblem::Missing(placeholder)),
            },
            Token::Unknown(name) => {
                out.push_str("{{");
                out.push_str(&name);
                out.push_str("}}");
                problems.push(TemplateProblem::Unknown(name));
            }
        }
    }
    out
}

/// Calls `f` on every text run of the blocks (paragraph text and preformatted text).
fn for_each_text(blocks: &mut [Block], f: &mut dyn FnMut(&mut String)) {
    for block in blocks {
        match block {
            Block::Paragraph(inlines)
            | Block::AlignedParagraph(_, inlines)
            | Block::Heading { inlines, .. } => {
                for inline in inlines {
                    if let Inline::Text(text, _) = inline {
                        f(text);
                    }
                }
            }
            Block::Pre(text) => f(text),
            Block::List { items, .. } => {
                for item in items {
                    for_each_text(item, f);
                }
            }
            Block::Quote(children) => for_each_text(children, f),
        }
    }
}

/// Control characters become spaces, runs of white space one space; trimmed and cut.
fn clean_subject(subject: &str) -> String {
    let mapped: String = subject
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    mapped
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_TEMPLATE_SUBJECT_CHARS)
        .collect()
}
