//! Desktop shell: window, commands and wiring. All behaviour lives in `mail-engine`; this crate
//! only forwards commands to [`AppCore`] and core events to the UI.

mod autostart;
// Drawn for Windows (taskbar overlay, notification area); tested everywhere.
#[cfg_attr(not(windows), allow(dead_code))]
mod badge;
pub mod commands;
mod default_app;
pub mod drops;
#[cfg(feature = "e2e")]
mod e2e;
mod eml_files;
mod events;
pub mod lifecycle;
pub mod links;
mod navigation;
mod notifications;
mod oauth;
pub mod popup;
mod sound;
mod tray;
mod updater;
mod window_theme;
pub mod windows;

use std::sync::Mutex;

use mail_engine::credentials::{Credentials, SystemKeychain};
use mail_engine::logging::{self, LogConfig, LogGuard};
use mail_engine::{AppCore, BuildInfo, CoreConfig, CoreServices};
use specta_typescript::Typescript;
use tauri::{AppHandle, Manager, RunEvent, Runtime, Theme, WebviewWindow};
use tauri_specta::{Builder, ErrorHandlingMode, collect_commands, collect_events};

pub use events::{CORE_EVENT_NAME, CoreEventMessage};

/// Label of the main window in `tauri.conf.json`.
pub(crate) const MAIN_WINDOW: &str = "main";

