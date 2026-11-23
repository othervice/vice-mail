//! Attachment file names: made harmless before they are shown or used as a suggestion in the
//! save dialog, and the check for types that run code when opened.

use icu_normalizer::ComposingNormalizerBorrowed;
use icu_properties::CodePointMapData;
use icu_properties::props::GeneralCategory;

/// Name used when nothing usable is left of the original name.
pub const FALLBACK_NAME: &str = "attachment";

/// Longest name in UTF-8 bytes. APFS and NTFS allow 255 (bytes resp. UTF-16 units); the margin
/// leaves room for the " (2)" the save dialog may add.
pub const MAX_NAME_BYTES: usize = 200;

/// Longest extension that is kept when a name is shortened.
const MAX_EXTENSION_BYTES: usize = 16;

/// Makes an attachment name harmless on macOS and Windows:
///
/// - Unicode NFC (macOS and Windows compare names differently otherwise)
/// - only the last path component: no `/`, `\`, `..`, drive letters (`C:\…`, `C:x`)
/// - no control characters (NUL included), no bidirectional controls (U+202A–U+202E,
///   U+2066–U+2069, U+200E, U+200F, U+061C) and no other invisible format characters except the
///   joiners U+200C/U+200D that some scripts need
/// - none of `< > : " | ? *` (Windows), whitespace runs collapsed
/// - no leading dots (hidden files) and no trailing dots or spaces (Windows drops them)
/// - no reserved Windows device names (`CON`, `PRN`, `AUX`, `NUL`, `COM0`–`COM9`, `LPT0`–`LPT9`,
///   also with superscript digits, `CONIN$`, `CONOUT$`), with or without extension
/// - at most [`MAX_NAME_BYTES`], keeping the extension
/// - [`FALLBACK_NAME`] when nothing is left
pub fn sanitize_filename(name: &str) -> String {
    let nfc = ComposingNormalizerBorrowed::new_nfc();
    let normalized = nfc.normalize(name);
    let last = last_component(&normalized);

    let category = CodePointMapData::<GeneralCategory>::new();
    let mut cleaned = String::with_capacity(last.len());
    let mut pending_space = false;
    for ch in last.chars() {
        let mapped = match ch {
            '\t' | '\n' | '\r' => Some(' '),
            c if c.is_control() => None,
            '\u{200C}' | '\u{200D}' => Some(ch),
            c if is_bidi_control(c) => None,
            c if category.get(c) == GeneralCategory::Format => None,
            '<' | '>' | ':' | '"' | '|' | '?' | '*' | '/' | '\\' => Some('_'),
            c if c.is_whitespace() => Some(' '),
            c => Some(c),
        };
        match mapped {
            Some(' ') => pending_space = true,
            Some(c) => {
                if pending_space && !cleaned.is_empty() {
                    cleaned.push(' ');
                }
                pending_space = false;
                cleaned.push(c);
            }
            None => {}
        }
    }

    let trimmed = cleaned
        .trim_start_matches(['.', ' '])
        .trim_end_matches(['.', ' ']);
    let mut result = if trimmed.is_empty() {
        String::new()
    } else if cleaned.starts_with('.') && !cleaned[1..].starts_with('.') && !trimmed.contains('.') {
        // ".pdf" or ".bashrc": keep it recognisable without making it a hidden file.
        format!("{FALLBACK_NAME}.{trimmed}")
    } else {
        trimmed.to_owned()
    };

    result = shorten(&result);
    result = nfc.normalize(&result).into_owned();
    if result.is_empty() {
        return FALLBACK_NAME.to_owned();
    }
    if is_reserved_windows_name(&result) {
        result.insert(0, '_');
        result = shorten(&result);
    }
    result
}

/// [`sanitize_filename`] for a part's declared name, with a neutral name and an extension
/// derived from the MIME type when the part has no usable name.
pub fn attachment_file_name(name: Option<&str>, mime: &str) -> String {
    let sanitized = name.map(sanitize_filename);
    match sanitized {
        Some(name) if name != FALLBACK_NAME => name,
        _ => match extension_for_mime(mime) {
            Some(ext) => format!("{FALLBACK_NAME}.{ext}"),
            None => FALLBACK_NAME.to_owned(),
        },
    }
}

/// The part after the last `/` or `\`, ignoring trailing separators; `.` and `..` count as
/// nothing. A drive prefix such as `C:` is left to the `:` replacement.
fn last_component(name: &str) -> &str {
    name.split(['/', '\\'])
        .rev()
        .find(|part| !part.trim().is_empty())
        .filter(|part| !matches!(part.trim(), "." | ".."))
        .unwrap_or("")
}

