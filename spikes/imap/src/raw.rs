//! Workarounds for what async-imap 0.12 hides: tagged status of stream commands, response codes
//! (APPENDUID, COPYUID, MODIFIED, READ-ONLY), extension responses (ENABLED, VANISHED).
//! Built on the public `run_command` / `read_response` primitives and `imap_proto` types.

use async_imap::imap_proto::rfc4315::UidSetMember;
use async_imap::imap_proto::{AttributeValue, BodyStructure, Envelope, Response, ResponseCode, Status};
/// async-imap 0.12 re-exports `ResponseData` only as `pub(crate)`, so the type cannot be named
/// although public methods return it. It can be reached through a public associated type.
pub trait IoOk {
    type T;
}
impl<X> IoOk for std::io::Result<X> {
    type T = X;
}
pub type ResponseData =
    <<async_imap::extensions::idle::Handle<crate::net::Stream> as futures::Stream>::Item as IoOk>::T;
use tokio::io::AsyncWriteExt;

use crate::errors::{NetError, from_imap, from_io};
use crate::net::Sess;

#[derive(Debug, Clone)]
pub struct Done {
    pub status: &'static str,
    pub code: Option<String>,
    pub info: Option<String>,
    pub append_uid: Option<(u32, String)>,
    pub copy_uid: Option<(u32, String, String)>,
    pub read_only: bool,
}

impl Done {
    pub fn from_resp(resp: &Response<'_>) -> Option<Done> {
        let (status, outcome) = match resp {
            Response::Done { status, outcome, .. } => (status, outcome),
            Response::Data { status, outcome } => (status, outcome),
            _ => return None,
        };
        let status = match status {
            Status::Ok => "OK",
            Status::No => "NO",
            Status::Bad => "BAD",
            Status::PreAuth => "PREAUTH",
            Status::Bye => "BYE",
        };
        let mut d = Done {
            status,
            code: outcome.code.as_ref().map(|c| format!("{c:?}")),
            info: outcome.information.as_ref().map(|s| s.to_string()),
            append_uid: None,
            copy_uid: None,
            read_only: false,
        };
        match &outcome.code {
            Some(ResponseCode::AppendUid(v, uids)) => d.append_uid = Some((*v, set_str(uids))),
            Some(ResponseCode::CopyUid(v, from, to)) => d.copy_uid = Some((*v, set_str(from), set_str(to))),
            Some(ResponseCode::ReadOnly) => d.read_only = true,
            _ => {}
        }
        Some(d)
    }
    pub fn ok(&self) -> bool {
        self.status == "OK"
    }
    /// `[MODIFIED 1,3:5]` is unknown to imap-proto 0.17: the code stays None and the text keeps it.
    pub fn modified(&self) -> Option<String> {
        let text = self.info.as_deref()?;
        let start = text.find("[MODIFIED ")? + "[MODIFIED ".len();
        let end = text[start..].find(']')? + start;
        Some(text[start..end].to_string())
    }
    pub fn into_result(self) -> Result<Done, NetError> {
        match self.status {
            "OK" => Ok(self),
            "NO" => Err(NetError::No(format!("{:?} {:?}", self.code, self.info))),
            _ => Err(NetError::Bad(format!("{:?} {:?}", self.code, self.info))),
        }
    }
}

pub fn set_str(members: &[UidSetMember]) -> String {
    members
        .iter()
        .map(|m| match m {
            UidSetMember::Uid(u) => u.to_string(),
            UidSetMember::UidRange(r) => format!("{}:{}", r.start(), r.end()),
        })
        .collect::<Vec<_>>()
        .join(",")
}

pub struct Raw {
    pub untagged: Vec<ResponseData>,
    pub done: Done,
}

impl Raw {
    pub fn vanished(&self) -> Vec<(bool, String)> {
        self.untagged
            .iter()
            .filter_map(|r| match r.parsed() {
                Response::Vanished { earlier, uids } => Some((
                    earlier.to_owned(),
                    uids.iter()
                        .map(|r| format!("{}:{}", r.start(), r.end()))
                        .collect::<Vec<_>>()
                        .join(","),
                )),
                _ => None,
            })
            .collect()
    }
}

/// Runs any command and collects untagged responses until the matching tagged response.
pub async fn raw(s: &mut Sess, cmd: &str) -> Result<Raw, NetError> {
    let tag = s.run_command(cmd).await.map_err(from_imap)?;
    let mut untagged = Vec::new();
    loop {
        let r = s
            .read_response()
            .await
            .map_err(|e| from_io(&e))?
            .ok_or_else(|| NetError::ConnectionLost("closed during command".into()))?;
        if let Response::Done { tag: t, .. } = r.parsed()
            && *t == tag
        {
            let done = Done::from_resp(r.parsed()).expect("done");
            return Ok(Raw { untagged, done });
        }
        untagged.push(r);
    }
}

