//! Linux MPRIS2 integration.

#[cfg(any(target_os = "linux", test))]
mod imp {
    use crate::{action::Action, station::Station, PlaybackState};
    use mpris_server::{
        zbus::{self, fdo},
        LoopStatus, Metadata, PlaybackRate, PlaybackStatus, PlayerInterface, Property,
        RootInterface, Server, Time, TrackId, Volume,
    };
    use std::{
        collections::hash_map::DefaultHasher,
        hash::{Hash, Hasher},
        sync::Arc,
    };
    use tokio::sync::{mpsc, RwLock};
    use tracing::warn;

    #[derive(Debug)]
    enum Update {
        PlaybackState(PlaybackState),
        Station {
            station: Box<Station>,
            track_title: Option<String>,
        },
        Volume(f32),
    }

    #[derive(Clone, Debug)]
    pub struct MprisHandle {
        update_tx: mpsc::UnboundedSender<Update>,
    }

    impl MprisHandle {
        pub fn start(action_tx: mpsc::UnboundedSender<Action>, volume: f32) -> Self {
            let (update_tx, update_rx) = mpsc::unbounded_channel();
            tokio::spawn(async move {
                if let Err(error) = run(action_tx, update_rx, volume).await {
                    warn!(%error, "MPRIS integration unavailable");
                }
            });
            Self { update_tx }
        }

        pub fn set_playback_state(&self, state: PlaybackState) {
            let _ = self.update_tx.send(Update::PlaybackState(state));
        }

        pub fn set_station(&self, station: Station) {
            let _ = self.update_tx.send(Update::Station {
                station: Box::new(station),
                track_title: None,
            });
        }

        pub fn set_track_title(&self, station: Station, title: String) {
            let _ = self.update_tx.send(Update::Station {
                station: Box::new(station),
                track_title: Some(title),
            });
        }

        pub fn set_volume(&self, volume: f32) {
            let _ = self.update_tx.send(Update::Volume(volume));
        }
    }

    #[derive(Debug)]
    struct State {
        playback_status: PlaybackStatus,
        metadata: Metadata,
        volume: Volume,
    }

    #[derive(Clone, Debug)]
    struct MprisService {
        action_tx: mpsc::UnboundedSender<Action>,
        state: Arc<RwLock<State>>,
    }

    impl MprisService {
        fn send(&self, action: Action) -> fdo::Result<()> {
            self.action_tx
                .send(action)
                .map_err(|_| fdo::Error::Failed("somars event loop stopped".to_string()))
        }
    }

    impl RootInterface for MprisService {
        async fn raise(&self) -> fdo::Result<()> {
            Err(fdo::Error::NotSupported(
                "somars has no raisable desktop window".to_string(),
            ))
        }

        async fn quit(&self) -> fdo::Result<()> {
            self.send(Action::Quit)
        }

        async fn can_quit(&self) -> fdo::Result<bool> {
            Ok(true)
        }

        async fn fullscreen(&self) -> fdo::Result<bool> {
            Ok(false)
        }

        async fn set_fullscreen(&self, _fullscreen: bool) -> zbus::Result<()> {
            Ok(())
        }

        async fn can_set_fullscreen(&self) -> fdo::Result<bool> {
            Ok(false)
        }

        async fn can_raise(&self) -> fdo::Result<bool> {
            Ok(false)
        }

        async fn has_track_list(&self) -> fdo::Result<bool> {
            Ok(false)
        }

        async fn identity(&self) -> fdo::Result<String> {
            Ok("somars".to_string())
        }

        async fn desktop_entry(&self) -> fdo::Result<String> {
            Ok("somars".to_string())
        }

        async fn supported_uri_schemes(&self) -> fdo::Result<Vec<String>> {
            Ok(Vec::new())
        }

        async fn supported_mime_types(&self) -> fdo::Result<Vec<String>> {
            Ok(vec!["audio/mpeg".to_string()])
        }
    }

    impl PlayerInterface for MprisService {
        async fn next(&self) -> fdo::Result<()> {
            self.send(Action::TuneNext)
        }

        async fn previous(&self) -> fdo::Result<()> {
            self.send(Action::TunePrev)
        }

        async fn pause(&self) -> fdo::Result<()> {
            self.send(Action::Pause)
        }