fn is_bidi_control(c: char) -> bool {
    matches!(
        c,
        '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' | '\u{061C}'
    )
}

/// Cuts the name to [`MAX_NAME_BYTES`] on a character boundary and keeps a short extension.
fn shorten(name: &str) -> String {
    if name.len() <= MAX_NAME_BYTES {
        return name.to_owned();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(dot) if dot > 0 && name.len() - dot <= MAX_EXTENSION_BYTES + 1 => name.split_at(dot),
        _ => (name, ""),
    };
    let budget = MAX_NAME_BYTES - ext.len();
    let cut = stem.floor_char_boundary(budget);
    let stem = stem[..cut].trim_end_matches(['.', ' ']);
    let stem = if stem.is_empty() { FALLBACK_NAME } else { stem };
    format!("{stem}{ext}")
}

/// Windows device names, also with an extension (`aux.txt`) and trailing spaces before it.
fn is_reserved_windows_name(name: &str) -> bool {
    let base = name.split('.').next().unwrap_or("").trim_end();
    let upper = base.to_uppercase();
    match upper.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" => true,
        _ => {
            let mut chars = upper.chars();
            let prefix: String = chars.by_ref().take(3).collect();
            let rest: Vec<char> = chars.collect();
            (prefix == "COM" || prefix == "LPT")
                && rest.len() == 1
                && matches!(rest[0], '0'..='9' | '¹' | '²' | '³')
        }
    }
}

fn extension_for_mime(mime: &str) -> Option<&'static str> {
    let essence = mime
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    Some(match essence.as_str() {
        "application/pdf" => "pdf",
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "text/plain" => "txt",
        "text/csv" => "csv",
        "text/calendar" => "ics",
        "text/html" => "html",
        "application/zip" => "zip",
        "message/rfc822" | "message/global" => "eml",
        _ => return None,
    })
}

/// Extensions that run code, install software or open a document that can run code when the
/// user opens the file, on macOS or Windows. Opening them gets a warning first. Grouped:
///
/// - Windows programs, installers and scripts: run directly or through a system host
///   (Windows Script Host, PowerShell, mshta, msiexec, Control Panel, ClickOnce).
/// - Windows shortcuts and settings files: `.lnk`, `.url`, `.scf`, `.settingcontent-ms`,
///   `.library-ms`, `.search-ms`, `.reg`, `.inf` point at or change other programs.
/// - Disk images and archives that mount: `.iso`, `.img`, `.vhd(x)` (Windows), `.dmg`
///   (macOS) carry programs and, on Windows, remove the mark-of-the-web from their contents.
/// - macOS apps, installers and scripts: `.app`, `.pkg`, `.command`, `.tool`, AppleScript,
///   Automator workflows, `.terminal` settings (can run a command), `.mobileconfig` profiles,
///   `.webloc`/`.inetloc`/`.fileloc` shortcuts.
/// - Shell and interpreter scripts: run in Terminal or when made executable.
/// - Java archives: run with a double click where Java is installed.
/// - Office files with macros (`.docm`, `.xlsm`, ...), Excel add-ins (`.xll`, `.xlam`) and
///   compiled help (`.chm`): run code from inside the document.
/// - HTML, XHTML, SVG and MHT: open in the default browser as local files, outside the mail
///   sandbox, with scripts enabled. Phishing pages and "HTML smuggling" use exactly this.
const EXECUTABLE_EXTENSIONS: &[&str] = &[
    // Windows programs, installers, scripts
    "exe",
    "com",
    "bat",
    "cmd",
    "scr",
    "pif",
    "cpl",
    "msi",
    "msp",
    "mst",
    "msix",
    "msixbundle",
    "appx",
    "appxbundle",
    "appinstaller",
    "application",
    "xbap",
    "gadget",
    "msc",
    "hta",
    "ps1",
    "psm1",
    "ps1xml",
    "psc1",
    "vb",
    "vbs",
    "vbe",
    "js",
    "jse",
    "wsf",
    "wsh",
    "wsc",
    "ws",
    "sct",
    "dll",
    "sys",
    "drv",
    "ocx",
    "cab",
    // Windows shortcuts, settings
    "lnk",
    "url",
    "scf",
    "reg",
    "inf",
    "settingcontent-ms",
    "library-ms",
    "search-ms",
    // disk images
    "iso",
    "img",
    "vhd",
    "vhdx",
    "dmg",
    "sparseimage",
    "sparsebundle",
    // macOS
    "app",
    "pkg",
    "mpkg",
    "command",
    "tool",
    "scpt",
    "scptd",
    "applescript",
    "workflow",
    "action",
    "terminal",
    "mobileconfig",
    "webloc",
    "inetloc",
    "fileloc",
    "prefpane",
    "kext",
    "osax",
    "saver",
    "qlgenerator",
    "plugin",
    "bundle",
    // shell and interpreter scripts
    "sh",
    "bash",
    "zsh",
    "csh",
    "tcsh",
    "ksh",
    "fish",
    "py",
    "pyc",
    "pyw",
    "pl",
    "rb",
    "php",
    "lua",
    "tcl",
    // Java
    "jar",
    "jnlp",
    // documents that run code
    "docm",
    "dotm",
    "xlsm",
    "xltm",
    "xlsb",
    "xlam",
    "xll",
    "pptm",
    "potm",
    "ppsm",
    "ppam",
    "sldm",
    "chm",
    // Stage 15 review: further types that run code or are used for it in phishing (remote
    // desktop connections, OneNote, Excel web queries and symbolic links, ClickOnce and Office
    // add-ins, provisioning packages, sandbox configurations, themes, troubleshooting packs,
    // search connectors, Access databases, Python zip apps, update packages, internet
    // shortcuts, certificates, macOS shortcuts)
    "rdp",
    "one",
    "onepkg",
    "iqy",
    "slk",
    "appref-ms",
    "vsto",
    "ppkg",
    "wsb",
    "theme",
    "themepack",
    "deskthemepack",
    "diagcab",
    "searchconnector-ms",
    "xla",
    "xlm",
    "ade",
    "adp",
    "accda",
    "accde",
    "mda",
    "mdb",
    "mde",
    "pyz",
    "pyzw",
    "msu",
    "website",
    "cer",
    "crt",
    "der",
    "shortcut",
    // local web documents
    "html",
    "htm",
    "xhtml",
    "xht",
    "shtml",
    "mht",
    "mhtml",
    "svg",
    "svgz",
];

