//! Tracker protection (stage 20, decision 0058), without IO.
//!
//! - **Images.** External images are blocked anyway until the user loads them (security
//!   boundary 2). Among them, the HTML pipeline marks trackers: *pixels* (an image of at most
//!   2 × 2 pixels or hidden by its attributes or inline style, [`is_pixel`]), images on *known
//!   tracking hosts* ([`is_known_tracker_host`]) and images whose address carries *tracking
//!   parameters* ([`strip_tracking_parameters`]). Pixels and known trackers are never loaded,
//!   not even after "load images"; an image with tracking parameters loads from its address
//!   without them. The counts go to the reading pane ([`TrackerReport`]).
//! - **Links.** Opening a link drops the tracking parameters of its address
//!   ([`strip_tracking_parameters`]); the link card shows the clean target. Redirect links of
//!   tracking services are never resolved: that would be a connection to the tracker.
//!
//! The lists are maintained here, in code. Parameter names are compared without regard to
//! ASCII case; only names that are used for tracking and nothing else are listed, so that no
//! link loses a parameter it needs.

use std::borrow::Cow;

use url::Url;

/// Query parameters that only track (compared ignoring ASCII case).
pub const TRACKING_PARAMETERS: &[&str] = &[
    // Google Ads and Analytics, Display & Video 360
    "gclid",
    "gclsrc",
    "dclid",
    "gbraid",
    "wbraid",
    "_ga",
    "_gl",
    // Meta, Microsoft, X/Twitter, TikTok, LinkedIn, Yandex, Instagram
    "fbclid",
    "msclkid",
    "twclid",
    "ttclid",
    "li_fat_id",
    "yclid",
    "igshid",
    // Mailchimp
    "mc_cid",
    "mc_eid",
    // HubSpot
    "_hsenc",
    "_hsmi",
    "__hssc",
    "__hstc",
    "__hsfp",
    "hsctatracking",
    // Marketo
    "mkt_tok",
    // Adobe Analytics
    "s_cid",
    // Omeda, Vero, Rakuten, Openstat, Wicked Reports
    "oly_anon_id",
    "oly_enc_id",
    "vero_id",
    "vero_conv",
    "rb_clickid",
    "_openstat",
    "wickedid",
    // MailerLite, Klaviyo, Sailthru
    "ml_subscriber",
    "ml_subscriber_hash",
    "_kx",
    "_ke",
    "sc_cid",
    // Drip, Simplycast, ActiveCampaign
    "__s",
    "ss_email_id",
    "vgo_ee",
];

/// Prefixes of tracking parameters: campaign parameters of Google Analytics (`utm_source`, …),
/// Matomo (`mtm_`), Piwik (`pk_`) and HubSpot ads (`hsa_`).
pub const TRACKING_PARAMETER_PREFIXES: &[&str] = &["utm_", "mtm_", "pk_", "hsa_"];

/// Hosts that serve open-tracking pixels and nothing a reader needs (a subdomain counts as the
/// host). Hosts of content (image servers of newsletter services) are deliberately not here.
pub const KNOWN_TRACKER_HOSTS: &[&str] = &[
    // Mailchimp, Mandrill
    "list-manage.com",
    "mandrillapp.com",
    // SendGrid, SparkPost, Amazon SES, Mailjet, Brevo (Sendinblue)
    "sendgrid.net",
    "sparkpostmail.com",
    "awstrack.me",
    "mjt.lu",
    "sendibt2.com",
    "sendibt3.com",
    "sendibm1.com",
    // Salesforce Marketing Cloud, Pardot, Eloqua, Marketo
    "exct.net",
    "exacttarget.com",
    "pardot.com",
    "eloqua.com",
    "mktoresp.com",
    // HubSpot
    "hubspotemail.net",
    "hs-analytics.net",
    // Constant Contact, Campaign Monitor, iContact, MailerLite, Klaviyo, Customer.io, ConvertKit
    "rs6.net",
    "cmail19.com",
    "cmail20.com",
    "icptrack.com",
    "mlsend.com",
    "klaviyomail.com",
    "customeriomail.com",
    "convertkit-mail.com",
    "convertkit-mail2.com",
    // Substack's open pixel
    "eotrx.substackcdn.com",
    // Read-receipt services of mail add-ons
    "mailtrack.io",
    "yesware.com",
    "bananatag.com",
    "getnotify.com",
    "mailfoogae.appspot.com",
    "r.superhuman.com",
    "emltrk.com",
    "returnpath.net",
    // Ad and analytics networks
    "doubleclick.net",
    "google-analytics.com",
];

/// Why an image counts as a tracker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrackerKind {
    /// A pixel: at most 2 × 2 pixels, or hidden.
    Pixel,
    /// An image on a known tracking host.
    KnownHost,
    /// An image whose address carried tracking parameters (loaded without them).
    Parameters,
}

/// Most hosts named in a [`TrackerReport`].
pub const MAX_TRACKER_HOSTS: usize = 8;