/// Single source of truth for the commands and events the UI sees (decision 0004).
pub fn specta_builder<R: Runtime>() -> Builder<R> {
    Builder::<R>::new()
        .commands(collect_commands![
            commands::app::get_app_info,
            commands::app::get_startup_problem,
            // Stage 15: "Export diagnostics…" (system save dialog, then the core writes).
            commands::app::export_diagnostics::<tauri::Wry>,
            commands::preferences::get_preferences,
            commands::preferences::set_preferences,
            commands::preferences::get_reading_preferences,
            commands::preferences::set_reading_preferences,
            // Stage 8: UI preferences, window layouts, message windows (decision 0025).
            commands::ui::get_ui_preferences,
            commands::ui::set_ui_preferences,
            commands::ui::get_window_layout::<tauri::Wry>,
            commands::ui::set_window_layout::<tauri::Wry>,
            commands::ui::open_message_window::<tauri::Wry>,
            commands::ui::open_composer_window::<tauri::Wry>,
            // Stage 19 (decision 0055): the source window and "all headers".
            commands::source::open_source_window::<tauri::Wry>,
            commands::source::get_message_source,
            commands::source::get_message_headers,
            commands::redirect::redirect_message,
            commands::redirect::cancel_redirect,
            // Stage 19 (decision 0056): contact groups.
            commands::contact_lists::list_contact_lists,
            commands::contact_lists::get_contact_list,
            commands::contact_lists::create_contact_list,
            commands::contact_lists::rename_contact_list,
            commands::contact_lists::delete_contact_list,
            commands::contact_lists::add_contact_list_members,
            commands::contact_lists::remove_contact_list_member,
            commands::contact_lists::suggest_contact_lists,
            commands::contact_lists::import_vcards::<tauri::Wry>,
            commands::contact_lists::export_vcards::<tauri::Wry>,
            // Stage 19 (decision 0057): mbox export and import.
            commands::mbox::export_mailbox_mbox::<tauri::Wry>,
            commands::mbox::import_mailbox_mbox::<tauri::Wry>,
            commands::mbox::cancel_mbox_job,
            commands::mbox::list_mbox_jobs,
            // Stage 19 (decision 0057): takeover from a Thunderbird profile.
            commands::takeover::list_thunderbird_profiles::<tauri::Wry>,
            commands::takeover::open_thunderbird_profile::<tauri::Wry>,
            commands::takeover::choose_thunderbird_profile::<tauri::Wry>,
            commands::takeover::apply_thunderbird_account,
            commands::takeover::import_thunderbird_folder,
            commands::accounts::list_accounts,
            commands::accounts::discover_known_provider,
            commands::accounts::discover_online,
            commands::accounts::test_connection,
            commands::accounts::add_account,
            commands::accounts::remove_account,
            commands::accounts::retry_account,
            commands::accounts::update_password,
            commands::oauth::get_oauth_availability,
            commands::oauth::add_oauth_account,
            commands::oauth::sign_in_again,
            commands::oauth::cancel_oauth_sign_in,
            commands::mail::list_mailboxes,
            commands::mail::set_mailbox_role,
            commands::mail::list_messages,
            commands::mail::get_messages_by_ids,
            commands::mail::get_message_labels,
            commands::mail::set_message_label,
            commands::reading::get_message,
            commands::reading::load_remote_content,
            commands::reading::list_remote_content_senders,
            commands::reading::revoke_remote_content,
            commands::reading::describe_link,
            commands::reading::open_link,
            // The runtime parameter only matters for the type export; Tauri's handler infers it.
            commands::reading::save_attachment::<tauri::Wry>,
            commands::reading::open_attachment,
            commands::reading::preview_attachment,
            commands::reading::save_all_attachments::<tauri::Wry>,
            commands::reading::get_prefetch_settings,
            commands::reading::set_prefetch_settings,
            commands::reading::get_prefetch_status,
            commands::actions::apply_action,
            commands::actions::undo_action,
            commands::actions::create_mailbox,
            commands::actions::create_folder,
            commands::actions::reload_folders,
            commands::actions::rename_mailbox,
            commands::actions::delete_mailbox,
            commands::actions::list_operations,
            commands::actions::retry_operation,
            commands::actions::discard_operation,
            commands::actions::get_outbox_counts,
            commands::actions::set_writes_enabled,
            // Stage 7: search, threads, unified inbox, groups, hidden accounts, identities.
            commands::search::search,
            commands::search::search_on_server,
            commands::search::get_search_index_status,
            commands::search::rebuild_search_index,
            // Stage 7: saved searches (synced) and recent searches (device), decision 0026.
            commands::search::list_saved_searches,
            commands::search::save_search,
            commands::search::rename_saved_search,
            commands::search::delete_saved_search,
            commands::search::list_recent_searches,
            commands::search::add_recent_search,
            commands::search::clear_recent_searches,
            commands::threads::list_threads,
            commands::threads::list_unified_threads,
            commands::threads::get_thread,
            commands::threads::list_unified_inbox,
            // Decision 0039: the sorted inbox as conversations.
            commands::threads::list_inbox_conversations,
            commands::threads::list_inbox_conversations_filtered,
            commands::threads::list_unread_first_conversations,
            commands::threads::conversation_targets,
            commands::groups::list_account_groups,
            commands::groups::create_account_group,
            commands::groups::update_account_group,
            commands::groups::delete_account_group,
            commands::groups::list_hidden_accounts,
            commands::groups::set_account_hidden,
            commands::groups::list_identities,
            commands::groups::add_identity,
            commands::groups::update_identity,
            commands::groups::set_default_identity,
            commands::groups::delete_identity,
            commands::groups::get_reply_identity,
            // Stage 5: drafts, attachments, sending, the outbox view, settings, contacts.
            commands::compose::create_draft,
            commands::compose::get_draft,
            commands::compose::save_draft,
            commands::compose::close_draft,
            commands::compose::delete_draft,
            commands::compose::list_drafts,
            commands::compose::open_draft_from_message,
            commands::compose::pick_attachments::<tauri::Wry>,
            commands::compose::add_dropped_attachments,
            commands::compose::remove_draft_attachment,
            commands::compose::send_draft,
            commands::compose::cancel_send,
            commands::compose::list_outgoing,
            commands::compose::resolve_unclear_send,
            commands::compose::get_compose_settings,
            commands::compose::set_compose_settings,
            commands::compose::get_send_settings,
            commands::compose::set_sent_copy_mode,
            commands::compose::list_compose_identities,
            commands::compose::set_identity_signature,
            commands::compose::search_contacts,
            // Stage 16 (decision 0033): send options, read receipts.
            commands::compose::set_draft_send_options,
            commands::compose::get_read_receipt,
            commands::compose::send_read_receipt,
            commands::compose::decline_read_receipt,
            // Stage 10: send later, size limit, inline images, pasted HTML (decision 0023).
            commands::composer::schedule_send,
            commands::composer::list_scheduled_sends,
            commands::composer::reschedule_send,
            commands::composer::resolve_missed_send,
            commands::composer::check_draft_size,
            commands::composer::pick_inline_images::<tauri::Wry>,
            commands::composer::add_dropped_inline_images,
            commands::composer::inline_image_data,
            commands::composer::clean_pasted_html,
            commands::contacts::list_contacts,
            commands::contacts::get_contact,
            commands::contacts::set_contact_name,
            commands::contacts::list_contact_duplicates,
            commands::contacts::dismiss_contact_duplicate,
            commands::contacts::merge_contacts,
            commands::contacts::separate_contact,
            // Stage 17 (decision 0050): the contacts view.
            commands::contacts::set_contact_note,
            commands::contacts::pick_contact_picture::<tauri::Wry>,
            commands::contacts::load_dropped_contact_picture,
            commands::contacts::set_contact_picture,
            commands::contacts::remove_contact_picture,
            commands::contacts::get_contact_picture,
            commands::contacts::list_contact_pictures,
            commands::contacts::list_contact_messages,
            commands::contacts::mark_contact_spam,
            commands::contacts::move_contact_mail_to_junk,
            commands::contacts::undo_contact_spam,
            // Stage 10: signatures and templates (decision 0023).
            commands::snippets::list_signatures,
            commands::snippets::save_signature,
            commands::snippets::delete_signature,
            commands::snippets::get_identity_signature_defaults,
            commands::snippets::set_identity_signature_defaults,
            commands::snippets::pick_signature_images::<tauri::Wry>,
            commands::snippets::add_dropped_signature_images,
            commands::snippets::remove_signature_image,
            commands::snippets::signature_image_data,
            commands::snippets::list_templates,
            commands::snippets::save_template,
            commands::snippets::delete_template,
            commands::snippets::fill_template,
            // Stage 9: inbox comfort (decision 0020).
            commands::comfort::list_inbox,
            commands::comfort::list_inbox_filtered,
            commands::comfort::get_inbox_settings,
            commands::comfort::set_inbox_settings,
            commands::comfort::get_message_category,
            commands::comfort::set_sender_category,
            commands::comfort::set_sender_important,
            commands::comfort::list_sender_rules,
            commands::comfort::delete_sender_rule,
            commands::comfort::get_snooze_times,
            commands::comfort::snooze_messages,
            commands::comfort::unsnooze_messages,
            commands::comfort::set_pinned,
            commands::comfort::set_task,
            commands::comfort::list_snoozed,
            commands::comfort::list_pinned,
            commands::comfort::list_flagged,
            commands::comfort::list_tasks,
            // Stage 16 (decision 0033).
            commands::parity::set_reading_list,
            commands::parity::list_reading_list,
            commands::parity::list_unread_first,
            commands::parity::get_unread_first_views,
            commands::parity::set_unread_first_view,
            commands::parity::set_mailbox_favorite,
            commands::parity::set_mailbox_color,
            commands::parity::reorder_favorite_mailboxes,
            commands::parity::list_favorite_mailboxes,
            commands::parity::block_sender,
            commands::parity::unblock_sender,
            commands::parity::list_blocked_senders,
            commands::parity::transfer_messages,
            commands::parity::set_attachment_pinned,
            commands::parity::get_attachment_pins,
            commands::parity::list_pinned_attachments,
            commands::parity::count_blocked_hidden,
            // Stage 19, task A: sorted and filtered lists, orders, table (decisions 0061, 0062).
            commands::views::list_view_messages,
            commands::views::list_view_conversations,
            commands::views::count_list_view,
            commands::views::list_view_tags,
            commands::views::get_list_orders,
            commands::views::set_list_order,
            commands::views::get_list_table,
            commands::views::set_list_table,
            commands::comfort::list_rules,
            commands::comfort::create_rule,
            commands::comfort::update_rule,
            commands::comfort::delete_rule,
            commands::comfort::reorder_rules,
            commands::comfort::get_notification_settings,
            commands::comfort::set_notification_settings::<tauri::Wry>,
            commands::comfort::set_notification_texts,
            commands::comfort::get_notification_sound_choices,
            commands::comfort::get_message_extras,
            commands::comfort::unsubscribe,
            commands::comfort::open_invite,
            commands::comfort::export_message::<tauri::Wry>,
            commands::comfort::print_window::<tauri::Wry>,
            commands::comfort::get_default_mail_app::<tauri::Wry>,
            commands::comfort::set_default_mail_app::<tauri::Wry>,
            commands::comfort::take_launch_links,
            // Stage 9: .eml files, read-only in a window of their own (decision 0027).
            commands::eml::open_eml_file::<tauri::Wry>,
            commands::eml::get_eml_message,
            commands::eml::load_eml_remote_content,
            commands::eml::save_eml_attachment::<tauri::Wry>,
            commands::eml::open_eml_attachment,
            commands::eml::preview_eml_attachment,
            commands::eml::save_all_eml_attachments::<tauri::Wry>,
            // Stage 12: AI access over MCP (decisions 0015, 0021).
            commands::mcp::get_mcp_settings,
            commands::mcp::set_mcp_enabled,
            commands::mcp::set_mcp_account_access,
            commands::mcp::create_mcp_client,
            commands::mcp::revoke_mcp_client,
            commands::mcp::list_mcp_clients,
            commands::mcp::get_mcp_setup,
            commands::mcp::list_mcp_access,
            commands::mcp::get_mcp_status,
            commands::mcp::test_mcp_connection,
            commands::ai::get_ai_settings,
            commands::ai::get_ai_availability,
            commands::ai::set_ai_enabled,
            commands::ai::save_ai_profile,
            commands::ai::delete_ai_profile,
            commands::ai::set_ai_function,
            commands::ai::set_ai_account_cloud,
            commands::ai::set_ai_consent,
            commands::ai::set_open_reply_settings,
            commands::ai::list_ai_models,
            commands::ai::test_ai_connection,
            commands::ai::list_ai_requests,
            commands::ai::cancel_ai_request,
            commands::ai::ai_summarize_thread,
            commands::ai::ai_suggest_reply,
            commands::ai::ai_open_reply_draft,
            commands::ai::ai_writing_help,
            commands::ai::ai_ask,
            commands::ai::ai_prepare_transfer,
            commands::ai::list_open_reply_hints,
            commands::ai::dismiss_open_reply_hint,
            // Stage 11: settings sync (decisions 0014, 0022).
            commands::settings_sync::get_settings_sync_status,
            commands::settings_sync::enable_settings_sync,
            commands::settings_sync::preview_settings_sync_join,
            commands::settings_sync::join_settings_sync,
            commands::settings_sync::sync_settings_now,
            commands::settings_sync::resolve_sync_question,
            // Security review F1 (decision 0040).
            commands::settings_sync::resolve_server_change,
            commands::settings_sync::change_settings_sync_passphrase,
            commands::settings_sync::reset_settings_sync,
            commands::settings_sync::disable_settings_sync,
            commands::settings_sync::set_sync_device_name,
            // Stage 15 (decision 0037): in-app updates, opt-in.
            commands::updates::get_update_info,
            commands::updates::set_update_auto_check,
            commands::updates::check_for_updates,
            commands::updates::download_update,
            commands::updates::install_update,
            // Stage 15 (decision 0044): the guided first start.
            commands::first_start::get_first_start,
            commands::first_start::finish_first_start,
            // Stage 20 (decisions 0058 to 0060): attachment reminder, reminders without a
            // reply, the attachments view, the mailbox quota.
            commands::reminders::check_attachment_reminder,
            commands::reminders::get_follow_up_times,
            commands::reminders::set_follow_up,
            commands::reminders::clear_follow_up,
            commands::reminders::list_follow_ups,
            commands::attachments_view::list_attachments,
            commands::quota::get_account_quota,
            // Stage 18 (decision 0051): the new-mail popup, the only commands its window may
            // call (`popup::POPUP_COMMANDS`).
            commands::popup::get_new_mail_popup,
            commands::popup::read_from_popup::<tauri::Wry>,
            commands::popup::delete_from_popup,
            commands::popup::undo_popup_delete,
            commands::popup::dismiss_popup,
            commands::popup::open_inbox_from_popup::<tauri::Wry>,
            commands::popup::fit_new_mail_popup::<tauri::Wry>,
            // Stage 21 (decisions 0069, 0070): running in the background, the pause, the login
            // item, the tray menu's texts, the main window's lifecycle.
            commands::desktop::get_background_settings,
            commands::desktop::set_background_settings::<tauri::Wry>,
            commands::desktop::get_autostart::<tauri::Wry>,
            commands::desktop::set_autostart::<tauri::Wry>,
            commands::desktop::get_notification_pause,
            commands::desktop::pause_notifications,
            commands::desktop::set_desktop_texts::<tauri::Wry>,
            commands::desktop::main_window_ready::<tauri::Wry>,
            commands::desktop::close_main_window::<tauri::Wry>,
        ])
        .events(collect_events![
            CoreEventMessage,
            popup::NewMailPopupChanged,
            lifecycle::CloseToBackgroundAsked
        ])
        .error_handling(ErrorHandlingMode::Result)
        // IPC types are one-way (arguments in, results and events out): one alias per type.
        .disable_serde_phases()
        .constant("CORE_EVENT_NAME", CORE_EVENT_NAME)
        // Stage 4: the delay of `markReadOnOpen: "afterDelay"`, and the most messages one
        // `applyAction` may cover (larger selections are split by the UI).
        .constant("MARK_READ_DELAY_MS", mail_model::MARK_READ_DELAY_MS)
        .constant("MAX_ACTION_MESSAGES", mail_model::MAX_ACTION_MESSAGES)
        // Stage 19, task A: limits of the list views (decisions 0061, 0062).
        .constant("MAX_LIST_ORDERS", mail_model::MAX_LIST_ORDERS)
        .constant("MAX_QUICK_FILTER_TEXT", mail_model::MAX_QUICK_FILTER_TEXT)
        .constant("MIN_TABLE_COLUMN_WIDTH", mail_model::MIN_TABLE_COLUMN_WIDTH)
        .constant("MAX_TABLE_COLUMN_WIDTH", mail_model::MAX_TABLE_COLUMN_WIDTH)
        // Stage 16 (decision 0036): attachment previews and pins.
        .constant(
            "MAX_PREVIEW_INPUT_BYTES",
            mail_model::MAX_PREVIEW_INPUT_BYTES,
        )
        .constant("MAX_PREVIEW_TEXT_BYTES", mail_model::MAX_PREVIEW_TEXT_BYTES)
        .constant("MAX_PREVIEW_PDF_PAGES", mail_model::MAX_PREVIEW_PDF_PAGES)
        .constant("MAX_PINNED_ATTACHMENTS", mail_model::MAX_PINNED_ATTACHMENTS)
        // Stage 20 (decision 0060): the attachments view's page.
        .constant("MAX_ATTACHMENT_PAGE", mail_model::MAX_ATTACHMENT_PAGE)
        // Stage 20 (decision 0060): from this share on, the account shows a notice.
        .constant("QUOTA_WARN_PERCENT", mail_model::QUOTA_WARN_PERCENT)
        // Stage 7: the largest search, thread and unified page, and the most messages of a
        // thread view.
        .constant("MAX_SEARCH_PAGE", mail_model::MAX_SEARCH_PAGE)
        .constant("MAX_THREAD_MESSAGES", mail_model::MAX_THREAD_MESSAGES)
        // Stage 7 (decision 0026): saved and recent searches.
        .constant(
            "MAX_SAVED_SEARCHES",
            u32::try_from(mail_model::MAX_SAVED_SEARCHES).unwrap_or(u32::MAX),
        )
        .constant(
            "MAX_SAVED_SEARCH_NAME_CHARS",
            u32::try_from(mail_model::MAX_SAVED_SEARCH_NAME_CHARS).unwrap_or(u32::MAX),
        )
        .constant(
            "MAX_RECENT_SEARCHES",
            u32::try_from(mail_model::MAX_RECENT_SEARCHES).unwrap_or(u32::MAX),
        )
        // Stage 5: the undo delay's range and default, and the attachment limits (bytes).
        .constant("MAX_SEND_DELAY_SECS", mail_model::MAX_SEND_DELAY_SECS)
        .constant(
            "DEFAULT_SEND_DELAY_SECS",
            mail_model::DEFAULT_SEND_DELAY_SECS,
        )
        .constant(
            "MAX_ATTACHMENT_BYTES",
            u32::try_from(mail_model::MAX_ATTACHMENT_BYTES).unwrap_or(u32::MAX),
        )
        .constant(
            "LARGE_ATTACHMENT_WARNING_BYTES",
            u32::try_from(mail_model::LARGE_ATTACHMENT_WARNING_BYTES).unwrap_or(u32::MAX),
        )
        .constant("MAX_ATTACHMENTS", mail_model::MAX_ATTACHMENTS)
        .constant(
            "MAX_CONTACT_SUGGESTIONS",
            mail_model::MAX_CONTACT_SUGGESTIONS,
        )
        // Stage 12: the access log's page size and retention, the client name length.
        .constant("MAX_MCP_ACCESS_PAGE", mail_model::MAX_MCP_ACCESS_PAGE)
        .constant(
            "MCP_ACCESS_RETENTION_DAYS",
            mail_model::MCP_ACCESS_RETENTION_DAYS,
        )
        .constant("MCP_ACCESS_MAX_ENTRIES", mail_model::MCP_ACCESS_MAX_ENTRIES)
        .constant(
            "MAX_MCP_CLIENT_NAME_CHARS",
            mail_model::MAX_MCP_CLIENT_NAME_CHARS,
        )
        .constant("MAX_MCP_CLIENTS", mail_model::MAX_MCP_CLIENTS)
        // Stage 11: the shortest passphrase the core accepts, and the sync folder's name.
        .constant(
            "MIN_SYNC_PASSPHRASE_CHARS",
            mail_model::MIN_SYNC_PASSPHRASE_CHARS,
        )
        .constant("SETTINGS_SYNC_FOLDER", mail_model::SETTINGS_SYNC_FOLDER)
        // Stage 10 limits the composer checks before the core does (decision 0023).
        .constant(
            "MISSED_SEND_GRACE_MS",
            SafeInteger(mail_model::MISSED_SEND_GRACE_MS),
        )
        .constant(
            "MAX_SCHEDULE_AHEAD_MS",
            SafeInteger(mail_model::MAX_SCHEDULE_AHEAD_MS),
        )
        .constant(
            "MAX_INLINE_IMAGE_BYTES",
            u32::try_from(mail_model::MAX_INLINE_IMAGE_BYTES).unwrap_or(u32::MAX),
        )
        .constant(
            "MAX_SIGNATURE_IMAGES",
            u32::try_from(mail_model::MAX_SIGNATURE_IMAGES).unwrap_or(u32::MAX),
        )
        .constant(
            "MAX_SIGNATURE_IMAGE_BYTES",
            u32::try_from(mail_model::MAX_SIGNATURE_IMAGE_BYTES).unwrap_or(u32::MAX),
        )
        .constant(
            "MAX_SIGNATURE_IMAGES_BYTES",
            u32::try_from(mail_model::MAX_SIGNATURE_IMAGES_BYTES).unwrap_or(u32::MAX),
        )
        // Stage 8: the limits of the UI preferences (decision 0025).
        .constant("MIN_TEXT_SIZE", mail_model::MIN_TEXT_SIZE)
        .constant("MAX_TEXT_SIZE", mail_model::MAX_TEXT_SIZE)
        .constant("DEFAULT_TEXT_SIZE", mail_model::DEFAULT_TEXT_SIZE)
        // The accounts-and-folders column's width (window layout).
        .constant("MIN_SIDEBAR_WIDTH", mail_model::MIN_SIDEBAR_WIDTH)
        .constant("MAX_SIDEBAR_WIDTH", mail_model::MAX_SIDEBAR_WIDTH)
        .constant("DEFAULT_SIDEBAR_WIDTH", mail_model::DEFAULT_SIDEBAR_WIDTH)
        // Stage 18: the list's width beside the reading pane and its height above it.
        .constant("MIN_LIST_WIDTH", mail_model::MIN_LIST_WIDTH)
        .constant("MAX_LIST_WIDTH", mail_model::MAX_LIST_WIDTH)
        .constant("DEFAULT_LIST_WIDTH", mail_model::DEFAULT_LIST_WIDTH)
        .constant("MIN_LIST_HEIGHT", mail_model::MIN_LIST_HEIGHT)
        .constant("MAX_LIST_HEIGHT", mail_model::MAX_LIST_HEIGHT)
        .constant("DEFAULT_LIST_HEIGHT", mail_model::DEFAULT_LIST_HEIGHT)
        .constant(
            "MAX_RECENT_COMMANDS",
            u32::try_from(mail_model::MAX_RECENT_COMMANDS).unwrap_or(u32::MAX),
        )
        .constant(
            "MAX_SHORTCUT_KEY_CHARS",
            u32::try_from(mail_model::MAX_SHORTCUT_KEY_CHARS).unwrap_or(u32::MAX),
        )
        .constant(
            "MAX_SHORTCUTS_PER_COMMAND",
            u32::try_from(mail_model::MAX_SHORTCUTS_PER_COMMAND).unwrap_or(u32::MAX),
        )
        .constant(
            "MAX_SHORTCUT_OVERRIDES",
            u32::try_from(mail_model::MAX_SHORTCUT_OVERRIDES).unwrap_or(u32::MAX),
        )
        .constant(
            "MAX_COMMAND_ID_CHARS",
            u32::try_from(mail_model::MAX_COMMAND_ID_CHARS).unwrap_or(u32::MAX),
        )
        .constant(
            "MAX_RECENT_COMMAND_CHARS",
            u32::try_from(mail_model::MAX_RECENT_COMMAND_CHARS).unwrap_or(u32::MAX),
        )
        // Stage 17: the most entries of the column's views.
        .constant(
            "MAX_COLUMN_VIEWS",
            u32::try_from(mail_model::MAX_COLUMN_VIEWS).unwrap_or(u32::MAX),
        )
        // Stage 9 and 13 (decision 0027): the largest `.eml` file the app opens, and the most
        // ids one `getMessagesByIds` takes.
        .constant("MAX_EML_FILE_BYTES", mail_model::MAX_EML_FILE_BYTES)
        .constant("MAX_MESSAGES_BY_IDS", mail_model::MAX_MESSAGES_BY_IDS)
        // Stage 19 (decision 0055): the most bytes the source window shows, the most recipients
        // of a redirect.
        .constant("MAX_SOURCE_BYTES", mail_model::MAX_SOURCE_BYTES)
        // Stage 19 (decision 0056): contact groups and when the composer suggests Bcc.
        .constant("MAX_CONTACT_LISTS", mail_model::MAX_CONTACT_LISTS)
        .constant(
            "MAX_CONTACT_LIST_MEMBERS",
            mail_model::MAX_CONTACT_LIST_MEMBERS,
        )
        .constant(
            "MAX_CONTACT_LIST_NAME_CHARS",
            mail_model::MAX_CONTACT_LIST_NAME_CHARS,
        )
        .constant("MANY_RECIPIENTS", mail_model::MANY_RECIPIENTS)
        .constant(
            "MAX_REDIRECT_RECIPIENTS",
            mail_model::MAX_REDIRECT_RECIPIENTS,
        )
}