        async fn play_pause(&self) -> fdo::Result<()> {
            self.send(Action::TogglePause)
        }

        async fn stop(&self) -> fdo::Result<()> {
            self.send(Action::Stop)
        }

        async fn play(&self) -> fdo::Result<()> {
            self.send(Action::ResumePlayback)
        }

        async fn seek(&self, _offset: Time) -> fdo::Result<()> {
            Ok(())
        }

        async fn set_position(&self, _track_id: TrackId, _position: Time) -> fdo::Result<()> {
            Ok(())
        }

        async fn open_uri(&self, _uri: String) -> fdo::Result<()> {
            Err(fdo::Error::NotSupported(
                "somars cannot open arbitrary URIs".to_string(),
            ))
        }

        async fn playback_status(&self) -> fdo::Result<PlaybackStatus> {
            Ok(self.state.read().await.playback_status)
        }

        async fn loop_status(&self) -> fdo::Result<LoopStatus> {
            Ok(LoopStatus::None)
        }

        async fn set_loop_status(&self, _loop_status: LoopStatus) -> zbus::Result<()> {
            Ok(())
        }

        async fn rate(&self) -> fdo::Result<PlaybackRate> {
            Ok(1.0)
        }

        async fn set_rate(&self, _rate: PlaybackRate) -> zbus::Result<()> {
            Ok(())
        }

        async fn shuffle(&self) -> fdo::Result<bool> {
            Ok(false)
        }

        async fn set_shuffle(&self, _shuffle: bool) -> zbus::Result<()> {
            Ok(())
        }

        async fn metadata(&self) -> fdo::Result<Metadata> {
            Ok(self.state.read().await.metadata.clone())
        }

        async fn volume(&self) -> fdo::Result<Volume> {
            Ok(self.state.read().await.volume)
        }

        async fn set_volume(&self, volume: Volume) -> zbus::Result<()> {
            if volume.is_finite() {
                self.action_tx
                    .send(Action::SetVolume(volume.clamp(0.0, 2.0) as f32))
                    .map_err(|_| fdo::Error::Failed("somars event loop stopped".to_string()))?;
            }
            Ok(())
        }

        async fn position(&self) -> fdo::Result<Time> {
            Ok(Time::ZERO)
        }

        async fn minimum_rate(&self) -> fdo::Result<PlaybackRate> {
            Ok(1.0)
        }

        async fn maximum_rate(&self) -> fdo::Result<PlaybackRate> {
            Ok(1.0)
        }

        async fn can_go_next(&self) -> fdo::Result<bool> {
            Ok(true)
        }

        async fn can_go_previous(&self) -> fdo::Result<bool> {
            Ok(true)
        }

        async fn can_play(&self) -> fdo::Result<bool> {
            Ok(true)
        }

        async fn can_pause(&self) -> fdo::Result<bool> {
            Ok(true)
        }

        async fn can_seek(&self) -> fdo::Result<bool> {
            Ok(false)
        }

        async fn can_control(&self) -> fdo::Result<bool> {
            Ok(true)
        }
    }

    async fn run(
        action_tx: mpsc::UnboundedSender<Action>,
        mut update_rx: mpsc::UnboundedReceiver<Update>,
        volume: f32,
    ) -> zbus::Result<()> {
        let state = initial_state(volume);
        let service = MprisService {
            action_tx,
            state: state.clone(),
        };
        let server = Server::new("somars", service).await?;

        while let Some(update) = update_rx.recv().await {
            let property = match update {
                Update::PlaybackState(playback_state) => {
                    let playback_status = to_mpris_playback_status(playback_state);
                    state.write().await.playback_status = playback_status;
                    Property::PlaybackStatus(playback_status)
                }
                Update::Station {
                    station,
                    track_title,
                } => {
                    let metadata = build_metadata(&station, track_title.as_deref());
                    state.write().await.metadata = metadata.clone();
                    Property::Metadata(metadata)
                }
                Update::Volume(volume) => {
                    let volume = f64::from(volume.clamp(0.0, 2.0));
                    state.write().await.volume = volume;
                    Property::Volume(volume)
                }
            };
            server.properties_changed([property]).await?;
        }

        Ok(())
    }