/// The trackers of one rendering, each image address once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackerReport {
    pub pixels: u32,
    pub known_hosts: u32,
    pub parameters: u32,
    /// Hosts of the blocked pixels and known trackers, in document order, each once, at most
    /// [`MAX_TRACKER_HOSTS`].
    pub hosts: Vec<String>,
}

impl TrackerReport {
    /// All trackers found.
    pub fn total(&self) -> u32 {
        self.pixels
            .saturating_add(self.known_hosts)
            .saturating_add(self.parameters)
    }

    pub(crate) fn add(&mut self, kind: TrackerKind, host: Option<&str>) {
        match kind {
            TrackerKind::Pixel => self.pixels = self.pixels.saturating_add(1),
            TrackerKind::KnownHost => self.known_hosts = self.known_hosts.saturating_add(1),
            TrackerKind::Parameters => self.parameters = self.parameters.saturating_add(1),
        }
        if kind != TrackerKind::Parameters
            && let Some(host) = host
            && self.hosts.len() < MAX_TRACKER_HOSTS
            && !self.hosts.iter().any(|h| h == host)
        {
            self.hosts.push(host.to_owned());
        }
    }
}

/// An address without its tracking parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanedUrl {
    pub url: String,
    /// The names of the removed parameters as written, each once, in order.
    pub removed: Vec<String>,
}

/// Whether a query parameter name tracks.
pub fn is_tracking_parameter(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    TRACKING_PARAMETERS.contains(&lower.as_str())
        || TRACKING_PARAMETER_PREFIXES
            .iter()
            .any(|prefix| lower.starts_with(prefix) && lower.len() > prefix.len())
}

/// The `http`/`https` address without its tracking parameters; `None` when it has none (or is
/// no `http`/`https` URL). The other parameters keep their order and exact spelling; a query
/// left empty goes. The fragment stays.
pub fn strip_tracking_parameters(href: &str) -> Option<CleanedUrl> {
    let mut url = Url::parse(href.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let query = url.query()?.to_owned();
    let mut kept: Vec<&str> = Vec::new();
    let mut removed: Vec<String> = Vec::new();
    for pair in query.split('&') {
        let raw_name = pair.split_once('=').map_or(pair, |(name, _)| name);
        let name = percent_decode(raw_name);
        if !name.is_empty() && is_tracking_parameter(&name) {
            if !removed.iter().any(|r| r == name.as_ref()) {
                removed.push(name.into_owned());
            }
        } else if !pair.is_empty() {
            kept.push(pair);
        }
    }
    if removed.is_empty() {
        return None;
    }
    if kept.is_empty() {
        url.set_query(None);
    } else {
        url.set_query(Some(&kept.join("&")));
    }
    Some(CleanedUrl {
        url: url.into(),
        removed,
    })
}

/// Whether `host` (ASCII, any case) is a known tracking host or below one.
pub fn is_known_tracker_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    KNOWN_TRACKER_HOSTS.iter().any(|known| {
        host == *known
            || host
                .strip_suffix(known)
                .is_some_and(|rest| rest.ends_with('.'))
    })
}

/// Largest image (each side, CSS pixels) that counts as a pixel.
pub const PIXEL_MAX: u32 = 2;

/// Whether an `<img>` is a tracking pixel by its `width`/`height` attributes, its inline
/// `style` and the `hidden` attribute: hidden (`display: none`, `visibility: hidden`,
/// `hidden`), a side of 0, or both sides at most [`PIXEL_MAX`]. Percentages and unknown
/// units say nothing.
pub fn is_pixel(
    width: Option<&str>,
    height: Option<&str>,
    style: Option<&str>,
    hidden: bool,
) -> bool {
    if hidden {
        return true;
    }
    let mut w = width.and_then(length);
    let mut h = height.and_then(length);
    if let Some(style) = style {
        for declaration in style.split(';') {
            let Some((property, value)) = declaration.split_once(':') else {
                continue;
            };
            let property = property.trim().to_ascii_lowercase();
            let value = value
                .trim()
                .trim_end_matches("!important")
                .trim()
                .to_ascii_lowercase();
            match property.as_str() {
                "display" if value == "none" => return true,
                "visibility" if value == "hidden" || value == "collapse" => return true,
                "width" | "max-width" => {
                    w = length(&value).map_or(w, |v| Some(w.map_or(v, |o| o.min(v))))
                }
                "height" | "max-height" => {
                    h = length(&value).map_or(h, |v| Some(h.map_or(v, |o| o.min(v))));
                }
                _ => {}
            }
        }
    }
    match (w, h) {
        (Some(0), _) | (_, Some(0)) => true,
        (Some(w), Some(h)) => w <= PIXEL_MAX && h <= PIXEL_MAX,
        _ => false,
    }
}

/// A length in CSS pixels: a number, optionally with `px`; `None` for percentages, other
/// units and anything else.
fn length(value: &str) -> Option<u32> {
    let value = value.trim();
    let number = value.strip_suffix("px").unwrap_or(value).trim();
    if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    let parsed: f64 = number.parse().ok()?;
    if !parsed.is_finite() || parsed < 0.0 {
        return None;
    }
    // Rounded up: 1.5 px is drawn on two pixels.
    let ceil = parsed.ceil();
    (ceil <= f64::from(u32::MAX)).then(|| {
        // In range by the check above.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let value = ceil as u32;
        value
    })
}

