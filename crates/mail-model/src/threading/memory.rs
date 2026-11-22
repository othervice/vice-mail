//! In-memory threads of one account: the reference implementation of [`ThreadLookup`] and of
//! applying a [`ThreadAssignment`]. Used by tests and for rebuilding threads in one pass; the
//! store keeps the same state in tables.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::convert::Infallible;

use super::{
    ThreadAssignment, ThreadConfig, ThreadId, ThreadLookup, ThreadMessage, ThreadTarget,
    assign_thread, own_id, subject_key,
};
use crate::MessageId;

#[derive(Debug, Default)]
struct Members {
    messages: BTreeSet<MessageId>,
    ids: Vec<String>,
}

/// Threads of one account in memory.
#[derive(Debug)]
pub struct ThreadIndex {
    config: ThreadConfig,
    next: i64,
    /// Message-id (own or referenced) → thread.
    links: HashMap<String, ThreadId>,
    /// Own Message-ID → subjects of the messages carrying it.
    owners: HashMap<String, Vec<String>>,
    /// Subject key → messages.
    by_subject: HashMap<String, Vec<MessageId>>,
    messages: HashMap<MessageId, (ThreadId, ThreadMessage)>,
    threads: BTreeMap<ThreadId, Members>,
}

impl ThreadIndex {
    pub fn new(config: ThreadConfig) -> Self {
        Self {
            config,
            next: 1,
            links: HashMap::new(),
            owners: HashMap::new(),
            by_subject: HashMap::new(),
            messages: HashMap::new(),
            threads: BTreeMap::new(),
        }
    }

    /// The decision for `message` against the current state ([`assign_thread`]).
    pub fn assign(&self, message: &ThreadMessage) -> ThreadAssignment {
        match assign_thread(self, &self.config, message) {
            Ok(assignment) => assignment,
            Err(never) => match never {},
        }
    }

    /// Threads `message` and records it. A message inserted before keeps its thread.
    pub fn insert(&mut self, message: ThreadMessage) -> ThreadId {
        if let Some((thread, _)) = self.messages.get(&message.message) {
            return *thread;
        }
        let assignment = self.assign(&message);
        self.apply(message, &assignment)
    }

    /// Records `message` as `assignment` says (what the store does in one transaction).
    pub fn apply(&mut self, message: ThreadMessage, assignment: &ThreadAssignment) -> ThreadId {
        let thread = match assignment.target {
            ThreadTarget::Existing(thread) => thread,
            ThreadTarget::New => {
                let thread = ThreadId(self.next);
                self.next += 1;
                thread
            }
        };
        let mut target = self.threads.remove(&thread).unwrap_or_default();
        for absorbed in &assignment.absorbed {
            let Some(members) = self.threads.remove(absorbed) else {
                continue;
            };
            for id in &members.messages {
                if let Some(entry) = self.messages.get_mut(id) {
                    entry.0 = thread;
                }
            }
            for id in &members.ids {
                self.links.insert(id.clone(), thread);
            }
            target.messages.extend(members.messages);
            target.ids.extend(members.ids);
        }
        for id in &assignment.link_ids {
            if self.links.insert(id.clone(), thread) != Some(thread) {
                target.ids.push(id.clone());
            }
        }
        if let Some(own) = message.message_id.as_deref().and_then(own_id) {
            self.owners
                .entry(own.to_owned())
                .or_default()
                .push(message.subject.clone());
        }
        if let Some(key) = subject_key(&message.subject) {
            self.by_subject
                .entry(key)
                .or_default()
                .push(message.message);
        }
        target.messages.insert(message.message);
        self.threads.insert(thread, target);
        self.messages.insert(message.message, (thread, message));
        thread
    }

    /// The thread of a message, if it was inserted.
    pub fn thread_of(&self, message: MessageId) -> Option<ThreadId> {
        self.messages.get(&message).map(|(thread, _)| *thread)
    }

    /// The messages of a thread, ascending.
    pub fn messages_of(&self, thread: ThreadId) -> Vec<MessageId> {
        self.threads
            .get(&thread)
            .map(|m| m.messages.iter().copied().collect())
            .unwrap_or_default()
    }

    /// All threads as sets of messages, each ascending, ordered by their first message: the
    /// grouping without thread ids, comparable across runs.
    pub fn partition(&self) -> Vec<Vec<MessageId>> {
        let mut groups: Vec<Vec<MessageId>> = self
            .threads
            .values()
            .map(|m| m.messages.iter().copied().collect())
            .collect();
        groups.sort();
        groups
    }

    /// Number of threads.
    pub fn len(&self) -> usize {
        self.threads.len()
    }

    pub fn is_empty(&self) -> bool {
        self.threads.is_empty()
    }
}

impl ThreadLookup for ThreadIndex {
    type Error = Infallible;

    fn threads_with_ids(&self, ids: &[&str]) -> Result<Vec<ThreadId>, Infallible> {
        Ok(ids
            .iter()
            .filter_map(|id| self.links.get(*id).copied())
            .collect())
    }

    fn subjects_with_message_id(&self, id: &str) -> Result<Vec<String>, Infallible> {
        Ok(self.owners.get(id).cloned().unwrap_or_default())
    }

    fn messages_with_subject(
        &self,
        key: &str,
        from: i64,
        until: i64,
    ) -> Result<Vec<(ThreadId, ThreadMessage)>, Infallible> {
        let Some(messages) = self.by_subject.get(key) else {
            return Ok(Vec::new());
        };
        Ok(messages
            .iter()
            .filter_map(|id| self.messages.get(id))
            .filter(|(_, m)| (from..=until).contains(&m.date))
            .map(|(thread, m)| (*thread, m.clone()))
            .collect())
    }
}
