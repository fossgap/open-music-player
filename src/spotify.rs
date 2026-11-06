//! Spotify catalog and account work runs off the GTK thread; librespot owns playback.
mod auth;
pub mod catalog;
pub mod playback;
use crate::model::SpotifyAccount;
use auth::Job;
use catalog::{Api, LocalPlaylist, SyncResult};
use librespot_core::{authentication::Credentials, config::SessionConfig, session::Session};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

pub fn track_id(uri: &str) -> Option<&str> {
    uri.strip_prefix("spotify:track:")
        .filter(|id| catalog::valid_id(id))
}
pub fn track_url(uri: &str) -> Option<String> {
    track_id(uri).map(|id| format!("https://open.spotify.com/track/{id}"))
}

pub enum Command {
    Connect(String),
    Restore(String),
    Disconnect,
    Search(u64, String),
    Sync(Vec<LocalPlaylist>, Vec<String>),
    Publish(LocalPlaylist),
    Reload(LocalPlaylist),
    Shutdown,
}
pub enum Reply {
    Authorize(String),
    Connected(Result<Box<(SpotifyAccount, playback::Playback)>, String>),
    Disconnected(Result<(), String>),
    Search(u64, Result<Vec<crate::model::Track>, String>),
    Synced(String, Vec<LocalPlaylist>, Result<SyncResult, String>),
}
pub struct Service {
    tx: mpsc::Sender<(Job, Command)>,
    rx: mpsc::Receiver<(u64, Reply)>,
    epoch: Arc<AtomicU64>,
}
impl Default for Service {
    fn default() -> Self {
        let (tx, commands) = mpsc::channel();
        let (replies, rx) = mpsc::channel();
        std::thread::spawn(move || worker(commands, replies));
        Self {
            tx,
            rx,
            epoch: Arc::new(AtomicU64::new(0)),
        }
    }
}
impl Service {
    pub fn send(&self, command: Command) {
        let epoch = if matches!(
            command,
            Command::Connect(_) | Command::Restore(_) | Command::Disconnect | Command::Shutdown
        ) {
            self.epoch.fetch_add(1, Ordering::SeqCst) + 1
        } else {
            self.epoch.load(Ordering::SeqCst)
        };
        let _ = self.tx.send((
            Job {
                epoch,
                current: self.epoch.clone(),
            },
            command,
        ));
    }
    pub fn cancel(&self) {
        self.epoch.fetch_add(1, Ordering::SeqCst);
    }
    pub fn poll(&self) -> Vec<Reply> {
        let epoch = self.epoch.load(Ordering::SeqCst);
        self.rx
            .try_iter()
            .filter_map(|(e, reply)| (e == epoch).then_some(reply))
            .collect()
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.send(Command::Shutdown);
    }
}

