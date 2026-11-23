//! Calendar invitations (stage 9): the `text/calendar` parts of a stored message, read by
//! `mail_content::read_calendar` with the system's time-zone database, and handed to the
//! system's calendar as an `.ics` file. Replying to invitations is not part of version 1.

use jiff::civil::DateTime;
use jiff::tz::TimeZone;
use mail_content::{ZoneResolver, calendar_parts, read_calendar};
use mail_model::{CalendarInviteInfo, CoreError, MessageExtras, MessageId};

use crate::core::{AppCore, Connector};

/// The time-zone database of the system (jiff); floating times in the user's zone.
pub(crate) struct SystemZones {
    pub local: TimeZone,
}

impl ZoneResolver for SystemZones {
    fn to_utc(&self, tzid: Option<&str>, local: DateTime) -> Option<i64> {
        let zone = match tzid {
            Some(name) => TimeZone::get(name).ok()?,
            None => self.local.clone(),
        };
        zone.to_ambiguous_timestamp(local)
            .compatible()
            .ok()
            .map(|ts| ts.as_second())
    }
}

impl<C: Connector> AppCore<C> {
    /// What the reading pane shows besides the body: "Why here?", the unsubscribe options and
    /// the calendar invitations (those only when the body is stored whole).
    pub async fn get_message_extras(&self, message: MessageId) -> Result<MessageExtras, CoreError> {
        let category = self.get_message_category(message).await?;
        let unsubscribe = self.get_unsubscribe_options(message).await?;
        let store = self.store_ref()?;
        let raw = match store.body_info(message).await? {
            Some(info) if info.complete => store.load_message(message).await?,
            _ => None,
        };
        let body_complete = raw.is_some();
        let invites = match raw {
            Some(raw) => {
                let local = self.comfort().zone();
                tokio::task::spawn_blocking(move || {
                    let zones = SystemZones { local };
                    calendar_parts(&raw)
                        .into_iter()
                        .map(|part| CalendarInviteInfo {
                            part_id: part.part_id,
                            invite: read_calendar(&part.data, &zones),
                        })
                        .collect::<Vec<_>>()
                })
                .await
                .map_err(|err| CoreError::Internal(format!("calendar task: {err}")))?
            }
            None => Vec::new(),
        };
        Ok(MessageExtras {
            category,
            unsubscribe,
            invites,
            body_complete,
        })
    }

    /// "Im Kalender öffnen": writes the calendar part as a read-only `.ics` file into the
    /// app's directory for opened files and hands it to the system's default calendar app.
    pub async fn open_invite(&self, message: MessageId, part_id: u32) -> Result<(), CoreError> {
        let store = self.store_ref()?;
        let raw = match store.body_info(message).await? {
            Some(info) if info.complete => store.load_message(message).await?,
            _ => None,
        }
        .ok_or_else(|| {
            CoreError::NotFound(format!("the body of message {message} is not stored"))
        })?;
        let part = tokio::task::spawn_blocking(move || {
            calendar_parts(&raw)
                .into_iter()
                .find(|p| p.part_id == part_id)
        })
        .await
        .map_err(|err| CoreError::Internal(format!("calendar task: {err}")))?
        .ok_or_else(|| {
            CoreError::NotFound(format!("calendar part {part_id} of message {message}"))
        })?;
        self.reading_ref()?
            .open_file_copy("invite.ics".into(), part.data)
            .await?;
        tracing::info!(message = message.0, "invitation handed to the calendar");
        Ok(())
    }
}