/// An `i64` constant below 2^53 (milliseconds), exported as a plain TypeScript number.
#[derive(serde::Serialize, specta::Type)]
#[serde(transparent)]
struct SafeInteger(#[specta(type = specta_typescript::Number)] i64);

/// TypeScript exporter settings, shared by the freshness test and `UPDATE_BINDINGS=1`.
pub fn typescript() -> Typescript {
    Typescript::default().header(
        "// Generated by tauri-specta from app/src-tauri. Do not edit.\n\
         // Regenerate with `UPDATE_BINDINGS=1 cargo test -p mail-app bindings`.\n",
    )
}

/// The app context from `tauri.conf.json`. Expanded once per crate: on macOS the macro embeds
/// `Info.plist` under a fixed symbol name.
fn context<R: Runtime>() -> tauri::Context<R> {
    tauri::generate_context!()
}

/// Holds the log writer until the app exits; dropping it flushes the last lines.
struct LogGuardState(Mutex<Option<LogGuard>>);

pub fn run() {
    let specta = specta_builder::<tauri::Wry>();
    // Stage 18 (decision 0051): the new-mail popup's window may call its own commands only.
    let handler = popup::restrict(specta.invoke_handler());
    #[cfg(not(feature = "e2e"))]
    let builder = tauri::Builder::default().invoke_handler(handler);
    // Desktop-test build: embedded WebDriver and the `e2e_*` test commands (decision 0005).
    #[cfg(feature = "e2e")]
    let builder = e2e::install(tauri::Builder::default(), handler);
    let app = with_system_plugins(builder)
        .setup(move |app| {
            init_logging(app.handle());
            let data_dir = data_dir(app.handle())?;
            let package = app.package_info();
            tracing::info!(version = %package.version, platform = std::env::consts::OS, "starting");
            let services = services(app.handle())?;
            let core = tauri::async_runtime::block_on(AppCore::start(
                CoreConfig {
                    data_dir,
                    build: BuildInfo {
                        version: package.version.to_string(),
                        commit: option_env!("MAIL_APP_COMMIT").map(str::to_owned),
                        build_date: option_env!("MAIL_APP_BUILD_DATE").map(str::to_owned),
                    },
                },
                services,
            ));
            let theme = initial_theme(&core);
            // Stage 21 (decisions 0069, 0070): the main window may come and go; the login item.
            lifecycle::install(app.handle(), &core);
            app.manage(AutostartState(autostart(app.handle())));
            if let Some(state) = app.try_state::<AutostartState>() {
                state.0.repair();
            }
            // Stage 9: native notifications, the Dock/taskbar counter, mailto: links.
            app.manage(notifications::Counter::default());
            core.set_notification_sink(notifications::NativeNotifications::install(app.handle()));
            notifications::follow_badge(app.handle().clone(), core.subscribe());
            app.manage(links::LinkQueue::default());
            // Stage 15 (decision 0037): the update source; it only checks with consent.
            start_updates(app.handle(), &core);
            attach_core(app.handle(), &specta, core);
            // Stage 21: started by the login item, the app runs without a window until asked.
            if lifecycle::started_in_background(std::env::args().skip(1)) {
                tracing::info!("started in the background");
            } else {
                open_main_window(app.handle(), theme, true)?;
            }
            // `mailto:` links of the start wait for the page (after the window exists, so a link
            // never builds a second one).
            for url in default_app::mailto_arguments(std::env::args().skip(1)) {
                links::opened(app.handle(), &url);
            }
            tray::refresh(app.handle());
            // Stage 9: `.eml` files of the start (Windows passes them as arguments).
            let cwd = std::env::current_dir().ok();
            eml_files::open_files(
                app.handle(),
                eml_files::eml_arguments(std::env::args().skip(1), cwd.as_deref()),
            );
            Ok(())
        })
        .build(context());

    let app = match app {
        Ok(app) => app,
        Err(err) => {
            tracing::error!(%err, "app failed to start");
            eprintln!("app failed to start: {err}");
            std::process::exit(1);
        }
    };
    app.run(|handle, event| {
        // Stage 9: macOS hands `mailto:` links (the app as default mail app) and `.eml` files
        // (decision 0027) to the app.
        #[cfg(target_os = "macos")]
        if let RunEvent::Opened { urls } = &event {
            for url in urls.iter().filter(|url| url.scheme() != "file") {
                links::opened(handle, url.as_str());
            }
            eml_files::open_files(handle, eml_files::eml_urls(urls));
        }
        // Stage 21: a click on the Dock icon, or the app started again from the Finder, opens
        // the main window again if it was closed into the background.
        #[cfg(target_os = "macos")]
        if let RunEvent::Reopen { .. } = &event
            && !handle
                .get_webview_window(MAIN_WINDOW)
                .is_some_and(|w| w.is_visible().unwrap_or(false))
        {
            lifecycle::show_main_window(handle);
        }
        // Stage 21: the last window closed; with "keep running" the app stays (decision 0069).
        if let RunEvent::ExitRequested { code, api, .. } = &event
            && handle
                .try_state::<lifecycle::Lifecycle>()
                .is_some_and(|l| l.keeps_running(*code))
        {
            api.prevent_exit();
        }
        if let RunEvent::Exit = event {
            before_exit(handle);
        }
    });
}

/// Stage 21 (decision 0070): the login item of this app (managed state).
struct AutostartState(autostart::Autostart);

/// The login item: the user's launch agent (macOS) or Run key (Windows); a launch agent in the
/// test's own directory in desktop-test builds. Development builds do not register themselves.
fn autostart<R: Runtime>(app: &AppHandle<R>) -> autostart::Autostart {
    let exe = std::env::current_exe().unwrap_or_default();
    let identifier = app.config().identifier.clone();
    #[cfg(feature = "e2e")]
    {
        let path = e2e::login_items_dir()
            .unwrap_or_default()
            .join(format!("{identifier}.plist"));
        autostart::Autostart::new(
            Box::new(autostart::LaunchAgent {
                path,
                label: identifier,
            }),
            exe,
            None,
        )
    }
    #[cfg(not(feature = "e2e"))]
    {
        let development = cfg!(debug_assertions).then_some("development build");
        #[cfg(target_os = "macos")]
        {
            let path = app
                .path()
                .home_dir()
                .unwrap_or_default()
                .join("Library/LaunchAgents")
                .join(format!("{identifier}.plist"));
            let unavailable = development.or_else(|| {
                autostart::translocated(&exe)
                    .then_some("the app runs from a translocated copy; move it to Applications")
            });
            autostart::Autostart::new(
                Box::new(autostart::LaunchAgent {
                    path,
                    label: identifier,
                }),
                exe,
                unavailable,
            )
        }
        #[cfg(windows)]
        {
            let _ = identifier;
            let name = app
                .config()
                .product_name
                .clone()
                .unwrap_or_else(|| app.package_info().name.clone());
            autostart::Autostart::new(Box::new(autostart::RunKey { name }), exe, development)
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            let _ = (identifier, development);
            autostart::Autostart::new(
                Box::new(autostart::LaunchAgent {
                    path: std::path::PathBuf::new(),
                    label: String::new(),
                }),
                exe,
                Some("not supported on this system"),
            )
        }
    }
}

/// Whether the app starts with the system (as the system has it).
pub(crate) fn autostart_status<R: Runtime>(app: &AppHandle<R>) -> mail_model::Autostart {
    app.try_state::<AutostartState>().map_or(
        mail_model::Autostart {
            enabled: false,
            available: false,
        },
        |s| s.0.status(),
    )
}

/// Switches the login item on or off.
pub(crate) fn set_autostart<R: Runtime>(
    app: &AppHandle<R>,
    enabled: bool,
) -> Result<mail_model::Autostart, mail_model::CoreError> {
    let Some(state) = app.try_state::<AutostartState>() else {
        return Err(mail_model::CoreError::Internal(
            "no login item service".into(),
        ));
    };
    state.0.set(enabled).map_err(|err| match err.kind() {
        std::io::ErrorKind::Unsupported => mail_model::CoreError::InvalidArgument(err.to_string()),
        _ => mail_model::CoreError::Internal(format!("login item: {err}")),
    })
}

/// Ends the sync tasks (connections get a moment to log out) and flushes the log. On every
/// quit, and on Windows before an update's installer takes over (decision 0037).
pub(crate) fn before_exit<R: Runtime>(handle: &AppHandle<R>) {
    tracing::info!("exiting");
    if let Some(core) = handle.try_state::<AppCore>() {
        tauri::async_runtime::block_on(core.shutdown());
    }
    if let Some(state) = handle.try_state::<LogGuardState>() {
        drop(
            state
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take(),
        );
    }
}

/// Stage 15 (decision 0037): hands the core the update source on top of the updater plugin,
/// with the endpoint from `tauri.conf.json` (desktop-test builds may point to a local server).
/// Without a usable endpoint the app runs without updates and says so in About.
fn start_updates<R: Runtime>(app: &AppHandle<R>, core: &AppCore) {
    #[cfg(feature = "e2e")]
    let endpoint = e2e::update_endpoint().or_else(|| updater::configured_endpoint(app));
    #[cfg(not(feature = "e2e"))]
    let endpoint = updater::configured_endpoint(app);
    let Some(endpoint) = endpoint else {
        tracing::warn!("no update endpoint configured; updates are off in this build");
        return;
    };
    match updater::PluginSource::new(app.clone(), endpoint, updater::installed_app_only) {
        Ok(source) => tauri::async_runtime::block_on(core.start_updates(
            std::sync::Arc::new(source),
            mail_engine::UpdateSchedule::default(),
        )),
        Err(err) => tracing::error!(%err, "update source unavailable; updates are off"),
    }
}

/// Stage 9: the notification plugin (used from Rust only; the UI has no notification
/// permission) and, on Windows, the single-instance plugin: a second start (a `mailto:` link
/// while the app runs) hands its arguments to this process and ends.
fn with_system_plugins<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    #[cfg(windows)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
        // Stage 21: the login item while the app runs already changes nothing.
        if lifecycle::started_in_background(argv.iter().skip(1).cloned()) {
            return;
        }
        let urls = default_app::mailto_arguments(argv.iter().skip(1).cloned());
        // Decision 0027: an `.eml` file double-clicked while the app runs.
        let files =
            eml_files::eml_arguments(argv.into_iter().skip(1), Some(std::path::Path::new(&cwd)));
        // Stage 21: the app started again shows its window, also after it was closed into the
        // background.
        if urls.is_empty() && files.is_empty() {
            lifecycle::show_main_window(app);
        }
        for url in urls {
            links::opened(app, &url);
        }
        eml_files::open_files(app, files);
    }));
    builder
        .plugin(tauri_plugin_notification::init())
        // Decision 0037: used from Rust only; no capability grants `updater:*`.
        .plugin(updater::plugin())
}

