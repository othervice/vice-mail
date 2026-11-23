//! Header evidence of known systems, newsletter software and mailing services.

use super::headers::ClassifyHeaders;

/// Headers only one system writes, and its name.
const KNOWN_SYSTEMS: &[(&str, &str)] = &[
    ("X-GitHub-Reason", "GitHub"),
    ("X-GitLab-Project", "GitLab"),
    ("X-GitLab-NotificationReason", "GitLab"),
    ("X-Gitea-Reason", "Gitea"),
    ("X-Forgejo-Reason", "Forgejo"),
    ("X-JIRA-FingerPrint", "Jira"),
    ("X-Bugzilla-Reason", "Bugzilla"),
    ("X-Redmine-Project", "Redmine"),
    ("X-Phabricator-Sent-This-Message", "Phabricator"),
    ("X-Gerrit-MessageType", "Gerrit"),
    ("X-Jenkins-Job", "Jenkins"),
    ("X-Discourse-Post-Id", "Discourse"),
    ("X-Launchpad-Notification-Type", "Launchpad"),
    ("X-Facebook-Notify", "Facebook"),
];

/// Headers of newsletter software and campaign tools (`X-Campaign…` and `X-Newsletter…` are
/// matched as prefixes).
const CAMPAIGN_HEADERS: &[&str] = &[
    "X-CSA-Complaints",
    "X-Report-Abuse",
    "X-MC-User",
    "X-Mailjet-Campaign",
    "X-Mailin-Campaign",
];

/// Newsletter software in `X-Mailer` (lower case, contained).
const NEWSLETTER_MAILERS: &[&str] = &[
    "mailchimp",
    "cleverreach",
    "rapidmail",
    "newsletter2go",
    "sendinblue",
    "brevo",
    "phplist",
    "mailpoet",
    "mautic",
    "sendy",
    "mailerlite",
    "acymailing",
    "klaviyo",
    "hubspot",
    "inxmail",
    "newsletter",
];

/// Bounce domains of marketing services (the `Return-Path` ends with one of them).
const MARKETING_DOMAINS: &[(&str, &str)] = &[
    ("mcsv.net", "Mailchimp"),
    ("mcdlv.net", "Mailchimp"),
    ("rsgsv.net", "Mailchimp"),
    ("hubspotemail.net", "HubSpot"),
    ("exacttarget.com", "Salesforce Marketing Cloud"),
    ("createsend.com", "Campaign Monitor"),
    ("klaviyomail.com", "Klaviyo"),
    ("mktomail.com", "Marketo"),
    ("ccsend.com", "Constant Contact"),
    ("constantcontact.com", "Constant Contact"),
    ("mlsend.com", "MailerLite"),
    ("nl2go.com", "Newsletter2Go"),
    ("sendinblue.com", "Brevo"),
    ("brevo.com", "Brevo"),
];

/// Headers of mailing services used for transactional mail as well.
const SERVICE_HEADERS: &[(&str, &str)] = &[
    ("Feedback-ID", "Feedback-ID"),
    ("X-SES-Outgoing", "Amazon SES"),
    ("X-SG-EID", "SendGrid"),
    ("X-Mailgun-Sid", "Mailgun"),
    ("X-PM-Message-Id", "Postmark"),
    ("X-Mandrill-User", "Mandrill"),
    ("X-MSFBL", "SparkPost"),
    ("X-MJ-Mid", "Mailjet"),
    ("X-SFMC-Stack", "Salesforce Marketing Cloud"),
];

/// Bounce domains of those services.
const SERVICE_DOMAINS: &[(&str, &str)] = &[
    ("amazonses.com", "Amazon SES"),
    ("sendgrid.net", "SendGrid"),
    ("mailgun.org", "Mailgun"),
    ("mailgun.net", "Mailgun"),
    ("mtasv.net", "Postmark"),
    ("sparkpostmail.com", "SparkPost"),
    ("mandrillapp.com", "Mandrill"),
    ("mailjet.com", "Mailjet"),
];

/// Auto-reply subjects (lower case, at the start).
const AUTO_REPLY_SUBJECTS: &[&str] = &[
    "automatic reply:",
    "auto reply:",
    "auto-reply:",
    "autoreply:",
    "out of office",
    "out of the office",
    "automatische antwort:",
    "abwesenheitsnotiz",
    "abwesend:",
    "réponse automatique",
    "absence :",
    "respuesta automática",
    "risposta automatica",
    "automatisch antwoord",
];

/// `(system, header)` of a known system's header.
pub(crate) fn known_system(headers: &ClassifyHeaders) -> Option<(&'static str, &'static str)> {
    KNOWN_SYSTEMS
        .iter()
        .find(|(header, _)| headers.field(header).is_some())
        .map(|(header, system)| (*system, *header))
}