/// MIME types that mean the same as [`EXECUTABLE_EXTENSIONS`], for parts without a telling name.
const EXECUTABLE_MIME_TYPES: &[&str] = &[
    "application/x-msdownload",
    "application/x-msdos-program",
    "application/x-ms-installer",
    "application/x-msi",
    "application/vnd.microsoft.portable-executable",
    "application/x-dosexec",
    "application/x-executable",
    "application/x-mach-binary",
    "application/x-sh",
    "application/x-shellscript",
    "application/x-csh",
    "application/x-bat",
    "application/x-powershell",
    "application/hta",
    "application/javascript",
    "application/x-javascript",
    "text/javascript",
    "text/vbscript",
    "application/java-archive",
    "application/x-java-archive",
    "application/x-java-jnlp-file",
    "application/x-apple-diskimage",
    "application/vnd.apple.installer+xml",
    "application/x-iso9660-image",
    "application/x-ms-shortcut",
    "application/vnd.ms-word.document.macroenabled.12",
    "application/vnd.ms-excel.sheet.macroenabled.12",
    "application/vnd.ms-powerpoint.presentation.macroenabled.12",
    "application/vnd.ms-htmlhelp",
    "text/html",
    "application/xhtml+xml",
    "image/svg+xml",
    "application/x-mimearchive",
    "application/x-rdp",
    "application/onenote",
    "application/msonenote",
    "application/x-ms-application",
    "application/vnd.ms-excel.addin.macroenabled.12",
    "application/x-x509-ca-cert",
];

