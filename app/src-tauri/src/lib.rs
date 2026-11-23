//! Desktop shell: window, commands and wiring. All behaviour lives in `mail-engine`; this crate
//! only forwards commands to [`AppCore`] and core events to the UI.

pub mod commands;
mod default_app;
pub mod drops;
#[cfg(feature = "e2e")]
mod e2e;
mod events;
pub mod links;
mod navigation;
mod notifications;
mod window_theme;

use std::sync::Mutex;

use mail_engine::credentials::{Credentials, SystemKeychain};
use mail_engine::logging::{self, LogConfig, LogGuard};
use mail_engine::{AppCore, BuildInfo, CoreConfig, CoreServices};
use specta_typescript::Typescript;
use tauri::webview::NewWindowResponse;
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
            commands::preferences::get_preferences,
            commands::preferences::set_preferences,
            commands::preferences::get_reading_preferences,
            commands::preferences::set_reading_preferences,
            commands::accounts::list_accounts,
            commands::accounts::discover_known_provider,
            commands::accounts::discover_online,
            commands::accounts::test_connection,
            commands::accounts::add_account,
            commands::accounts::remove_account,
            commands::accounts::retry_account,
            commands::accounts::update_password,
            commands::mail::list_mailboxes,
            commands::mail::set_mailbox_role,
            commands::mail::list_messages,
            commands::reading::get_message,
            commands::reading::load_remote_content,
            commands::reading::list_remote_content_senders,
            commands::reading::revoke_remote_content,
            commands::reading::describe_link,
            commands::reading::open_link,
            // The runtime parameter only matters for the type export; Tauri's handler infers it.
            commands::reading::save_attachment::<tauri::Wry>,
            commands::reading::open_attachment,
            commands::reading::get_prefetch_settings,
            commands::reading::set_prefetch_settings,
            commands::reading::get_prefetch_status,
            commands::actions::apply_action,
            commands::actions::undo_action,
            commands::actions::create_mailbox,
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
            commands::threads::list_threads,
            commands::threads::list_unified_threads,
            commands::threads::get_thread,
            commands::threads::list_unified_inbox,
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
            commands::comfort::list_tasks,
            commands::comfort::list_rules,
            commands::comfort::create_rule,
            commands::comfort::update_rule,
            commands::comfort::delete_rule,
            commands::comfort::reorder_rules,
            commands::comfort::get_notification_settings,
            commands::comfort::set_notification_settings::<tauri::Wry>,
            commands::comfort::set_notification_texts,
            commands::comfort::get_message_extras,
            commands::comfort::unsubscribe,
            commands::comfort::open_invite,
            commands::comfort::export_message::<tauri::Wry>,
            commands::comfort::print_window::<tauri::Wry>,
            commands::comfort::get_default_mail_app::<tauri::Wry>,
            commands::comfort::set_default_mail_app::<tauri::Wry>,
            commands::comfort::take_launch_links,
        ])
        .events(collect_events![CoreEventMessage])
        .error_handling(ErrorHandlingMode::Result)
        // IPC types are one-way (arguments in, results and events out): one alias per type.
        .disable_serde_phases()
        .constant("CORE_EVENT_NAME", CORE_EVENT_NAME)
        // Stage 4: the delay of `markReadOnOpen: "afterDelay"`, and the most messages one
        // `applyAction` may cover (larger selections are split by the UI).
        .constant("MARK_READ_DELAY_MS", mail_model::MARK_READ_DELAY_MS)
        .constant("MAX_ACTION_MESSAGES", mail_model::MAX_ACTION_MESSAGES)
        // Stage 7: the largest search, thread and unified page, and the most messages of a
        // thread view.
        .constant("MAX_SEARCH_PAGE", mail_model::MAX_SEARCH_PAGE)
        .constant("MAX_THREAD_MESSAGES", mail_model::MAX_THREAD_MESSAGES)
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
}

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
    #[cfg(not(feature = "e2e"))]
    let builder = tauri::Builder::default().invoke_handler(specta.invoke_handler());
    // Desktop-test build: embedded WebDriver and the `e2e_*` test commands (decision 0005).
    #[cfg(feature = "e2e")]
    let builder = e2e::install(tauri::Builder::default(), specta.invoke_handler());
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
            let theme_events = core.subscribe();
            // Stage 9: native notifications, the Dock/taskbar counter, mailto: links.
            core.set_notification_sink(notifications::NativeNotifications::install(app.handle()));
            notifications::follow_badge(app.handle().clone(), core.subscribe());
            app.manage(links::LinkQueue::default());
            for url in default_app::mailto_arguments(std::env::args().skip(1)) {
                links::opened(app.handle(), &url);
            }
            attach_core(app.handle(), &specta, core);
            let window = open_main_window(app.handle(), theme)?;
            window_theme::follow(window, theme_events);
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
        // Stage 9: macOS hands `mailto:` links (the app as default mail app) to the app.
        #[cfg(target_os = "macos")]
        if let RunEvent::Opened { urls } = &event {
            for url in urls {
                links::opened(handle, url.as_str());
            }
        }
        if let RunEvent::Exit = event {
            tracing::info!("exiting");
            // Ends the sync tasks; connections get a moment to log out.
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
    });
}

