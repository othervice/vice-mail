//! Deterministic synthetic mail data: five accounts, several folders each, Zipf-distributed
//! senders and a mixed German/English Zipf vocabulary with umlauts, numbers and tracking URLs.
//! Every message is derived from its sequence number, so it can be regenerated identically.

use crate::rng::{Rng, Zipf, hash64};
use std::collections::HashSet;

pub const START: i64 = 1_546_300_800; // 2019-01-01 00:00 UTC
pub const END: i64 = 1_795_348_800; // 2026-11-22 12:00 UTC
pub const DAY: i64 = 86_400;
pub const ACCOUNT_SIZES: [usize; 5] = [250_000, 120_000, 80_000, 40_000, 10_000];
const CUSTOM_PER_ACCOUNT: [usize; 5] = [15, 12, 9, 6, 4];
pub const VOCAB_SIZE: usize = 50_000;
const SENDERS: usize = 20_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Inbox,
    Sent,
    Drafts,
    Archive,
    Trash,
    Junk,
    Custom,
}

impl Role {
    pub fn as_str(self) -> Option<&'static str> {
        match self {
            Role::Inbox => Some("inbox"),
            Role::Sent => Some("sent"),
            Role::Drafts => Some("drafts"),
            Role::Archive => Some("archive"),
            Role::Trash => Some("trash"),
            Role::Junk => Some("junk"),
            Role::Custom => None,
        }
    }
}

pub struct MailboxDef {
    pub name: String,
    pub role: Role,
    pub weight: f64,
}

const CUSTOM_NAMES: [&str; 15] = [
    "Newsletter",
    "Rechnungen",
    "Projekte",
    "Projekte/Alpha",
    "Projekte/Beta",
    "Familie",
    "Reisen",
    "Verein",
    "Steuer/2023",
    "Steuer/2024",
    "Bestellungen",
    "Kunden",
    "Kunden/Müller GmbH",
    "Lieferanten",
    "Bewerbungen",
];

/// Folders of one account in sync order (Inbox first).
pub fn mailbox_defs(account: usize) -> Vec<MailboxDef> {
    let mut v = vec![
        MailboxDef { name: "INBOX".into(), role: Role::Inbox, weight: 0.50 },
        MailboxDef { name: "Sent".into(), role: Role::Sent, weight: 0.12 },
        MailboxDef { name: "Drafts".into(), role: Role::Drafts, weight: 0.005 },
        MailboxDef { name: "Archive".into(), role: Role::Archive, weight: 0.20 },
    ];
    let k = CUSTOM_PER_ACCOUNT[account];
    for (i, name) in CUSTOM_NAMES.iter().take(k).enumerate() {
        let weight = if i == 0 { 0.06 } else { 0.08 / (k - 1) as f64 };
        v.push(MailboxDef { name: (*name).into(), role: Role::Custom, weight });
    }
    v.push(MailboxDef { name: "Trash".into(), role: Role::Trash, weight: 0.02 });
    v.push(MailboxDef { name: "Junk".into(), role: Role::Junk, weight: 0.015 });
    v
}

#[derive(Clone, Copy)]
pub struct LocMeta {
    pub mailbox: u16,
    pub seen: bool,
    pub flagged: bool,
    pub answered: bool,
}

pub struct Meta {
    pub seq: u64,
    pub date: i64,
    pub locs: [Option<LocMeta>; 2],
    /// Body text is cached (and therefore indexed) in the "realistic" scenario.
    pub has_body: bool,
    pub sent: bool,
}

pub fn account_base_seq(account: usize) -> u64 {
    ACCOUNT_SIZES[..account].iter().sum::<usize>() as u64
}