fn connect(
    runtime: &tokio::runtime::Runtime,
    previous: Option<&str>,
    client_id: &str,
    job: &Job,
    replies: &mpsc::Sender<(u64, Reply)>,
) -> Result<(Api, Session, SpotifyAccount, playback::Playback), String> {
    let http = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("OpenAudioPlayer/0.1")
        .build()
        .map_err(|e| e.to_string())?;
    let (stored, token) = if let Some(expected) = previous {
        let stored = auth::read()?;
        if stored.account_id != expected {
            return Err("Saved Spotify sign-in belongs to another account. Connect again.".into());
        }
        let token = auth::refresh(&http, &stored)?;
        (stored, token)
    } else {
        let id = if client_id.is_empty() {
            SessionConfig::default().client_id
        } else {
            client_id.into()
        };
        if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(
                "Enter a valid Spotify Client ID, or leave it empty to use librespot's default."
                    .into(),
            );
        }
        let token = auth::login(&http, &id, job, |url| {
            let _ = replies.send((job.epoch, Reply::Authorize(url)));
        })?;
        let refresh_token = token
            .refresh_token
            .clone()
            .ok_or("Spotify did not provide a refresh token. Try signing in again.")?;
        (
            auth::Stored {
                refresh_token,
                client_id: id,
                account_id: String::new(),
            },
            token,
        )
    };
    let mut api = Api::new(http, stored, token);
    let account = api.profile(job)?;
    if previous.is_some_and(|id| id != account.id) {
        return Err("Spotify returned a different account. Connect again.".into());
    }
    api.stored.account_id = account.id.clone();
    let config = SessionConfig {
        client_id: api.stored.client_id.clone(),
        ..SessionConfig::default()
    };
    let session = Session::new(config, None);
    let result = runtime.block_on(async {
        tokio::time::timeout(
            Duration::from_secs(35),
            session.connect(Credentials::with_access_token(api.access()), false),
        )
        .await
    });
    if !matches!(result, Ok(Ok(()))) {
        session.shutdown();
        return Err("librespot could not connect to Spotify. Check your Premium subscription and connection, then sign in again.".into());
    }
    job.check().inspect_err(|_| session.shutdown())?;
    if session
        .get_user_attribute("type")
        .is_some_and(|t| t != "premium")
    {
        session.shutdown();
        return Err("Spotify playback requires a Premium account with librespot.".into());
    }
    auth::save(&api.stored).inspect_err(|_| session.shutdown())?;
    let _entered = runtime.enter();
    let playback = playback::Playback::new(session.clone()).inspect_err(|_| session.shutdown())?;
    Ok((api, session, account, playback))
}
fn worker(commands: mpsc::Receiver<(Job, Command)>, replies: mpsc::Sender<(u64, Reply)>) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build();
    let mut active: Option<(Api, Session)> = None;
    for (job, command) in commands {
        if matches!(command, Command::Shutdown) {
            break;
        }
        if job.check().is_err() {
            continue;
        }
        let restore = matches!(&command, Command::Restore(_));
        let reload = matches!(&command, Command::Reload(_));
        let reply = match command {
            Command::Connect(client_id) | Command::Restore(client_id) => {
                // A restore command carries an account ID, not an OAuth client ID.
                let result = runtime
                    .as_ref()
                    .map_err(|_| "Could not start Spotify's network runtime.".to_string())
                    .and_then(|runtime| {
                        connect(
                            runtime,
                            restore.then_some(client_id.as_str()),
                            if restore { "" } else { &client_id },
                            &job,
                            &replies,
                        )
                    });
                match result {
                    Ok((api, session, account, playback)) => {
                        if let Some((_, old)) = active.take() {
                            old.shutdown();
                        }
                        active = Some((api, session));
                        Reply::Connected(Ok(Box::new((account, playback))))
                    }
                    Err(error) => Reply::Connected(Err(error)),
                }
            }
            Command::Disconnect => {
                if let Some((_, session)) = active.take() {
                    session.shutdown();
                }
                Reply::Disconnected(auth::delete())
            }
            Command::Search(generation, query) => Reply::Search(
                generation,
                active
                    .as_mut()
                    .ok_or_else(|| "Connect your Spotify account first.".into())
                    .and_then(|(api, _)| api.search(&query, &job)),
            ),
            Command::Sync(before, ignored) => {
                let account = active
                    .as_ref()
                    .map(|(a, _)| a.stored.account_id.clone())
                    .unwrap_or_default();
                let result = active
                    .as_mut()
                    .ok_or_else(|| "Connect your Spotify account first.".into())
                    .and_then(|(api, _)| api.synchronize(&before, &ignored, &job));
                Reply::Synced(account, before, result)
            }
            Command::Publish(local) | Command::Reload(local) => {
                let account = active
                    .as_ref()
                    .map(|(a, _)| a.stored.account_id.clone())
                    .unwrap_or_default();
                let result = active
                    .as_mut()
                    .ok_or_else(|| "Connect your Spotify account first.".into())
                    .and_then(|(api, _)| {
                        if reload {
                            let link = local
                                .link
                                .as_ref()
                                .ok_or("This playlist is not linked to Spotify.")?;
                            if link.account_id != api.stored.account_id {
                                return Err("This playlist belongs to another account.".into());
                            }
                            api.playlist(&link.id, &job).map(|mut r| {
                                r.local_id = Some(local.local_id.clone());
                                r
                            })
                        } else {
                            api.publish(&local, &job)
                        }
                    })
                    .map(|remote| SyncResult {
                        playlists: vec![remote],
                        ..SyncResult::default()
                    });
                Reply::Synced(account, vec![local], result)
            }
            Command::Shutdown => unreachable!(),
        };
        if job.check().is_ok() {
            let _ = replies.send((job.epoch, reply));
        }
    }
    if let Some((_, session)) = active {
        session.shutdown();
    }
    if let Ok(runtime) = runtime {
        runtime.shutdown_timeout(Duration::from_secs(1));
    }
}
