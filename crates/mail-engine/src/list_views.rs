//! Stage 19, task A (decisions 0061 and 0062): sorted and filtered list views, the order the
//! user chose per view, and the table presentation of the wide list.
//!
//! The quick filter reuses the search (decision 0013): its text is parsed and compiled like a
//! search query typed in the view (scope, `account:`, `in:`, dates, `from:` and the other
//! operators, the last word as a prefix), its switches set the compiled filter's flags (a
//! contradiction with the text, `is:read` with "Unread", matches nothing), and only the tag and
//! "From contacts" are conditions of their own. The store runs it as a sorted list
//! (`mail_store::views`), not as a search: the result is the view, in its order.
//!
//! | Key | Content | Sync |
//! |---|---|---|
//! | `ui.list_orders` | [`ViewOrder`] per view key (the default order is not stored) | device |
//! | `ui.list_table` | [`ListTableSettings`] | device |
//!
//! Both stay on the device: view keys name local mailbox ids (like "Unread first", stage 16),
//! and the table's widths are sizes of this screen (decision 0025).

use mail_model::{
    CoreError, InboxScope, ListOrder, ListTableSettings, ListViewRequest, MAX_LIST_ORDERS,
    MAX_VIEW_KEY_CHARS, MAX_VIEW_TAGS, SearchRequest, SearchScope, UnifiedView, ViewCursor,
    ViewMessagePage, ViewOrder, ViewThreadPage,
};
use mail_store::{GLOBAL_SCOPE, InboxMailboxes, ViewFilter, ViewQuery};

use crate::AppCore;
use crate::core::Connector;
use crate::search::{LocalZone, resolve};

/// Device setting: the order of every view that is not newest first.
pub(crate) const LIST_ORDERS_KEY: &str = "ui.list_orders";
/// Device setting: the list's presentation and the table's columns.
pub(crate) const LIST_TABLE_KEY: &str = "ui.list_table";

fn mailboxes_of(scope: &InboxScope) -> InboxMailboxes {
    match scope {
        InboxScope::Mailbox { mailbox_id } => InboxMailboxes::Mailbox(*mailbox_id),
        InboxScope::Unified { view } => InboxMailboxes::Unified(match view {
            UnifiedView::All => None,
            UnifiedView::Group { group_id } => Some(group_id.clone()),
        }),
    }
}

fn search_scope(scope: &InboxScope) -> SearchScope {
    match scope {
        InboxScope::Mailbox { mailbox_id } => SearchScope::Mailbox {
            mailbox_id: *mailbox_id,
        },
        InboxScope::Unified {
            view: UnifiedView::All,
        } => SearchScope::All,
        InboxScope::Unified {
            view: UnifiedView::Group { group_id },
        } => SearchScope::Group {
            group_id: group_id.clone(),
        },
    }
}

/// A switch of the quick filter on top of what the text says about the same flag: on and the
/// text wants the opposite matches nothing (`None`).
fn switched(on: bool, from_text: Option<bool>) -> Option<Option<bool>> {
    match (on, from_text) {
        (false, text) => Some(text),
        (true, Some(false)) => None,
        (true, _) => Some(Some(true)),
    }
}

fn check_view_key(view: &str) -> Result<String, CoreError> {
    let view = view.trim().to_owned();
    if view.is_empty()
        || view.chars().count() > MAX_VIEW_KEY_CHARS
        || view.chars().any(char::is_control)
    {
        return Err(CoreError::InvalidArgument(format!(
            "a view key is 1 to {MAX_VIEW_KEY_CHARS} characters without controls"
        )));
    }
    Ok(view)
}

fn encode<T: serde::Serialize>(value: &T, what: &str) -> Result<String, CoreError> {
    serde_json::to_string(value).map_err(|err| CoreError::Internal(format!("{what}: {err}")))
}

impl<C: Connector> AppCore<C> {
    /// The store's query for a view, or `None` when nothing can match (a contradiction, an
    /// unknown account or folder in the text).
    async fn view_query(&self, request: ListViewRequest) -> Result<Option<ViewQuery>, CoreError> {
        let ListViewRequest {
            scope,
            order,
            filter,
        } = request;
        filter.validate().map_err(CoreError::InvalidArgument)?;
        let store = self.store_ref()?;
        let search = SearchRequest {
            query: filter.text.clone(),
            scope: search_scope(&scope),
            cursor: None,
            limit: 1,
            typing: true,
            month_first: filter.month_first,
        };
        let options = mail_model::search::CompileOptions {
            prefix_last_word: true,
            substrings: resolve::substrings_now(store).await?,
        };
        let compiled = {
            let zone = LocalZone::system();
            let context = self.search_context(&zone, &search);
            resolve::compile(&search, &context, options)
        };
        let plan = resolve::plan(store, &search, compiled).await?;
        if plan.matches_nothing {
            return Ok(None);
        }
        let mut search_filter = plan.filter;
        // The view's own mailbox is the first set of a mailbox scope; the view walks it.
        if matches!(scope, InboxScope::Mailbox { .. }) && !search_filter.mailbox_sets.is_empty() {
            search_filter.mailbox_sets.remove(0);
        }
        let (Some(unread), Some(flagged), Some(attachment)) = (
            switched(filter.unread, search_filter.unread),
            switched(filter.flagged, search_filter.flagged),
            switched(filter.attachment, search_filter.has_attachment),
        ) else {
            return Ok(None);
        };
        search_filter.unread = unread;
        search_filter.flagged = flagged;
        search_filter.has_attachment = attachment;
        Ok(Some(ViewQuery {
            scope: mailboxes_of(&scope),
            order,
            filter: ViewFilter {
                search: search_filter,
                contacts: filter.contacts,
                keyword: filter.tag,
            },
        }))
    }