    fn initial_state(volume: f32) -> Arc<RwLock<State>> {
        Arc::new(RwLock::new(State {
            playback_status: PlaybackStatus::Stopped,
            metadata: Metadata::builder().trackid(TrackId::NO_TRACK).build(),
            volume: f64::from(volume.clamp(0.0, 2.0)),
        }))
    }

    fn to_mpris_playback_status(state: PlaybackState) -> PlaybackStatus {
        match state {
            PlaybackState::Playing => PlaybackStatus::Playing,
            PlaybackState::Paused => PlaybackStatus::Paused,
            PlaybackState::Stopped => PlaybackStatus::Stopped,
        }
    }

    fn build_metadata(station: &Station, track_title: Option<&str>) -> Metadata {
        let mut hasher = DefaultHasher::new();
        station.id.hash(&mut hasher);
        track_title.hash(&mut hasher);
        let track_id = TrackId::try_from(format!(
            "/me/vasiliev/somars/track/t{:016x}",
            hasher.finish()
        ))
        .expect("generated MPRIS track ID must be a valid D-Bus object path");

        let mut builder = Metadata::builder()
            .trackid(track_id)
            .title(track_title.unwrap_or(&station.title))
            .album(&station.title)
            .url(&station.url);

        if !station.dj.is_empty() {
            builder = builder.artist([station.dj.clone()]);
        }
        if !station.genre.is_empty() {
            builder = builder.genre([station.genre.clone()]);
        }
        if !station.image.is_empty() {
            builder = builder.art_url(&station.image);
        }

        builder.build()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn station() -> Station {
            Station {
                id: "groove-salad".to_string(),
                title: "Groove Salad".to_string(),
                description: "Ambient beats".to_string(),
                dj: "SomaFM".to_string(),
                genre: "ambient".to_string(),
                url: "https://ice1.somafm.com/groovesalad-128-mp3".to_string(),
                image: "https://somafm.com/img/groovesalad120.png".to_string(),
                last_playing: String::new(),
            }
        }

        #[test]
        fn metadata_contains_station_and_track() {
            let metadata = build_metadata(&station(), Some("Artist - Track"));
            assert_eq!(metadata.title(), Some("Artist - Track"));
            assert_eq!(metadata.album(), Some("Groove Salad"));
            assert_eq!(metadata.artist(), Some(vec!["SomaFM".to_string()]));
            assert!(metadata.trackid().is_some());
        }

        #[test]
        fn track_id_changes_with_icy_title() {
            let station = station();
            let first = build_metadata(&station, Some("First"));
            let second = build_metadata(&station, Some("Second"));
            assert_ne!(first.trackid(), second.trackid());
        }

        #[tokio::test]
        async fn dbus_play_pause_routes_action() {
            if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
                return;
            }

            let (action_tx, mut action_rx) = mpsc::unbounded_channel();
            let service = MprisService {
                action_tx,
                state: initial_state(1.0),
            };
            let suffix = format!("somars.test{}", std::process::id());
            let bus_name = format!("org.mpris.MediaPlayer2.{suffix}");
            let _server = Server::new(&suffix, service).await.unwrap();
            let connection = zbus::Connection::session().await.unwrap();
            let proxy = zbus::Proxy::new(
                &connection,
                bus_name,
                "/org/mpris/MediaPlayer2",
                "org.mpris.MediaPlayer2.Player",
            )
            .await
            .unwrap();

            proxy.call::<_, _, ()>("PlayPause", &()).await.unwrap();

            assert_eq!(
                tokio::time::timeout(std::time::Duration::from_secs(1), action_rx.recv())
                    .await
                    .unwrap(),
                Some(Action::TogglePause)
            );
        }
    }
}

#[cfg(all(not(target_os = "linux"), not(test)))]
mod imp {
    use crate::{action::Action, station::Station, PlaybackState};
    use tokio::sync::mpsc;

    #[derive(Clone, Debug)]
    pub struct MprisHandle;

    impl MprisHandle {
        pub fn start(_action_tx: mpsc::UnboundedSender<Action>, _volume: f32) -> Self {
            Self
        }

        pub fn set_playback_state(&self, _state: PlaybackState) {}

        pub fn set_station(&self, _station: Station) {}

        pub fn set_track_title(&self, _station: Station, _title: String) {}

        pub fn set_volume(&self, _volume: f32) {}
    }
}

pub use imp::MprisHandle;