/// File logging in the app's log directory; console too in debug builds. A logging failure is
/// reported on stderr but does not stop the app.
fn init_logging<R: Runtime>(app: &AppHandle<R>) {
    let dir = match log_dir(app) {
        Ok(dir) => dir,
        Err(err) => {
            eprintln!("no log directory, file logging disabled: {err}");
            return;
        }
    };
    let config = LogConfig {
        dir,
        console: cfg!(debug_assertions),
        verbose: cfg!(debug_assertions),
    };
    match logging::init(&config) {
        Ok(guard) => {
            app.manage(LogGuardState(Mutex::new(Some(guard))));
        }
        Err(err) => eprintln!("file logging disabled: {err}"),
    }
}

/// What the core gets: the system keychain under the app's identifier (the bundle id), the IMAP
/// adapter, the system clock, the product sync settings, and the cache directory for copies of
/// opened attachments.
fn services<R: Runtime>(app: &AppHandle<R>) -> Result<CoreServices, Box<dyn std::error::Error>> {
    let keychain = SystemKeychain::new(&app.config().identifier)?;
    let mut services = CoreServices::new(Credentials::new(keychain));
    services.reading.open_dir = Some(cache_dir(app)?.join("attachments"));
    // Finding C-2 (decision 0043): image and PDF previews render in the `mail-mcp` helper; C-4
    // (decision 0046): it also re-encodes every image of a mail before the web view shows it.
    services.reading.preview_helper = preview_helper();
    // Desktop-test build: may trust the local test server's CA (decision 0007).
    #[cfg(feature = "e2e")]
    let mut services = e2e::with_test_ca(services)?;
    // Stage 14 (decision 0042): sign-in with Microsoft and Google opens the provider's page with
    // the same opener as links (the system browser; the desktop test's recorder).
    let oauth = mail_engine::OAuthService::with(
        oauth::config(),
        std::sync::Arc::clone(&services.reading.opener),
        std::sync::Arc::new(mail_engine::SystemClock),
    );
    services.credentials = services.credentials.with_oauth(std::sync::Arc::new(oauth));
    Ok(services)
}

