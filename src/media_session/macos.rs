use crate::{action::Action, station::Station, PlaybackState};
use anyhow::{bail, Context};
use dispatch::Queue;
use mediaplayer::prelude::{
    Artwork, CommandEvent, CommandToken, HandlerStatus, NowPlayingInfo, NowPlayingInfoCenter,
    NowPlayingMediaType, PlaybackState as NativePlaybackState, RemoteCommandCenter,
};
use std::{
    cell::RefCell,
    collections::{hash_map::DefaultHasher, HashSet},
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tracing::warn;

const MAX_ARTWORK_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Clone, Debug)]
struct SessionState {
    playback_state: PlaybackState,
    station: Option<(Station, Option<String>)>,
    artwork_path: Option<PathBuf>,
}

struct MainThreadSession {
    center: NowPlayingInfoCenter,
    artwork_path: Option<PathBuf>,
    artwork: Option<Artwork>,
    _tokens: Vec<CommandToken>,
}

thread_local! {
    static MAIN_THREAD_SESSION: RefCell<Option<MainThreadSession>> = const {
        RefCell::new(None)
    };
}

#[derive(Debug)]
pub struct MediaSessionHandle {
    state: Arc<Mutex<SessionState>>,
    artwork_requests: Arc<Mutex<HashSet<String>>>,
}

impl MediaSessionHandle {
    pub fn start(action_tx: tokio::sync::mpsc::UnboundedSender<Action>, _volume: f32) -> Self {
        Queue::main().exec_async(move || initialize(action_tx));
        Self {
            state: Arc::new(Mutex::new(SessionState {
                playback_state: PlaybackState::Stopped,
                station: None,
                artwork_path: None,
            })),
            artwork_requests: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn set_playback_state(&self, state: PlaybackState) {
        self.update(|session| session.playback_state = state);
    }

    pub fn set_station(&self, station: Station) {
        self.set_station_metadata(station, None);
    }

    pub fn set_track_title(&self, station: Station, title: String) {
        self.set_station_metadata(station, Some(title));
    }

    pub fn set_volume(&self, _volume: f32) {}

    fn update(&self, update: impl FnOnce(&mut SessionState)) {
        let snapshot = {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            update(&mut state);
            state.clone()
        };
        dispatch(snapshot);
    }

    fn set_station_metadata(&self, station: Station, track_title: Option<String>) {
        let artwork_url = station.image.trim().to_string();
        let cached_path = cached_artwork_path(&artwork_url);
        let snapshot = {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            let same_artwork = state
                .station
                .as_ref()
                .is_some_and(|(current, _)| current.image.trim() == artwork_url);
            state.artwork_path = if same_artwork {
                state.artwork_path.clone().or(cached_path)
            } else {
                cached_path
            };
            state.station = Some((station, track_title));
            state.clone()
        };
        let needs_download = !artwork_url.is_empty() && snapshot.artwork_path.is_none();
        dispatch(snapshot);
        if needs_download {
            self.request_artwork(artwork_url);
        }
    }

    fn request_artwork(&self, artwork_url: String) {
        let should_start = self
            .artwork_requests
            .lock()
            .is_ok_and(|mut requests| requests.insert(artwork_url.clone()));
        if !should_start {
            return;
        }

        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            if let Ok(mut requests) = self.artwork_requests.lock() {
                requests.remove(&artwork_url);
            }
            warn!(url = %artwork_url, "cannot download macOS artwork outside Tokio runtime");
            return;
        };
        let state = Arc::clone(&self.state);
        let requests = Arc::clone(&self.artwork_requests);
        runtime.spawn(async move {
            let result = download_artwork(&artwork_url).await;
            if let Ok(mut requests) = requests.lock() {
                requests.remove(&artwork_url);
            }

            match result {
                Ok(path) => {
                    let snapshot = {
                        let Ok(mut state) = state.lock() else {
                            return;
                        };
                        let is_current = state
                            .station
                            .as_ref()
                            .is_some_and(|(station, _)| station.image.trim() == artwork_url);
                        if !is_current {
                            return;
                        }
                        state.artwork_path = Some(path);
                        state.clone()
                    };
                    dispatch(snapshot);
                }
                Err(error) => {
                    warn!(%error, url = %artwork_url, "failed to download macOS artwork");
                }
            }
        });
    }
}

impl Drop for MediaSessionHandle {
    fn drop(&mut self) {
        Queue::main().exec_async(|| {
            MAIN_THREAD_SESSION.with(|session| {
                session.borrow_mut().take();
            });
        });
    }
}

fn initialize(action_tx: tokio::sync::mpsc::UnboundedSender<Action>) {
    let commands = RemoteCommandCenter::shared();
    enable_commands(&commands);
    let session = MainThreadSession {
        center: NowPlayingInfoCenter::default_center(),
        artwork_path: None,
        artwork: None,
        _tokens: command_tokens(&commands, action_tx),
    };
    MAIN_THREAD_SESSION.with(|slot| *slot.borrow_mut() = Some(session));
}

fn dispatch(state: SessionState) {
    Queue::main().exec_async(move || apply(state));
}

fn apply(state: SessionState) {
    MAIN_THREAD_SESSION.with(|session| {
        let mut session = session.borrow_mut();
        let Some(session) = session.as_mut() else {
            return;
        };
        if let Some((station, track_title)) = state.station.as_ref() {
            publish_metadata(
                session,
                station,
                track_title.as_deref(),
                &state.playback_state,
                state.artwork_path.as_deref(),
            );
        }
        session
            .center
            .set_playback_state(to_native_playback_state(&state.playback_state));
    });
}

fn enable_commands(commands: &RemoteCommandCenter) {
    commands.play_command().set_enabled(true);
    commands.pause_command().set_enabled(true);
    commands.stop_command().set_enabled(true);
    commands.toggle_play_pause_command().set_enabled(true);
    commands.next_track_command().set_enabled(true);
    commands.previous_track_command().set_enabled(true);
}

fn command_tokens(
    commands: &RemoteCommandCenter,
    action_tx: tokio::sync::mpsc::UnboundedSender<Action>,
) -> Vec<CommandToken> {
    vec![
        commands.on_play(command_handler(action_tx.clone(), Action::Play)),
        commands.on_pause(command_handler(action_tx.clone(), Action::Pause)),
        commands.on_stop(command_handler(action_tx.clone(), Action::Stop)),
        commands.on_toggle_play_pause(command_handler(action_tx.clone(), Action::TogglePause)),
        commands.on_next_track(command_handler(action_tx.clone(), Action::TuneNext)),
        commands.on_previous_track(command_handler(action_tx, Action::TunePrev)),
    ]
}

fn command_handler(
    action_tx: tokio::sync::mpsc::UnboundedSender<Action>,
    action: Action,
) -> impl FnMut(CommandEvent) -> HandlerStatus + Send + 'static {
    move |_| {
        if action_tx.send(action.clone()).is_ok() {
            HandlerStatus::Success
        } else {
            HandlerStatus::CommandFailed
        }
    }
}