/// Evidence of newsletter software or a campaign.
pub(crate) fn campaign(headers: &ClassifyHeaders) -> Option<String> {
    if let Some(header) = CAMPAIGN_HEADERS.iter().find(|h| headers.field(h).is_some()) {
        return Some((*header).to_owned());
    }
    if let Some((name, _)) = headers
        .fields
        .iter()
        .find(|(name, _)| name.starts_with("x-campaign") || name.starts_with("x-newsletter"))
    {
        return Some(header_case(name));
    }
    if let Some(mailer) = headers.field("X-Mailer") {
        let lower = mailer.to_lowercase();
        if NEWSLETTER_MAILERS.iter().any(|tool| lower.contains(tool)) {
            return Some(short(&format!("X-Mailer: {mailer}")));
        }
    }
    bounce_domain(headers, MARKETING_DOMAINS)
        .or_else(|| campaign_monitor(headers).then_some("Campaign Monitor"))
        .map(|service| format!("Return-Path: {service}"))
}

/// Evidence of a mailing service (transactional or bulk).
pub(crate) fn mailing_service(headers: &ClassifyHeaders) -> Option<String> {
    if let Some((_, service)) = SERVICE_HEADERS
        .iter()
        .find(|(header, _)| headers.field(header).is_some())
    {
        return Some((*service).to_owned());
    }
    if let Some(service) = bounce_domain(headers, SERVICE_DOMAINS) {
        return Some(format!("Return-Path: {service}"));
    }
    // VERP bounce addresses (`bounces+123-alice=example.org@…`) and list bounce addresses.
    let bounce_like = |address: &str| {
        let local = address
            .rsplit_once('@')
            .map_or("", |(l, _)| l)
            .to_lowercase();
        local.starts_with("bounce")
            || local.contains("-bounces")
            || local.ends_with("-owner")
            || local.starts_with("owner-")
            || local.ends_with("-request")
    };
    if let Some(path) = headers.return_path.as_deref().filter(|p| bounce_like(p)) {
        return Some(short(&format!("Return-Path: {path}")));
    }
    headers
        .sender
        .iter()
        .find(|a| bounce_like(&a.email))
        .map(|a| short(&format!("Sender: {}", a.email)))
}

/// Evidence of an automatic reply other than `Auto-Submitted`.
pub(crate) fn auto_reply(headers: &ClassifyHeaders) -> Option<String> {
    if headers
        .precedence
        .as_deref()
        .is_some_and(|p| p.eq_ignore_ascii_case("auto_reply"))
    {
        return Some("Precedence: auto_reply".to_owned());
    }
    for header in ["X-Autoreply", "X-Autorespond"] {
        if let Some(value) = headers.field(header)
            && !value.eq_ignore_ascii_case("no")
        {
            return Some(header.to_owned());
        }
    }
    let subject = headers.subject.as_deref()?.trim();
    let lower = subject.to_lowercase();
    AUTO_REPLY_SUBJECTS
        .iter()
        .any(|prefix| lower.starts_with(prefix))
        .then(|| short(&format!("Subject: {subject}")))
}

/// Evidence of a delivery, read or other report.
pub(crate) fn report(headers: &ClassifyHeaders) -> Option<String> {
    if headers.content_type.as_deref() == Some("multipart/report") {
        return Some(match &headers.report_type {
            Some(kind) => format!("multipart/report; report-type={kind}"),
            None => "multipart/report".to_owned(),
        });
    }
    if headers.return_path.as_deref() == Some("") {
        return Some("Return-Path: <>".to_owned());
    }
    let from = headers.from_email()?;
    let local = from.rsplit_once('@').map_or(from, |(l, _)| l);
    (local.eq_ignore_ascii_case("mailer-daemon") || local.eq_ignore_ascii_case("postmaster"))
        .then(|| short(&format!("From: {from}")))
}

fn bounce_domain(
    headers: &ClassifyHeaders,
    domains: &[(&str, &'static str)],
) -> Option<&'static str> {
    let path = headers.return_path.as_deref()?;
    let domain = path.rsplit_once('@')?.1.trim().to_ascii_lowercase();
    domains
        .iter()
        .find(|(suffix, _)| {
            domain == *suffix
                || domain
                    .strip_suffix(suffix)
                    .is_some_and(|rest| rest.ends_with('.'))
        })
        .map(|(_, service)| *service)
}

/// Campaign Monitor bounces through numbered domains (`cmail1.com` … `cmail30.com`).
fn campaign_monitor(headers: &ClassifyHeaders) -> bool {
    let Some((_, domain)) = headers
        .return_path
        .as_deref()
        .and_then(|p| p.rsplit_once('@'))
    else {
        return false;
    };
    let domain = domain.trim().to_ascii_lowercase();
    let mut labels = domain.rsplit('.');
    labels.next() == Some("com")
        && labels
            .next()
            .and_then(|label| label.strip_prefix("cmail"))
            .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

/// `x-campaign-id` → `X-Campaign-Id`.
fn header_case(name: &str) -> String {
    name.split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("-")
}

/// A header value cut for a reason shown in the UI.
pub(crate) fn short(value: &str) -> String {
    const MAX: usize = 64;
    let value = value.trim();
    if value.chars().count() <= MAX {
        return value.to_owned();
    }
    let mut cut: String = value.chars().take(MAX - 1).collect();
    cut.push('…');
    cut
}