/// The `mail-mcp` helper next to the executable, which renders image and PDF previews in a child
/// process (decision 0043) and re-encodes the images of mails (decision 0046). Shipped in both
/// installers; a release build without it refuses those previews and shows no image of a mail. A
/// development build without it (`cargo build -p mail-mcp` adds it) decodes them in the app.
fn preview_helper() -> Option<std::path::PathBuf> {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(err) => {
            tracing::warn!(error = %err, "own path unknown; previews render in the app");
            return None;
        }
    };
    let helper = exe.with_file_name(if cfg!(windows) {
        "mail-mcp.exe"
    } else {
        "mail-mcp"
    });
    (!cfg!(debug_assertions) || helper.is_file()).then_some(helper)
}

/// The app's data directory (database, backups, message bodies): the local, non-roaming one.
/// Windows: `%LOCALAPPDATA%\<identifier>`, next to the logs and WebView2's data; the roaming
/// `%APPDATA%` would copy a mail cache of gigabytes with every roaming-profile logon. macOS:
/// `~/Library/Application Support/<identifier>`, the same as the roaming variant there.
#[cfg(not(feature = "e2e"))]
fn data_dir<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<std::path::PathBuf> {
    app.path().app_local_data_dir()
}

/// The app's log directory.
#[cfg(not(feature = "e2e"))]
fn log_dir<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<std::path::PathBuf> {
    app.path().app_log_dir()
}