fn publish_metadata(
    session: &mut MainThreadSession,
    station: &Station,
    track_title: Option<&str>,
    playback_state: &PlaybackState,
    artwork_path: Option<&Path>,
) {
    let mut info = NowPlayingInfo::new()
        .title(track_title.unwrap_or(&station.title))
        .album_title(&station.title)
        .live_stream(true)
        .media_type(NowPlayingMediaType::Audio)
        .asset_url(&station.url)
        .service_identifier("somars")
        .default_playback_rate(1.0)
        .playback_rate(if matches!(playback_state, PlaybackState::Playing) {
            1.0
        } else {
            0.0
        });

    if !station.dj.is_empty() {
        info = info.artist(&station.dj);
    }

    if session.artwork_path.as_deref() != artwork_path {
        session.artwork_path = artwork_path.map(Path::to_path_buf);
        session.artwork = artwork_path.and_then(|path| {
            let path = path.to_string_lossy();
            match Artwork::from_path(&path) {
                Ok(artwork) => Some(artwork),
                Err(error) => {
                    warn!(%error, path = %path, "failed to load macOS artwork");
                    None
                }
            }
        });
    }

    session
        .center
        .set_now_playing_info_with_artwork(&info, session.artwork.as_ref());
}

fn cached_artwork_path(url: &str) -> Option<PathBuf> {
    if url.is_empty() {
        return None;
    }
    let path = artwork_cache_path(url);
    std::fs::metadata(&path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
        .then_some(path)
}

fn artwork_cache_path(url: &str) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("somars")
        .join("artwork")
        .join(format!("{:016x}", hasher.finish()))
}

async fn download_artwork(url: &str) -> anyhow::Result<PathBuf> {
    let path = artwork_cache_path(url);
    if std::fs::metadata(&path).is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0) {
        return Ok(path);
    }

    let response = reqwest::get(url)
        .await
        .with_context(|| format!("request failed: {url}"))?
        .error_for_status()
        .with_context(|| format!("server rejected artwork: {url}"))?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_ARTWORK_BYTES)
    {
        bail!("artwork exceeds {MAX_ARTWORK_BYTES} bytes");
    }
    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("failed to read artwork: {url}"))?;
    if bytes.is_empty() {
        bail!("artwork response is empty");
    }
    if bytes.len() as u64 > MAX_ARTWORK_BYTES {
        bail!("artwork exceeds {MAX_ARTWORK_BYTES} bytes");
    }

    let parent = path.parent().context("artwork cache has no parent")?;
    tokio::fs::create_dir_all(parent)
        .await
        .context("failed to create artwork cache")?;
    let temporary_path = path.with_extension(format!("tmp-{}", std::process::id()));
    tokio::fs::write(&temporary_path, &bytes)
        .await
        .context("failed to write artwork cache")?;
    tokio::fs::rename(&temporary_path, &path)
        .await
        .context("failed to commit artwork cache")?;
    Ok(path)
}

fn to_native_playback_state(state: &PlaybackState) -> NativePlaybackState {
    match state {
        PlaybackState::Playing => NativePlaybackState::Playing,
        PlaybackState::Paused => NativePlaybackState::Paused,
        PlaybackState::Stopped => NativePlaybackState::Stopped,
    }
}