/// Folder placement, dates and flags for every message of one account.
pub fn plan_account(account: usize, defs: &[MailboxDef], scale: f64) -> Vec<Meta> {
    let n = (ACCOUNT_SIZES[account] as f64 * scale) as usize;
    let base = account_base_seq(account);
    let total_w: f64 = defs.iter().map(|d| d.weight).sum();
    let secondary_targets: Vec<usize> = defs
        .iter()
        .enumerate()
        .filter(|(_, d)| matches!(d.role, Role::Custom | Role::Archive))
        .map(|(i, _)| i)
        .collect();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let seq = base + i as u64;
        let mut rng = Rng::new(seq.wrapping_mul(7919).wrapping_add(1));
        let date = START + ((END - START) as f64 * rng.f64().powf(0.6)) as i64;
        let age = END - date;
        let mut x = rng.f64() * total_w;
        let mut primary = defs.len() - 1;
        for (j, d) in defs.iter().enumerate() {
            if x < d.weight {
                primary = j;
                break;
            }
            x -= d.weight;
        }
        let mk = |mb: usize, rng: &mut Rng| {
            let d = &defs[mb];
            let p_seen = match d.role {
                Role::Inbox if age < 30 * DAY => 0.6,
                Role::Inbox => 0.96,
                Role::Custom if d.name == "Newsletter" => 0.4,
                Role::Custom => 0.97,
                Role::Archive => 0.995,
                Role::Sent | Role::Drafts => 1.0,
                Role::Trash => 0.9,
                Role::Junk => 0.2,
            };
            let seen = rng.chance(p_seen);
            let flagged = matches!(d.role, Role::Inbox | Role::Archive | Role::Custom) && rng.chance(0.02);
            let answered = d.role == Role::Inbox && seen && rng.chance(0.15);
            LocMeta { mailbox: mb as u16, seen, flagged, answered }
        };
        let l1 = mk(primary, &mut rng);
        let role = defs[primary].role;
        let mut l2 = None;
        if matches!(role, Role::Inbox | Role::Archive | Role::Custom) && rng.chance(0.08) {
            let t = *rng.pick(&secondary_targets);
            if t != primary {
                l2 = Some(mk(t, &mut rng));
            }
        }
        let flagged = l1.flagged || l2.is_some_and(|l| l.flagged);
        let has_body = (age < 30 * DAY && matches!(role, Role::Inbox | Role::Sent | Role::Drafts))
            || flagged
            || rng.chance(0.18);
        out.push(Meta {
            seq,
            date,
            locs: [Some(l1), l2],
            has_body,
            sent: matches!(role, Role::Sent | Role::Drafts),
        });
    }
    out
}

pub struct Attachment {
    pub name: String,
    pub mime: &'static str,
    pub size: i64,
}

pub struct Content {
    pub message_id_hdr: String,
    pub thread_id: i64,
    pub subject: String,
    pub from_name: String,
    pub from_addr: String,
    pub to: String,
    pub cc: Option<String>,
    pub size: i64,
    pub preview: String,
    pub body: String,
    pub attachments: Vec<Attachment>,
}

impl Content {
    pub fn sender_text(&self) -> String {
        format!("{} {}", self.from_name, self.from_addr)
    }
    pub fn recipients_text(&self) -> String {
        match &self.cc {
            Some(cc) => format!("{} {}", self.to, cc),
            None => self.to.clone(),
        }
    }
    pub fn attachment_text(&self) -> String {
        self.attachments.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(" ")
    }
}

pub struct World {
    pub vocab: Vec<String>,
    zipf_words: Zipf,
    senders: Vec<(String, String)>,
    zipf_senders: Zipf,
    last_names: Vec<&'static str>,
    domains: Vec<String>,
}