/// Whether opening the file can run code (see [`EXECUTABLE_EXTENSIONS`] for the reasons). The
/// name is sanitised first, so `evil.exe.` and `evil.exe\u{0}` count as `.exe`. Either the
/// extension or the declared MIME type is enough.
pub fn is_executable_type(name: &str, mime: &str) -> bool {
    let sanitized = sanitize_filename(name);
    let by_name = sanitized
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_lowercase())
        .is_some_and(|ext| EXECUTABLE_EXTENSIONS.contains(&ext.as_str()));
    let essence = mime
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    by_name || EXECUTABLE_MIME_TYPES.contains(&essence.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dangerous_names_become_harmless() {
        let cases: &[(&str, &str)] = &[
            ("../../x", "x"),
            ("../../../evil.sh", "evil.sh"),
            ("..\\..\\boot.ini", "boot.ini"),
            ("C:\\Windows\\System32\\evil.bat", "evil.bat"),
            ("C:evil.bat", "C_evil.bat"),
            ("/etc/passwd", "passwd"),
            ("dir/", "dir"),
            ("..", FALLBACK_NAME),
            ("../..", FALLBACK_NAME),
            ("CON", "_CON"),
            ("con", "_con"),
            ("aux.txt", "_aux.txt"),
            ("NUL .txt", "_NUL .txt"),
            ("LPT1", "_LPT1"),
            ("com9.log", "_com9.log"),
            ("COM¹.txt", "_COM¹.txt"),
            ("CONOUT$", "_CONOUT$"),
            ("CONSOLE.txt", "CONSOLE.txt"),
            ("evil.txt\0.exe", "evil.txt.exe"),
            ("\0", FALLBACK_NAME),
            ("invoice\u{202E}fdp.exe", "invoicefdp.exe"),
            ("a\u{2066}b\u{2069}c\u{200E}d\u{200F}e.pdf", "abcde.pdf"),
            ("zero\u{200B}width\u{FEFF}.txt", "zerowidth.txt"),
            ("tab\tand\x07bell.txt", "tab andbell.txt"),
            ("trailing dots and spaces. . .", "trailing dots and spaces"),
            ("evil.exe.", "evil.exe"),
            ("evil.exe   ", "evil.exe"),
            ("...hidden", "hidden"),
            (".bashrc", "attachment.bashrc"),
            ("a<b>c:d\"e|f?g*h.txt", "a_b_c_d_e_f_g_h.txt"),
            ("  many   spaces\u{00A0}here .txt", "many spaces here .txt"),
            ("", FALLBACK_NAME),
            ("   ", FALLBACK_NAME),
            ("📎 Notizen.txt", "📎 Notizen.txt"),
            ("議事録.txt", "議事録.txt"),
        ];
        for (input, expected) in cases {
            assert_eq!(sanitize_filename(input), *expected, "input {input:?}");
        }
    }

    #[test]
    fn names_are_nfc() {
        // "Ärger" decomposed (A + combining diaeresis) becomes the composed form.
        assert_eq!(sanitize_filename("A\u{0308}rger.txt"), "\u{00C4}rger.txt");
    }

    #[test]
    fn long_names_keep_their_extension() {
        let long = format!("{}.txt", "x".repeat(300));
        let out = sanitize_filename(&long);
        assert!(out.len() <= MAX_NAME_BYTES);
        assert!(out.ends_with(".txt"));
        let umlauts = format!("{}.pdf", "ä".repeat(300));
        let out = sanitize_filename(&umlauts);
        assert!(out.len() <= MAX_NAME_BYTES && out.ends_with(".pdf"));
        let no_ext = "y".repeat(400);
        assert_eq!(sanitize_filename(&no_ext).len(), MAX_NAME_BYTES);
    }

    #[test]
    fn missing_names_get_a_neutral_one() {
        assert_eq!(
            attachment_file_name(None, "application/pdf"),
            "attachment.pdf"
        );
        assert_eq!(
            attachment_file_name(Some("../"), "image/png"),
            "attachment.png"
        );
        assert_eq!(
            attachment_file_name(None, "application/x-unknown"),
            "attachment"
        );
        assert_eq!(attachment_file_name(Some("a.pdf"), "image/png"), "a.pdf");
    }

    #[test]
    fn executable_types_are_recognised() {
        for name in [
            "setup.exe",
            "INVOICE.PDF.EXE",
            "evil.exe.",
            "evil.exe\0",
            "invoice\u{202E}fdp.exe",
            "run.command",
            "Installer.pkg",
            "disk.dmg",
            "script.ps1",
            "macro.xlsm",
            "page.html",
            "image.svg",
            "app.jar",
            "link.lnk",
            "x.sh",
            // Stage 15 review.
            "Remote.rdp",
            "Notes.one",
            "query.iqy",
            "Setup.appref-ms",
            "dark.theme",
            "fix.diagcab",
            "data.accde",
            "tool.pyz",
            "root.cer",
            "Run.shortcut",
        ] {
            assert!(
                is_executable_type(name, "application/octet-stream"),
                "{name}"
            );
        }
        for name in [
            "report.pdf",
            "photo.jpg",
            "notes.txt",
            "archive.zip",
            "table.xlsx",
        ] {
            assert!(
                !is_executable_type(name, "application/octet-stream"),
                "{name}"
            );
        }
        assert!(is_executable_type("noext", "application/x-msdownload"));
        assert!(is_executable_type("noext", "Text/HTML; charset=utf-8"));
        assert!(is_executable_type("noext", "application/x-rdp"));
        assert!(!is_executable_type("noext", "application/pdf"));
    }
}