    /// Stage 19: one page of a view in the given order and with the quick filter, one row per
    /// message (see the module docs). `limit` 1..=500; `cursor` is the previous page's.
    pub async fn list_view_messages(
        &self,
        request: ListViewRequest,
        cursor: Option<ViewCursor>,
        limit: u32,
    ) -> Result<ViewMessagePage, CoreError> {
        let Some(query) = self.view_query(request).await? else {
            return Ok(ViewMessagePage {
                items: Vec::new(),
                next_cursor: None,
            });
        };
        Ok(self
            .store_ref()?
            .view_messages(query, cursor, limit)
            .await?)
    }

    /// Stage 19: the same as conversations (decision 0039): each conversation once, at the
    /// place of its newest message in the view that matches the filter.
    pub async fn list_view_conversations(
        &self,
        request: ListViewRequest,
        cursor: Option<ViewCursor>,
        limit: u32,
    ) -> Result<ViewThreadPage, CoreError> {
        let Some(query) = self.view_query(request).await? else {
            return Ok(ViewThreadPage {
                items: Vec::new(),
                next_cursor: None,
            });
        };
        Ok(self
            .store_ref()?
            .view_conversations(query, cursor, limit)
            .await?)
    }

    /// Stage 19: how many messages of the view match the quick filter (the order does not
    /// matter; a message in two mailboxes of a unified view counts twice).
    pub async fn count_list_view(&self, request: ListViewRequest) -> Result<u32, CoreError> {
        let Some(query) = self.view_query(request).await? else {
            return Ok(0);
        };
        let count = self.store_ref()?.view_count(query).await?;
        Ok(u32::try_from(count).unwrap_or(u32::MAX))
    }

    /// Stage 19: the tags in the view's mailboxes, for the quick filter's tag menu (keywords
    /// not starting with `$`, each once, sorted, at most [`MAX_VIEW_TAGS`]).
    pub async fn list_view_tags(&self, scope: InboxScope) -> Result<Vec<String>, CoreError> {
        Ok(self
            .store_ref()?
            .view_tags(mailboxes_of(&scope), MAX_VIEW_TAGS)
            .await?)
    }

    /// Stage 19: the views with an order of their own (device setting).
    pub async fn get_list_orders(&self) -> Result<Vec<ViewOrder>, CoreError> {
        let settings = self.store_ref()?.settings(GLOBAL_SCOPE).await?;
        Ok(settings
            .iter()
            .find(|s| s.key == LIST_ORDERS_KEY)
            .and_then(|s| serde_json::from_str::<Vec<ViewOrder>>(&s.value).ok())
            .unwrap_or_default())
    }

    /// Stage 19: sets the order of `view`; `None` or the default order (newest first) removes
    /// it. Returns every view with an order of its own.
    pub async fn set_list_order(
        &self,
        view: String,
        order: Option<ListOrder>,
    ) -> Result<Vec<ViewOrder>, CoreError> {
        let view = check_view_key(&view)?;
        let mut orders = self.get_list_orders().await?;
        orders.retain(|o| o.view != view);
        if let Some(order) = order.filter(|o| !o.is_default()) {
            if orders.len() >= MAX_LIST_ORDERS {
                return Err(CoreError::InvalidArgument(format!(
                    "at most {MAX_LIST_ORDERS} views keep an order of their own"
                )));
            }
            orders.push(ViewOrder { view, order });
        }
        self.store_ref()?
            .put_settings(
                GLOBAL_SCOPE,
                vec![(LIST_ORDERS_KEY.to_owned(), encode(&orders, "list orders")?)],
            )
            .await?;
        tracing::debug!(views = orders.len(), "list orders stored");
        Ok(orders)
    }

    /// Stage 19: the list's presentation and the table's columns on this device; the defaults
    /// for what is not stored, a stored value normalized (unknown columns dropped, widths out of
    /// range back at their default).
    pub async fn get_list_table(&self) -> Result<ListTableSettings, CoreError> {
        let settings = self.store_ref()?.settings(GLOBAL_SCOPE).await?;
        let stored = settings.iter().find(|s| s.key == LIST_TABLE_KEY);
        let Some(stored) = stored else {
            return Ok(ListTableSettings::default());
        };
        match serde_json::from_str::<ListTableSettings>(&stored.value) {
            Ok(value) => Ok(value.normalized()),
            Err(err) => {
                tracing::warn!(%err, "stored list table settings unreadable; defaults apply");
                Ok(ListTableSettings::default())
            }
        }
    }

    /// Stage 19: stores the list's presentation and the table's columns (checked) and returns
    /// them in the table's order.
    pub async fn set_list_table(
        &self,
        settings: ListTableSettings,
    ) -> Result<ListTableSettings, CoreError> {
        settings.validate().map_err(CoreError::InvalidArgument)?;
        let settings = settings.normalized();
        self.store_ref()?
            .put_settings(
                GLOBAL_SCOPE,
                vec![(LIST_TABLE_KEY.to_owned(), encode(&settings, "list table")?)],
            )
            .await?;
        tracing::debug!(presentation = ?settings.presentation, "list table stored");
        Ok(settings)
    }
}

#[cfg(test)]
#[path = "list_views_tests.rs"]
mod tests;
