use std::path::PathBuf;

use directories::BaseDirs;

/// Root data directory: `$UNITED_DATA_DIR`, else `~/.local/share/united`.
pub fn data_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("UNITED_DATA_DIR") {
        return PathBuf::from(custom);
    }
    BaseDirs::new()
        .map(|b| b.data_dir().join("united"))
        .unwrap_or_else(|| PathBuf::from(".united"))
}

/// Directory for one service's session material (WhatsApp device store, etc.).
pub fn service_dir(service: &str) -> PathBuf {
    let dir = data_dir().join("sessions").join(service);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn db_path() -> PathBuf {
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    dir.join("united.db")
}

pub fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

/// Per-service attachment cache ceiling. Attachments are re-downloadable, so
/// a year of clips should not pin gigabytes on disk.
pub const MEDIA_BUDGET: u64 = 500_000_000;

/// Keep a media cache under `budget` bytes by deleting the oldest files first.
/// Attachments are re-downloadable, so a missing file just triggers a fetch on
/// the next open. Called after each download; the directory is small.
pub fn prune_media(dir: &std::path::Path, budget: u64) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = Vec::new();
    let mut total = 0u64;
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        total += meta.len();
        let at = meta
            .modified()
            .or_else(|_| meta.accessed())
            .unwrap_or(std::time::UNIX_EPOCH);
        files.push((at, meta.len(), entry.path()));
    }
    if total <= budget {
        return;
    }
    files.sort_by_key(|(at, _, _)| *at);
    for (_, len, path) in files {
        if total <= budget {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
        }
    }
}
