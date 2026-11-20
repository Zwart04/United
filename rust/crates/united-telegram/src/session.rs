//! JSON-backed Session storage for grammers.
//!
//! grammers ships a SQLite session, but it pulls `libsql`, which statically
//! links its own SQLite C amalgamation and clashes at link time with the
//! `rusqlite` build used by `united-core` (duplicate `sqlite3_*` symbols). This
//! store keeps the same state in one small JSON file instead: one SQLite in the
//! whole binary, and the session is human-inspectable.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use grammers_session::types::{DcOption, PeerId, PeerInfo, PeerRef, UpdateState, UpdatesState};
use grammers_session::{BoxFuture, Session, SessionData};
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    #[serde(default)]
    home_dc: i32,
    #[serde(default)]
    dc_options: Vec<DcOption>,
    #[serde(default)]
    peers: Vec<(PeerId, PeerInfo)>,
    #[serde(default)]
    updates: OptionsState,
}

/// `UpdatesState` derives Default, but keeping it behind `#[serde(default)]`
/// keeps old session files loadable if the update cache is removed.
#[derive(Default, Serialize, Deserialize)]
#[serde(transparent)]
struct OptionsState(UpdatesState);

struct State {
    home_dc: i32,
    dc_options: HashMap<i32, DcOption>,
    peers: HashMap<PeerId, PeerInfo>,
    updates: UpdatesState,
}

pub struct FileSession {
    state: Mutex<State>,
    path: PathBuf,
}

impl FileSession {
    pub fn open(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref().to_path_buf();
        let defaults = SessionData::default();
        let mut state = State {
            home_dc: defaults.home_dc,
            dc_options: defaults.dc_options,
            peers: defaults.peer_infos,
            updates: defaults.updates_state,
        };
        if let Ok(raw) = std::fs::read_to_string(&path) {
            if let Ok(saved) = serde_json::from_str::<Saved>(&raw) {
                state.home_dc = saved.home_dc;
                state
                    .dc_options
                    .extend(saved.dc_options.into_iter().map(|d| (d.id, d)));
                for (id, peer) in saved.peers {
                    state.peers.insert(id, peer);
                }
                state.updates = saved.updates.0;
            }
        }
        Self {
            state: Mutex::new(state),
            path,
        }
    }

    fn save(&self, state: &State) {
        let saved = Saved {
            home_dc: state.home_dc,
            dc_options: state.dc_options.values().cloned().collect(),
            peers: state
                .peers
                .iter()
                .map(|(id, info)| (*id, info.clone()))
                .collect(),
            updates: OptionsState(state.updates.clone()),
        };
        let Ok(raw) = serde_json::to_vec(&saved) else {
            return;
        };
        let tmp = self.path.with_extension("json.tmp");
        if std::fs::write(&tmp, raw).is_ok() {
            let _ = std::fs::rename(&tmp, &self.path);
        }
    }
}

fn poisoned() -> io::Error {
    io::Error::other("telegram session lock poisoned")
}

impl Session for FileSession {
    type Error = io::Error;

    fn home_dc_id(&self) -> Result<i32, Self::Error> {
        Ok(self.state.lock().map_err(|_| poisoned())?.home_dc)
    }

    fn set_home_dc_id(&self, dc_id: i32) -> BoxFuture<'_, Result<(), Self::Error>> {
        Box::pin(async move {
            let mut state = self.state.lock().map_err(|_| poisoned())?;
            state.home_dc = dc_id;
            self.save(&state);
            Ok(())
        })
    }

    fn dc_option(&self, dc_id: i32) -> Result<Option<DcOption>, Self::Error> {
        Ok(self
            .state
            .lock()
            .map_err(|_| poisoned())?
            .dc_options
            .get(&dc_id)
            .cloned())
    }

    fn set_dc_option(&self, dc_option: &DcOption) -> BoxFuture<'_, Result<(), Self::Error>> {
        let dc_option = dc_option.clone();
        Box::pin(async move {
            let mut state = self.state.lock().map_err(|_| poisoned())?;
            state.dc_options.insert(dc_option.id, dc_option);
            self.save(&state);
            Ok(())
        })
    }

    fn peer(&self, peer: PeerId) -> BoxFuture<'_, Result<Option<PeerInfo>, Self::Error>> {
        Box::pin(async move {
            Ok(self
                .state
                .lock()
                .map_err(|_| poisoned())?
                .peers
                .get(&peer)
                .cloned())
        })
    }

    fn peer_ref(&self, peer: PeerId) -> BoxFuture<'_, Result<Option<PeerRef>, Self::Error>> {
        Box::pin(async move {
            Ok(self
                .state
                .lock()
                .map_err(|_| poisoned())?
                .peers
                .get(&peer)
                .and_then(|info| info.auth())
                .map(|auth| PeerRef { id: peer, auth }))
        })
    }

    fn cache_peer(&self, peer: PeerInfo) -> BoxFuture<'_, Result<(), Self::Error>> {
        Box::pin(async move {
            let mut state = self.state.lock().map_err(|_| poisoned())?;
            state.peers.insert(peer.id(), peer);
            self.save(&state);
            Ok(())
        })
    }

    fn cache_peers(&self, peers: Vec<PeerInfo>) -> BoxFuture<'_, Result<(), Self::Error>> {
        Box::pin(async move {
            let mut state = self.state.lock().map_err(|_| poisoned())?;
            for peer in peers {
                state.peers.insert(peer.id(), peer);
            }
            self.save(&state);
            Ok(())
        })
    }

    fn updates_state(&self) -> BoxFuture<'_, Result<UpdatesState, Self::Error>> {
        Box::pin(async move { Ok(self.state.lock().map_err(|_| poisoned())?.updates.clone()) })
    }

    fn set_update_state(&self, update: UpdateState) -> BoxFuture<'_, Result<(), Self::Error>> {
        Box::pin(async move {
            let mut state = self.state.lock().map_err(|_| poisoned())?;
            match update {
                UpdateState::All(all) => state.updates = all,
                UpdateState::Primary { pts, date, seq } => {
                    state.updates.pts = pts;
                    state.updates.date = date;
                    state.updates.seq = seq;
                }
                UpdateState::Secondary { qts } => state.updates.qts = qts,
                UpdateState::Channel { id, pts } => {
                    match state.updates.channels.iter_mut().find(|c| c.id == id) {
                        Some(channel) => channel.pts = pts,
                        None => state
                            .updates
                            .channels
                            .push(grammers_session::types::ChannelState { id, pts }),
                    }
                }
            }
            self.save(&state);
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grammers_session::types::UpdatesState;

    #[tokio::test]
    async fn survives_reopen() {
        let dir = std::env::temp_dir().join(format!("united-session-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.json");
        let _ = std::fs::remove_file(&path);

        let session = FileSession::open(&path);
        assert!(session.dc_option(1).unwrap().is_some(), "default DCs kept");
        session.set_home_dc_id(4).await.unwrap();
        session
            .set_update_state(UpdateState::All(UpdatesState {
                pts: 7,
                ..Default::default()
            }))
            .await
            .unwrap();
        drop(session);

        let reopened = FileSession::open(&path);
        assert_eq!(reopened.home_dc_id().unwrap(), 4);
        assert_eq!(reopened.updates_state().await.unwrap().pts, 7);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