const CORE: &str = "und the die der to ich and sie das you ist is mit for nicht of zu in ein we es a den on auf für this wir im sich auch it be dass are von have dem bitte will mail hallo an please noch can bei your with wie at uns aus vielen dank thanks einen hi werden nach so as wenn our oder hat haben kann all bis eine if als einer from am sind but rechnung more not über was gerne termin by heute morgen new team update info meeting projekt angebot bestellung woche nächste nächsten zeit invoice order account konto zahlung lieferung versand newsletter grüße grüßen freundlichen regards best attached anbei schöne gruß möchte möglich können müssen würde natürlich frage fragen antwort rückmeldung änderung änderungen bestätigung überweisung gebühr lösung prüfung übersicht verfügung zurück später früh ähnlich größe schön danke liebe lieber sehr geehrte damen herren dear thank know just time get need would like could should about which there their what when also one any been were then them some these into only other than now its over after first well way even because how many those back good work day make see go year today tomorrow next week call status report data service support customer kunden kundennummer rechnungen rechnungsnummer recht rechts rechner recherche berechnung abrechnung datum frist erinnerung zugang passwort sicherheit login neue neuen neues alle allen jetzt hier dort immer schon wieder einfach gut genau eigentlich leider gleich bald vielleicht etwas nichts viel mehr weniger ganz jahr monat tag tage stunden minuten uhr montag dienstag mittwoch donnerstag freitag samstag sonntag januar februar märz april mai juni juli august september oktober november dezember price preis euro betrag summe total payment shipping delivery tracking paket sendung paketnummer adresse straße stadt telefon nummer email link website app download dokument dokumente datei dateien anhang unterlagen vertrag kündigung verlängerung abo subscription unsubscribe abmelden einstellungen settings privacy datenschutz bedingungen terms offer angebote rabatt sale gutschein code aktion auszug";

const PREFIXES: [&str; 40] = [
    "konto", "liefer", "bestell", "rechnungs", "kunden", "projekt", "termin", "zahlungs", "vertrags",
    "steuer", "versand", "depot", "strom", "miet", "gehalts", "bank", "auftrags", "preis", "produkt",
    "service", "daten", "sicherheits", "kranken", "reise", "versicherungs", "rück", "zugangs",
    "passwort", "benutzer", "konten", "jahres", "monats", "quartals", "waren", "bewerbungs",
    "schaden", "neben", "haupt", "sonder", "büro",
];
const SUFFIXES: [&str; 30] = [
    "auszug", "schein", "nummer", "bestätigung", "übersicht", "plan", "status", "daten", "änderung",
    "anfrage", "erinnerung", "frist", "adresse", "liste", "vertrag", "gebühr", "bericht", "meldung",
    "prüfung", "abrechnung", "zahlung", "termin", "kosten", "info", "stand", "wechsel", "fehler",
    "freigabe", "beleg", "antrag",
];
const SYLLABLES: [&str; 100] = [
    "ka", "ke", "ki", "ko", "ku", "la", "le", "li", "lo", "lu", "ma", "me", "mi", "mo", "mu", "na",
    "ne", "ni", "no", "nu", "ra", "re", "ri", "ro", "ru", "sa", "se", "si", "so", "su", "ta", "te",
    "ti", "to", "tu", "ba", "be", "bi", "bo", "bu", "da", "de", "di", "do", "du", "fa", "fe", "fi",
    "fo", "fu", "ga", "ge", "gi", "go", "gu", "ha", "he", "hi", "ho", "hu", "pa", "pe", "pi", "po",
    "pu", "wa", "we", "wi", "wo", "wu", "za", "ze", "zi", "zo", "zu", "sch", "ch", "st", "tr", "ng",
    "ck", "an", "en", "er", "in", "on", "un", "ar", "or", "ur", "ä", "ö", "ü", "äu", "ei", "au",
    "ie", "ß", "th", "ing",
];
const FIRST: [&str; 40] = [
    "Jürgen", "Jörg", "Sören", "Björn", "Günter", "Anna", "Max", "Lena", "Paul", "Sophie", "Lukas",
    "Emma", "Felix", "Mia", "Jonas", "Lea", "Tim", "Hannah", "John", "Sarah", "Michael", "Emily",
    "David", "Laura", "Özlem", "Ayşe", "Mehmet", "Zoë", "Chloé", "José", "René", "François",
    "Katrin", "Thomas", "Andreas", "Sabine", "Stefan", "Petra", "Jan", "Julia",
];
const LAST: [&str; 50] = [
    "Müller", "Schmidt", "Schneider", "Fischer", "Weber", "Meyer", "Wagner", "Becker", "Schulz",
    "Hoffmann", "Schäfer", "Koch", "Bauer", "Richter", "Klein", "Wolf", "Schröder", "Neumann",
    "Schwarz", "Zimmermann", "Braun", "Krüger", "Hofmann", "Hartmann", "Lange", "Schmitt", "Werner",
    "Krause", "Meier", "Lehmann", "Köhler", "Herrmann", "König", "Walter", "Huber", "Kaiser",
    "Fuchs", "Peters", "Möller", "Weiß", "Jung", "Hahn", "Smith", "Johnson", "Williams", "Brown",
    "Garcia", "Yılmaz", "Kaya", "Nowak",
];
const FREEMAIL: [&str; 8] =
    ["gmail.com", "web.de", "gmx.de", "t-online.de", "outlook.com", "icloud.com", "posteo.de", "mailbox.org"];
