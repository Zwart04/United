//! United core: service-neutral chat model, SQLite store, and event bus.
//!
//! Every protocol adapter (WhatsApp, Telegram, Discord, ...) translates its own
//! wire format into these types, so the unified inbox is built from real
//! messages instead of scraped page notifications.

pub mod adapter;
pub mod events;
pub mod model;
pub mod paths;
pub mod settings;
pub mod store;

pub use adapter::{Adapter, AuthInput};
pub use events::{CoreEvent, EventBus, ServiceStatus};
pub use model::{AccountId, CallEntry, Chat, Message, MessageStatus, Service};
pub use settings::AppSettings;
pub use store::Store;