/// The app's cache directory (copies of opened attachments, emptied at every start).
#[cfg(not(feature = "e2e"))]
fn cache_dir<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<std::path::PathBuf> {
    app.path().app_cache_dir()
}

// Desktop-test builds use a directory of the test's own and never the user's (decision 0005).
#[cfg(feature = "e2e")]
use e2e::{cache_dir, data_dir, log_dir};

/// Makes the core available to commands and forwards its events to the UI. Must run before the
/// first window exists, so no command can arrive before the core is managed.
fn attach_core<R: Runtime>(app: &AppHandle<R>, specta: &Builder<R>, core: AppCore) {
    specta.mount_events(app);
    events::forward(app.clone(), core.subscribe());
    app.manage(core);
    // Stage 5: paths of files dropped on the window (see `drops`).
    app.manage(drops::DropRegistry::default());
}

/// The window frame's theme at start, from the stored preferences (see [`window_theme`]).
fn initial_theme(core: &AppCore) -> Option<Theme> {
    match tauri::async_runtime::block_on(core.get_preferences()) {
        Ok(preferences) => window_theme::window_theme(preferences.theme),
        Err(err) => {
            tracing::warn!(%err, "preferences unreadable, the window frame follows the system");
            None
        }
    }
}

/// The window is declared in `tauri.conf.json` with `"create": false` and built here, after the
/// core is managed: at start, and again whenever it was closed into the background (stage 21,
/// [`lifecycle::show_main_window`], which shows it once it has its old place: `visible` false).
pub(crate) fn open_main_window<R: Runtime>(
    app: &AppHandle<R>,
    theme: Option<Theme>,
    visible: bool,
) -> tauri::Result<WebviewWindow<R>> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == MAIN_WINDOW)
        .cloned()
        .ok_or_else(|| tauri::Error::WindowNotFound)?;
    // The only place that learns where a click in the mail frame goes (decision 0005), the
    // same for every window of the app (`windows::guard`, decision 0025).
    let builder = tauri::WebviewWindowBuilder::from_config(app, &config)?
        .theme(theme)
        .visible(visible);
    let window = windows::guard(builder, app).build()?;
    // Stage 5: remember what the user drops on the window; on Windows the frame guard.
    windows::finish(app, &window)?;
    // Stage 21: closing it keeps the app running; requests wait for its page; links of a new
    // page are taken again; the counter is drawn on its taskbar button.
    lifecycle::follow_main_window(app, &window);
    if let Some(queue) = app.try_state::<links::LinkQueue>() {
        queue.reset();
    }
    if let Some(core) = app.try_state::<AppCore>() {
        window_theme::follow(window.clone(), core.subscribe());
    }
    notifications::redraw_badge(app);
    Ok(window)
}