const TLDS: [&str; 8] = ["de", "com", "net", "eu", "at", "ch", "io", "org"];

fn ascii_local(s: &str) -> String {
    let mut o = String::new();
    for c in s.to_lowercase().chars() {
        match c {
            'ä' => o.push_str("ae"),
            'ö' => o.push_str("oe"),
            'ü' => o.push_str("ue"),
            'ß' => o.push_str("ss"),
            c if c.is_ascii_alphanumeric() => o.push(c),
            _ => {}
        }
    }
    o
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn synthetic_word(rng: &mut Rng) -> String {
    let n = rng.range(2, 4);
    let mut w = String::new();
    for _ in 0..n {
        w.push_str(rng.pick(&SYLLABLES));
    }
    w
}

impl World {
    pub fn new() -> Self {
        let mut rng = Rng::new(42);
        let mut seen = HashSet::new();
        let mut vocab: Vec<String> = Vec::with_capacity(VOCAB_SIZE);
        for w in CORE.split_whitespace() {
            if seen.insert(w.to_string()) {
                vocab.push(w.to_string());
            }
        }
        let mut tail: Vec<String> = Vec::new();
        for p in PREFIXES {
            for s in SUFFIXES {
                let w = format!("{p}{s}");
                if seen.insert(w.clone()) {
                    tail.push(w.clone());
                }
                if rng.chance(0.33) {
                    let infl = format!("{w}{}", rng.pick(&["en", "e", "s"]));
                    if seen.insert(infl.clone()) {
                        tail.push(infl);
                    }
                }
            }
        }
        let mut synth = Vec::new();
        while vocab.len() + tail.len() + synth.len() < VOCAB_SIZE {
            let w = synthetic_word(&mut rng);
            if w.chars().count() >= 4 && seen.insert(w.clone()) {
                synth.push(w);
            }
        }
        // Compounds share the mid ranks with the first synthetic words; the rest is the long tail.
        let split = 5_000.min(synth.len());
        tail.extend(synth.drain(..split));
        for i in (1..tail.len()).rev() {
            let j = rng.below(i as u64 + 1) as usize;
            tail.swap(i, j);
        }
        vocab.extend(tail);
        vocab.extend(synth);
        vocab.truncate(VOCAB_SIZE);

        let mut domains: Vec<String> = FREEMAIL.iter().map(|s| s.to_string()).collect();
        for _ in 0..1500 {
            let n = format!("{}{}", synthetic_word(&mut rng), rng.pick(&["", "-gmbh", "-shop", "-ag", "-online"]));
            domains.push(format!("{}.{}", ascii_local(&n).replace("gmbh", "-gmbh"), rng.pick(&TLDS)));
        }
        let mut senders = Vec::with_capacity(SENDERS);
        for i in 0..SENDERS {
            let mut r = Rng::new(i as u64 ^ 0xABCDEF);
            if r.chance(0.3) {
                let d = &domains[8 + r.below((domains.len() - 8) as u64) as usize];
                let company = capitalize(d.split('.').next().unwrap_or("firma"));
                let local = r.pick(&["newsletter", "noreply", "info", "rechnung", "service", "support", "news", "team"]);
                senders.push((company, format!("{local}@{d}")));
            } else {
                let f = r.pick(&FIRST);
                let l = r.pick(&LAST);
                let d = if r.chance(0.7) { &domains[r.below(8) as usize] } else { &domains[8 + r.below(1500) as usize] };
                senders.push((format!("{f} {l}"), format!("{}.{}@{}", ascii_local(f), ascii_local(l), d)));
            }
        }
        World {
            zipf_words: Zipf::new(vocab.len(), 1.0),
            vocab,
            zipf_senders: Zipf::new(SENDERS, 1.0),
            senders,
            last_names: LAST.to_vec(),
            domains,
        }
    }

    pub fn account_address(a: usize) -> (String, String) {
        (format!("Test User {}", a + 1), format!("user{}@vice-test.example", a + 1))
    }

    fn word(&self, rng: &mut Rng) -> &str {
        &self.vocab[self.zipf_words.sample(rng)]
    }

    fn sender(&self, rng: &mut Rng, account: usize) -> &(String, String) {
        let r = self.zipf_senders.sample(rng);
        &self.senders[(r + account * 3797) % SENDERS]
    }

    fn number_token(&self, rng: &mut Rng) -> String {
        match rng.below(4) {
            0 => format!("{}", rng.range(1000, 9_999_999_999)),
            1 => format!("{},{:02} €", rng.range(1, 2500), rng.below(100)),
            2 => format!("{:02}.{:02}.{}", rng.range(1, 28), rng.range(1, 12), rng.range(2019, 2026)),
            _ => format!("{:03}-{:07}-{:07}", rng.below(1000), rng.below(10_000_000), rng.below(10_000_000)),
        }
    }

    fn url(&self, rng: &mut Rng) -> String {
        const ALNUM: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let d = &self.domains[8 + rng.below((self.domains.len() - 8) as u64) as usize];
        let n = rng.range(12, 24);
        let h: String = (0..n).map(|_| ALNUM[rng.below(ALNUM.len() as u64) as usize] as char).collect();
        let w = ascii_local(self.word(rng));
        format!("https://{d}/{w}/{h}?utm_source=newsletter&utm_medium=email")
    }

    fn body(&self, rng: &mut Rng, to_first: &str, from_name: &str) -> String {
        let mut s = String::with_capacity(2048);
        match rng.below(10) {
            0..=3 => s.push_str(&format!("Hallo {to_first},\n\n")),
            4..=5 => s.push_str(&format!("Hi {to_first},\n\n")),
            6 => s.push_str("Sehr geehrte Damen und Herren,\n\n"),
            7 => s.push_str(&format!("Dear {to_first},\n\n")),
            _ => {}
        }
        if rng.chance(0.25) {
            s.push_str("vielen Dank für Ihre Nachricht. ");
        } else if rng.chance(0.15) {
            s.push_str("Thank you for your message. ");
        }
        let target = 20 + (rng.exp(200.0) as usize).min(3000);
        let mut urls = if rng.chance(0.3) { rng.range(1, 3) } else { 0 };
        let mut words = 0;
        while words < target {
            let len = rng.range(5, 16) as usize;
            for j in 0..len {
                if j > 0 {
                    s.push(' ');
                }
                let w = self.word(rng);
                if j == 0 { s.push_str(&capitalize(w)) } else { s.push_str(w) }
                if rng.chance(0.015) {
                    s.push(' ');
                    s.push_str(&self.number_token(rng));
                }
            }
            s.push_str(rng.pick(&[". ", ". ", ". ", "? ", "! ", ".\n\n"]));
            words += len;
            if urls > 0 && rng.chance(0.1) {
                s.push_str(&self.url(rng));
                s.push('\n');
                urls -= 1;
            }
        }
        for _ in 0..urls {
            s.push_str(&self.url(rng));
            s.push('\n');
        }
        match rng.below(20) {
            0..=6 => s.push_str(&format!("\nMit freundlichen Grüßen\n{from_name}\n")),
            7..=11 => s.push_str(&format!("\nBest regards\n{from_name}\n")),
            12..=15 => s.push_str(&format!("\nViele Grüße\n{from_name}\n")),
            _ => {}
        }
        s
    }

    fn attachment(&self, rng: &mut Rng) -> Attachment {
        let y = rng.range(2019, 2026);
        let (name, mime) = match rng.below(8) {
            0 => (format!("Rechnung_{y}-{:05}.pdf", rng.below(100_000)), "application/pdf"),
            1 => (format!("IMG_{:04}.jpg", rng.below(10_000)), "image/jpeg"),
            2 => (
                format!("{}_{}.docx", capitalize(self.word(rng)), capitalize(self.word(rng))),
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            ),
            3 => (format!("Angebot {}.pdf", rng.pick(&self.last_names)), "application/pdf"),
            4 => (format!("scan{:04}.pdf", rng.below(10_000)), "application/pdf"),
            5 => (
                format!("Präsentation Q{} {y}.pptx", rng.range(1, 4)),
                "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            ),
            6 => (
                format!("{}-{}.xlsx", self.word(rng), self.word(rng)),
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            ),
            _ => (format!("Kontoauszug_{y}_{:02}.pdf", rng.range(1, 12)), "application/pdf"),
        };
        Attachment { name, mime, size: 20_000 + rng.exp(400_000.0) as i64 }
    }

    fn addr_list(&self, rng: &mut Rng, account: usize, n: u64) -> String {
        (0..n)
            .map(|_| {
                let (name, addr) = self.sender(rng, account);
                format!("{name} <{addr}>")
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn content(&self, meta: &Meta, account: usize) -> Content {
        let seq = meta.seq;
        let mut rng = Rng::new(hash64(seq ^ 0x5EED));
        let (acc_name, acc_addr) = Self::account_address(account);
        let (from_name, from_addr, to, to_first) = if meta.sent {
            let k = rng.range(1, 3);
            let to = self.addr_list(&mut rng, account, k);
            let first = to.split_whitespace().next().unwrap_or("").to_string();
            (acc_name.clone(), acc_addr.clone(), to, first)
        } else {
            let (n, a) = self.sender(&mut rng, account).clone();
            let mut to = format!("{acc_name} <{acc_addr}>");
            if rng.chance(0.2) {
                let k = rng.range(1, 3);
                to.push_str(", ");
                to.push_str(&self.addr_list(&mut rng, account, k));
            }
            (n, a, to, "Test".to_string())
        };
        let cc = if rng.chance(0.15) {
            let k = rng.range(1, 2);
            Some(self.addr_list(&mut rng, account, k))
        } else {
            None
        };
        let mut subject = String::new();
        let r = rng.below(20);
        if r < 4 {
            subject.push_str("Re: ");
        } else if r < 5 {
            subject.push_str("AW: ");
        } else if r < 6 {
            subject.push_str("Fwd: ");
        }
        let nw = rng.range(2, 8);
        for i in 0..nw {
            if i > 0 {
                subject.push(' ');
            }
            let w = self.word(&mut rng);
            if i == 0 { subject.push_str(&capitalize(w)) } else { subject.push_str(w) }
        }
        if rng.chance(0.15) {
            subject.push(' ');
            subject.push_str(&self.number_token(&mut rng));
        }
        let body = self.body(&mut rng, &to_first, &from_name);
        let preview: String = body.chars().take(160).map(|c| if c == '\n' { ' ' } else { c }).collect();
        let attachments = if rng.chance(0.15) {
            (0..rng.range(1, 3)).map(|_| self.attachment(&mut rng)).collect()
        } else {
            Vec::new()
        };
        let domain = from_addr.split('@').nth(1).unwrap_or("example.org").to_string();
        let message_id_hdr = format!("<{seq:016x}.{:08x}@{domain}>", rng.next_u64() as u32);
        let thread_id = if rng.chance(0.4) { seq.saturating_sub(rng.range(1, 40)) } else { seq } as i64;
        let size = 3_000 + body.len() as i64 * 3 + attachments.iter().map(|a| a.size).sum::<i64>() + rng.exp(5_000.0) as i64;
        Content { message_id_hdr, thread_id, subject, from_name, from_addr, to, cc, size, preview, body, attachments }
    }
}

/// First `n` whitespace-separated words of `s` (used for the "body for all mails, shorter" case).
pub fn truncate_words(s: &str, n: usize) -> &str {
    let mut count = 0;
    let mut in_word = false;
    for (i, c) in s.char_indices() {
        if c.is_whitespace() {
            if in_word {
                count += 1;
                if count == n {
                    return &s[..i];
                }
            }
            in_word = false;
        } else {
            in_word = true;
        }
    }
    s
}