/// APPEND with a synchronizing literal; returns the tagged response with APPENDUID.
pub async fn append(
    s: &mut Sess,
    mailbox_wire: &str,
    flags: &str,
    date: Option<&str>,
    content: &[u8],
) -> Result<Done, NetError> {
    let date = date.map(|d| format!(" \"{d}\"")).unwrap_or_default();
    let cmd = format!("APPEND \"{mailbox_wire}\" ({flags}){date} {{{}}}", content.len());
    let tag = s.run_command(&cmd).await.map_err(from_imap)?;
    loop {
        let r = s
            .read_response()
            .await
            .map_err(|e| from_io(&e))?
            .ok_or_else(|| NetError::ConnectionLost("closed during APPEND".into()))?;
        match r.parsed() {
            Response::Continue { .. } => break,
            Response::Done { tag: t, .. } if *t == tag => {
                return Done::from_resp(r.parsed()).expect("done").into_result();
            }
            _ => {}
        }
    }
    let w = s.as_mut();
    w.write_all(content).await.map_err(|e| from_io(&e))?;
    w.write_all(b"\r\n").await.map_err(|e| from_io(&e))?;
    w.flush().await.map_err(|e| from_io(&e))?;
    loop {
        let r = s
            .read_response()
            .await
            .map_err(|e| from_io(&e))?
            .ok_or_else(|| NetError::ConnectionLost("closed during APPEND".into()))?;
        if let Response::Done { tag: t, .. } = r.parsed()
            && *t == tag
        {
            return Done::from_resp(r.parsed()).expect("done").into_result();
        }
    }
}

/// async-imap's `Flag` back to the wire form (`\\Seen`, `$label1`).
pub fn flag_wire(f: &async_imap::types::Flag<'_>) -> String {
    use async_imap::types::Flag as F;
    match f {
        F::Seen => "\\Seen".into(),
        F::Answered => "\\Answered".into(),
        F::Flagged => "\\Flagged".into(),
        F::Deleted => "\\Deleted".into(),
        F::Draft => "\\Draft".into(),
        F::Recent => "\\Recent".into(),
        F::MayCreate => "\\*".into(),
        F::Custom(c) => c.to_string(),
    }
}

// ---------------------------------------------------------------------------------------------
// Header records: what a store would keep per message after the initial sync.

#[derive(Debug, Clone, Default)]
pub struct HeaderRec {
    pub uid: u32,
    pub modseq: Option<u64>,
    pub flags: Vec<String>,
    pub size: Option<u32>,
    pub internal_date: Option<String>,
    pub subject: Option<String>,
    pub from: Option<String>,
    pub to_count: usize,
    pub date: Option<String>,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub has_envelope: bool,
    pub parts: u32,
    pub depth: u32,
    pub has_bodystructure: bool,
    pub header_fields: Option<Vec<u8>>,
}

fn lossy(b: &Option<std::borrow::Cow<'_, [u8]>>) -> Option<String> {
    b.as_ref().map(|b| String::from_utf8_lossy(b).into_owned())
}

pub fn walk(bs: &BodyStructure<'_>, depth: u32, parts: &mut u32, max_depth: &mut u32) {
    *max_depth = (*max_depth).max(depth);
    match bs {
        BodyStructure::Multipart { bodies, .. } => {
            for b in bodies {
                walk(b, depth + 1, parts, max_depth);
            }
        }
        BodyStructure::Message { body, .. } => {
            *parts += 1;
            walk(body, depth + 1, parts, max_depth);
        }
        _ => *parts += 1,
    }
}

impl HeaderRec {
    fn set_envelope(&mut self, e: &Envelope<'_>) {
        self.has_envelope = true;
        self.subject = lossy(&e.subject);
        self.date = lossy(&e.date);
        self.message_id = lossy(&e.message_id);
        self.in_reply_to = lossy(&e.in_reply_to);
        self.from = e.from.as_ref().and_then(|v| v.first()).map(|a| {
            format!(
                "{}@{}",
                a.mailbox
                    .as_ref()
                    .map(|m| String::from_utf8_lossy(m).into_owned())
                    .unwrap_or_default(),
                a.host
                    .as_ref()
                    .map(|m| String::from_utf8_lossy(m).into_owned())
                    .unwrap_or_default()
            )
        });
        self.to_count = e.to.as_ref().map(|v| v.len()).unwrap_or(0);
    }
    fn set_bodystructure(&mut self, bs: &BodyStructure<'_>) {
        self.has_bodystructure = true;
        let (mut parts, mut depth) = (0, 0);
        walk(bs, 0, &mut parts, &mut depth);
        self.parts = parts;
        self.depth = depth;
    }

