//! United — native multi-protocol messenger shell.
//!
//! One process owns three things: the SQLite store, the tokio runtime that runs
//! the protocol adapters, and the egui event loop that renders them. No webview.

use std::collections::HashMap;
use std::sync::Arc;

use united_core::events::EventBus;
use united_core::model::Service;
use united_core::{Adapter, AppSettings, Store};
use united_ui::app::UnitedApp;

/// One running instance per data directory: two processes would fight over the
/// same WhatsApp device store and Telegram session file.
#[cfg(unix)]
fn lock_data_dir() -> Option<std::fs::File> {
    use std::os::fd::AsRawFd;

    let dir = united_core::paths::data_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let file = std::fs::File::create(dir.join("united.lock")).ok()?;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    (rc == 0).then_some(file)
}

/// Windows has no `flock` in the std surface: a lock file opened with
/// `create_new` gives the same "one instance per data dir" guarantee, and the
/// guard deletes it on exit. ponytail: a hard crash leaves the lock behind;
/// swap in `LockFileEx` when that becomes annoying.
#[cfg(not(unix))]
fn lock_data_dir() -> Option<std::fs::File> {
    let dir = united_core::paths::data_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("united.lock");
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .ok()?;
    Some(file)
}

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let _lock = match lock_data_dir() {
        Some(lock) => lock,
        None => {
            eprintln!("United is already running (another instance holds the data directory).");
            std::process::exit(1);
        }
    };

    let store = Arc::new(Store::open(united_core::paths::db_path()).expect("open united.db"));
    let settings = AppSettings::load();
    let bus = EventBus::new();

    // Adapters publish from the runtime; the UI thread needs the same runtime
    // only to spawn one-shot calls, so enter it before creating the window.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()
        .expect("tokio runtime");
    let guard = runtime.enter();

    let mut adapters: HashMap<Service, Arc<dyn Adapter>> = HashMap::new();
    if settings.is_enabled(Service::WhatsApp.id()) {
        adapters.insert(
            Service::WhatsApp,
            Arc::new(united_whatsapp::WhatsAppAdapter::new()),
        );
    }
    if settings.is_enabled(Service::Telegram.id()) {
        adapters.insert(
            Service::Telegram,
            Arc::new(united_telegram::TelegramAdapter::new()),
        );
    }
    if settings.is_enabled(Service::Discord.id()) {
        adapters.insert(
            Service::Discord,
            Arc::new(united_discord::DiscordAdapter::new()),
        );
    }

    for adapter in adapters.values() {
        let adapter = adapter.clone();
        let bus = bus.clone();
        runtime.spawn(async move {
            if let Err(err) = adapter.start(bus).await {
                log::warn!("{} adapter failed: {err:#}", adapter.service().id());
            }
        });
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("United")
            // The window paints its own title bar (see `UnitedApp::chrome`).
            .with_decorations(false)
            .with_icon(
                // Same artwork as the desktop entry, so the taskbar and the
                // alt-tab switcher match what the launcher shows.
                eframe::icon_data::from_png_bytes(include_bytes!(
                    "../../../packaging/united-256.png"
                ))
                .unwrap_or_default(),
            )
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([800.0, 560.0])
            .with_app_id("com.davdigi.United"),
        ..Default::default()
    };

    let result = eframe::run_native(
        "United",
        options,
        Box::new(move |cc| {
            Ok(Box::new(UnitedApp::new(
                &cc.egui_ctx,
                store,
                bus,
                adapters,
                settings,
            )))
        }),
    );

    drop(guard);
    result
}