/// Stage 9: the notification plugin (used from Rust only; the UI has no notification
/// permission) and, on Windows, the single-instance plugin: a second start (a `mailto:` link
/// while the app runs) hands its arguments to this process and ends.
fn with_system_plugins<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    #[cfg(windows)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
        let urls = default_app::mailto_arguments(argv.into_iter().skip(1));
        if urls.is_empty()
            && let Some(window) = app.get_webview_window(MAIN_WINDOW)
        {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
        for url in urls {
            links::opened(app, &url);
        }
    }));
    builder.plugin(tauri_plugin_notification::init())
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
    // Desktop-test build: may trust the local test server's CA (decision 0007).
    #[cfg(feature = "e2e")]
    let services = e2e::with_test_ca(services)?;
    Ok(services)
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
/// core is managed.
fn open_main_window<R: Runtime>(
    app: &AppHandle<R>,
    theme: Option<Theme>,
) -> tauri::Result<WebviewWindow<R>> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == MAIN_WINDOW)
        .cloned()
        .ok_or_else(|| tauri::Error::WindowNotFound)?;
    // The Vite server of `tauri dev` is the app as well; never in a built app.
    let dev_url = if cfg!(dev) {
        app.config().build.dev_url.clone()
    } else {
        None
    };
    let links = app.clone();
    let window = tauri::WebviewWindowBuilder::from_config(app, &config)?
        .theme(theme)
        // The only place that learns where a click in the mail frame goes (decision 0005): the
        // app may load itself, the mail frame its own document; a link placeholder is
        // cancelled and reported to the core; everything else is cancelled.
        .on_navigation(move |url| {
            navigation::decide_main_navigation(url, dev_url.as_ref(), &|token, index| {
                report_link(&links, token, index);
            })
        })
        // Nothing in the page opens windows; `window.open` and `target=_blank`, also from the
        // mail frame, are refused (WebView2 marks the request handled, decision 0011).
        .on_new_window(|url, _features| {
            tracing::warn!(
                scheme = url.scheme(),
                host = url.host_str(),
                "refused to open a new window"
            );
            NewWindowResponse::Deny
        })
        .build()?;
    // Stage 5: remember what the user drops on the window; the composer may attach exactly
    // these files (`addDroppedAttachments`).
    let drop_app = app.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event
            && let Some(registry) = drop_app.try_state::<drops::DropRegistry>()
        {
            registry.record(paths);
        }
    });
    #[cfg(all(windows, feature = "webview2-frame-guard"))]
    install_frame_guard(&window)?;
    Ok(window)
}

/// A click on link `index` of a rendering: the core announces it to the UI (`linkActivated`),
/// which asks `describeLink` and, after the user confirmed, `openLink`.
fn report_link<R: Runtime>(app: &AppHandle<R>, token: mail_model::RenderToken, index: u32) {
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
fn install_frame_guard<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<()> {
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