    pub fn from_attrs(attrs: &[AttributeValue<'_>]) -> HeaderRec {
        let mut h = HeaderRec::default();
        for a in attrs {
            match a {
                AttributeValue::Uid(u) => h.uid = *u,
                AttributeValue::ModSeq(m) => h.modseq = Some(*m),
                AttributeValue::Flags(f) => h.flags = f.iter().map(|s| s.to_string()).collect(),
                AttributeValue::Rfc822Size(s) => h.size = Some(*s),
                AttributeValue::InternalDate(d) => h.internal_date = Some(d.to_string()),
                AttributeValue::Envelope(e) => h.set_envelope(e),
                AttributeValue::BodyStructure(bs) => h.set_bodystructure(bs),
                AttributeValue::BodySection { data, .. } => h.header_fields = data.as_ref().map(|d| d.to_vec()),
                _ => {}
            }
        }
        h
    }

    pub fn from_fetch(f: &async_imap::types::Fetch) -> HeaderRec {
        let mut h = HeaderRec {
            uid: f.uid.unwrap_or(0),
            modseq: f.modseq,
            size: f.size,
            flags: f.flags().map(|fl| flag_wire(&fl)).collect(),
            internal_date: f.internal_date().map(|d| d.to_rfc3339()),
            header_fields: f.header().map(|h| h.to_vec()),
            ..Default::default()
        };
        if let Some(e) = f.envelope() {
            h.set_envelope(e);
        }
        if let Some(bs) = f.bodystructure() {
            h.set_bodystructure(bs);
        }
        h
    }

    /// Which requested parts are missing (empty = everything parsed).
    pub fn missing(&self) -> Vec<&'static str> {
        let mut m = Vec::new();
        if self.uid == 0 {
            m.push("UID");
        }
        if self.modseq.is_none() {
            m.push("MODSEQ");
        }
        if self.size.is_none() {
            m.push("RFC822.SIZE");
        }
        if self.internal_date.is_none() {
            m.push("INTERNALDATE");
        }
        if !self.has_envelope {
            m.push("ENVELOPE");
        }
        if !self.has_bodystructure {
            m.push("BODYSTRUCTURE");
        }
        if self.header_fields.is_none() {
            m.push("HEADER.FIELDS");
        }
        m
    }

    /// Unfolded value of one header field from the HEADER.FIELDS block.
    pub fn header(&self, name: &str) -> Option<String> {
        let block = String::from_utf8_lossy(self.header_fields.as_deref()?).into_owned();
        let mut value: Option<String> = None;
        for line in block.split('\n') {
            let line = line.trim_end_matches('\r');
            if line.starts_with([' ', '\t']) {
                if let Some(v) = value.as_mut() {
                    v.push(' ');
                    v.push_str(line.trim());
                }
                continue;
            }
            if value.is_some() {
                break;
            }
            if let Some((k, v)) = line.split_once(':')
                && k.trim().eq_ignore_ascii_case(name)
            {
                value = Some(v.trim().to_string());
            }
        }
        value
    }
}

/// UID FETCH that, unlike async-imap's stream, checks the tagged status and therefore notices a
/// NO/BAD or a connection that ends between two responses.
pub async fn fetch_checked(
    s: &mut Sess,
    uid_set: &str,
    query: &str,
    out: &mut Vec<HeaderRec>,
) -> Result<Done, NetError> {
    let tag = s
        .run_command(format!("UID FETCH {uid_set} {query}"))
        .await
        .map_err(from_imap)?;
    loop {
        let r = s
            .read_response()
            .await
            .map_err(|e| from_io(&e))?
            .ok_or_else(|| NetError::ConnectionLost("closed during FETCH".into()))?;
        match r.parsed() {
            Response::Fetch(_, attrs) => out.push(HeaderRec::from_attrs(attrs)),
            Response::Done { tag: t, .. } if *t == tag => {
                return Done::from_resp(r.parsed()).expect("done").into_result();
            }
            _ => {} // unsolicited EXISTS/EXPUNGE etc.: a real client would feed them to the sync
        }
    }
}