/// A click on link `index` of a rendering: the core announces it to the UI (`linkActivated`),
/// which asks `describeLink` and, after the user confirmed, `openLink`.
pub(crate) fn report_link<R: Runtime>(
    app: &AppHandle<R>,
    token: mail_model::RenderToken,
    index: u32,
) {
    match app.try_state::<AppCore>() {
        Some(core) => {
            core.link_activated(token, index);
        }
        None => tracing::warn!(index, "link clicked before the core was ready"),
    }
}

/// Windows: checks every navigation inside the mail frame through WebView2's
/// `FrameNavigationStarting` (decision 0011). The COM calls live in the crate `webview2-guard`;
/// this crate only hands over the controller, which Tauri gives out safely.
#[cfg(all(windows, feature = "webview2-frame-guard"))]
pub(crate) fn install_frame_guard<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<()> {
    let app = window.app_handle().clone();
    let policy: std::sync::Arc<webview2_guard::FramePolicy> =
        std::sync::Arc::new(move |url: &str| {
            navigation::decide_frame_navigation(url, &|token, index| {
                report_link(&app, token, index);
            })
        });
    window.with_webview(move |webview| {
        match webview2_guard::guard_frame_navigations(&webview.controller(), policy) {
            Ok(()) => tracing::info!("WebView2 frame guard active"),
            Err(err) => tracing::error!(
                %err,
                "WebView2 frame guard could not be installed: links in the mail frame do nothing"
            ),
        }
    })
}

#[cfg(test)]
mod tests;