fn percent_decode(text: &str) -> Cow<'_, str> {
    if !text.contains(['%', '+']) {
        return Cow::Borrowed(text);
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        if byte == b'%'
            && let Some(value) = text
                .get(i + 1..i + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(value);
            i += 3;
            continue;
        }
        out.push(if byte == b'+' { b' ' } else { byte });
        i += 1;
    }
    Cow::Owned(String::from_utf8_lossy(&out).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_campaign_and_click_parameters_only() {
        let cleaned = strip_tracking_parameters(
            "https://shop.example/item?id=42&utm_source=news&utm_medium=email&fbclid=abc&color=red#top",
        )
        .expect("cleaned");
        assert_eq!(cleaned.url, "https://shop.example/item?id=42&color=red#top");
        assert_eq!(cleaned.removed, ["utm_source", "utm_medium", "fbclid"]);
    }

    #[test]
    fn keeps_the_exact_spelling_of_other_parameters() {
        let cleaned =
            strip_tracking_parameters("https://a.example/?q=a%20b+c&gclid=1&next=%2Fx%3Fy%3D1")
                .expect("cleaned");
        assert_eq!(
            cleaned.url,
            "https://a.example/?q=a%20b+c&next=%2Fx%3Fy%3D1"
        );
    }

    #[test]
    fn an_empty_query_goes() {
        let cleaned =
            strip_tracking_parameters("https://a.example/path?mc_cid=1&mc_eid=2").expect("cleaned");
        assert_eq!(cleaned.url, "https://a.example/path");
        assert_eq!(cleaned.removed, ["mc_cid", "mc_eid"]);
    }

    #[test]
    fn names_compare_without_case_and_encoded() {
        let cleaned =
            strip_tracking_parameters("http://a.example/?UTM_Campaign=x&%75tm_term=y&ok=1")
                .expect("cleaned");
        assert_eq!(cleaned.url, "http://a.example/?ok=1");
        assert_eq!(cleaned.removed, ["UTM_Campaign", "utm_term"]);
    }

    #[test]
    fn leaves_other_links_alone() {
        for href in [
            "https://a.example/?id=1&ref=2&source=3",
            "https://a.example/",
            "mailto:a@example.org?subject=utm_source",
            "ftp://a.example/?utm_source=x",
            "not a url",
            // A bare prefix is no parameter of its own.
            "https://a.example/?utm_=1",
        ] {
            assert_eq!(strip_tracking_parameters(href), None, "{href}");
        }
    }

    #[test]
    fn duplicate_parameters_are_named_once() {
        let cleaned = strip_tracking_parameters("https://a.example/?utm_source=a&utm_source=b&x")
            .expect("cleaned");
        assert_eq!(cleaned.url, "https://a.example/?x");
        assert_eq!(cleaned.removed, ["utm_source"]);
    }

    #[test]
    fn known_hosts_include_subdomains_only() {
        assert!(is_known_tracker_host("list-manage.com"));
        assert!(is_known_tracker_host("us5.List-Manage.com."));
        assert!(is_known_tracker_host("u123.ct.sendgrid.net"));
        assert!(!is_known_tracker_host("notlist-manage.com"));
        assert!(!is_known_tracker_host("mcusercontent.com"));
        assert!(!is_known_tracker_host("superhuman.com"));
        assert!(is_known_tracker_host("r.superhuman.com"));
    }

    #[test]
    fn pixels_by_size_and_visibility() {
        assert!(is_pixel(Some("1"), Some("1"), None, false));
        assert!(is_pixel(Some("2px"), Some("1"), None, false));
        assert!(is_pixel(Some("0"), None, None, false));
        assert!(is_pixel(None, None, Some("width:1px;height:1px"), false));
        assert!(is_pixel(
            None,
            None,
            Some("display: none !important"),
            false
        ));
        assert!(is_pixel(None, None, Some("visibility:hidden"), false));
        assert!(is_pixel(
            Some("600"),
            Some("300"),
            Some("max-height:0"),
            false
        ));
        assert!(is_pixel(Some("600"), Some("300"), None, true));
        assert!(!is_pixel(Some("600"), Some("1"), None, false));
        assert!(!is_pixel(Some("1"), None, None, false));
        assert!(!is_pixel(Some("100%"), Some("1"), None, false));
        assert!(!is_pixel(None, None, None, false));
        assert!(!is_pixel(Some("3"), Some("3"), None, false));
    }

    #[test]
    fn the_report_counts_and_names_hosts_once() {
        let mut report = TrackerReport::default();
        report.add(TrackerKind::Pixel, Some("a.example"));
        report.add(TrackerKind::KnownHost, Some("a.example"));
        report.add(TrackerKind::Parameters, Some("b.example"));
        assert_eq!(report.total(), 3);
        assert_eq!(report.hosts, ["a.example"]);
    }
}
