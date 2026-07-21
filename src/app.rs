//! Main application struct
//!
//! The App manages the component collection, event loop, and application state.

use crate::{
    action::Action,
    audio,
    components::{BottomControls, Component, Help, History, NowPlaying, StationList},
    config::Config,
    event::Event,
    media_session::MediaSessionHandle,
    station::Station,
    tui::Tui,
    MessageType, PlaybackState,
};
use color_eyre::eyre::Result;
use crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Direction, Layout as RatatuiLayout, Rect};
use rodio::Sink;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tracing::{debug, info};

// Component indices - must match order in App::new()
const COMPONENT_STATION_LIST: usize = 0;
const COMPONENT_NOW_PLAYING: usize = 1;
const COMPONENT_HISTORY: usize = 2;
const COMPONENT_HELP: usize = 3;
const COMPONENT_BOTTOM_CONTROLS: usize = 4;

/// History message type alias - use the one from main.rs
pub type HistoryMessage = crate::HistoryMessage;

/// Main application struct
pub struct App {
    // Components
    components: Vec<Box<dyn Component>>,

    // Shared state
    pub config: Config,
    pub stations: Vec<Station>,
    pub active_station: Option<usize>,
    pub selected_station: usize,

    // Playback state
    pub playback_state: PlaybackState,
    pub volume: f32,
    media_session: MediaSessionHandle,

    // Audio
    #[allow(dead_code)]
    pub audio_manager: audio::AudioManager,
    pub sink: Option<Arc<Mutex<Sink>>>,
    pub metadata_tx: mpsc::Sender<audio::MetadataEvent>,
    pub log_tx: mpsc::Sender<HistoryMessage>,

    // Playback timing
    pub playback_start_time: Option<Instant>,
    pub total_played: std::time::Duration,
    pub last_pause_time: Option<Instant>,
    pub playback_start_time_for_underrun: Option<Instant>,
    #[allow(dead_code)]
    pub last_position: std::time::Duration,
    #[allow(dead_code)]
    pub last_underrun_check: Option<Instant>,
    pub last_restart_time: Option<Instant>,
    pub restart_attempts: u32,
    #[allow(dead_code)]
    pub underrun_detected: bool,
    pub station_loading: bool,

    // Channels
    pub action_tx: UnboundedSender<Action>,
    action_rx: UnboundedReceiver<Action>,

    // State
    pub should_quit: bool,
    pub loading: bool,

    // UI state
    pub history_messages: Vec<HistoryMessage>,
    pub log_level: u8,

    // UDP control state
    #[allow(dead_code)]
    pub udp_enabled: bool,
    #[allow(dead_code)]
    pub udp_port: u16,

    // Initial station to play (from CLI or config)
    pub initial_station: Option<String>,
    pub auto_played: bool,
}

impl App {
    fn abort_playback_task(&mut self) {
        if let Some(handle) = self.audio_manager.take_handle() {
            handle.abort();
        }
        self.audio_manager.clear_current_station();
    }

    /// Create a new application instance
    pub fn new(
        sink: Arc<Mutex<Sink>>,
        metadata_tx: mpsc::Sender<audio::MetadataEvent>,
        log_tx: mpsc::Sender<HistoryMessage>,
        config: Config,
        initial_station: Option<String>,
    ) -> Self {
        let (action_tx, action_rx) = mpsc::unbounded_channel();

        let log_level = config.log_level;
        let volume = config.volume;
        let udp_enabled = config.udp_enabled;
        let udp_port = config.udp_port;
        let media_session = MediaSessionHandle::start(action_tx.clone(), volume);

        // Create components
        let components: Vec<Box<dyn Component>> = vec![
            Box::new(StationList::new()),
            Box::new(NowPlaying::new()),
            Box::new(History::new()),
            Box::new(Help::new()),
            Box::new(BottomControls::new()),
        ];

        Self {
            components,
            config,
            stations: Vec::new(),
            active_station: None,
            selected_station: 0,
            playback_state: PlaybackState::Stopped,
            volume,
            media_session,
            audio_manager: audio::AudioManager::new(),
            sink: Some(sink),
            metadata_tx,
            log_tx,
            playback_start_time: None,
            total_played: std::time::Duration::default(),
            last_pause_time: None,
            playback_start_time_for_underrun: None,
            last_position: std::time::Duration::default(),
            last_underrun_check: None,
            last_restart_time: None,
            restart_attempts: 0,
            underrun_detected: false,
            station_loading: false,
            action_tx,
            action_rx,
            should_quit: false,
            loading: true,
            history_messages: Vec::new(),
            log_level,
            udp_enabled,
            udp_port,
            initial_station,
            auto_played: false,
        }
    }

    /// Helper to add a history message
    fn add_history_message(&mut self, message: String, message_type: MessageType) {
        let history_msg = HistoryMessage {
            message,
            message_type,
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
        };
        self.history_messages.push(history_msg.clone());

        // Keep only last 1000 messages
        while self.history_messages.len() > 1000 {
            self.history_messages.remove(0);
        }

        // Forward to the History component
        let _ = self.action_tx.send(Action::AddHistoryMessage(history_msg));
    }

    /// Run the application
    pub async fn run(&mut self) -> Result<()> {
        let mut tui = Tui::new()?;
        let mut animation_interval = tokio::time::interval(Duration::from_millis(250));
        animation_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        tui.enter()?;

        // Initialize components
        for component in self.components.iter_mut() {
            component.register_action_handler(self.action_tx.clone())?;
            component.register_config_handler(self.config.clone())?;
            component.init(tui.size()?)?;
        }

        // Sync log level to History component
        if let Some(history) = self.components.get_mut(COMPONENT_HISTORY) {
            let _ = history.update(Action::SetLogLevel(self.log_level));
        }

        // Main event loop
        loop {
            let animation_active = self.loading || self.playback_state == PlaybackState::Playing;
            tokio::select! {
                event = tui.next_event() => {
                    if let Some(event) = event {
                        self.handle_event(event)?;
                        self.handle_actions(&mut tui, None)?;
                    }
                }
                action = self.action_rx.recv() => {
                    if let Some(action) = action {
                        self.handle_actions(&mut tui, Some(action))?;
                    }
                }
                _ = animation_interval.tick(), if animation_active => {
                    self.handle_actions(&mut tui, Some(Action::Tick))?;
                }
            }

            if self.should_quit {
                break;
            }
        }

        tui.exit()?;
        Ok(())
    }

    /// Handle events from the TUI
    fn handle_event(&mut self, event: Event) -> Result<()> {
        // Convert events to actions
        match event {
            Event::Init => {
                self.action_tx.send(Action::Render)?;
            }
            Event::Quit => {
                self.action_tx.send(Action::Quit)?;
            }
            Event::Resize(w, h) => {
                self.action_tx.send(Action::Resize(w, h))?;
            }
            Event::Key(key) => {
                self.handle_key_event(key)?;
                // Some components mutate their state directly on key events.
                self.action_tx.send(Action::Render)?;
            }
            _ => {}
        }

        // Forward events to components
        for component in self.components.iter_mut() {
            if let Some(action) = component.handle_events(Some(event.clone()))? {
                self.action_tx.send(action)?;
            }
        }

        Ok(())
    }

    /// Handle keyboard events
    fn handle_key_event(&mut self, key: KeyEvent) -> Result<()> {
        use crossterm::event::KeyCode;

        // Check for Ctrl+C
        if key.code == KeyCode::Char('c')
            && key
                .modifiers
                .contains(crossterm::event::KeyModifiers::CONTROL)
        {
            info!("Ctrl+C detected, initiating graceful shutdown");
            self.should_quit = true;
            return Ok(());
        }

        // Handle global keyboard shortcuts
        match key.code {
            KeyCode::Char('q') => {
                self.action_tx.send(Action::Quit)?;
                return Ok(());
            }
            KeyCode::Char(' ') => {
                self.action_tx.send(Action::TogglePause)?;
                return Ok(());
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.action_tx.send(Action::VolumeUp)?;
                return Ok(());
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                self.action_tx.send(Action::VolumeDown)?;
                return Ok(());
            }
            _ => {
                // For other keys, don't process them here - let components handle them via handle_events
                // This prevents double processing of key events
            }
        }

        Ok(())
    }

    /// Handle actions from components
    fn handle_actions(&mut self, tui: &mut Tui, first_action: Option<Action>) -> Result<()> {
        let mut needs_render = false;
        let mut next_action = first_action;
        loop {
            let action = match next_action.take() {
                Some(action) => action,
                None => match self.action_rx.try_recv() {
                    Ok(action) => action,
                    Err(_) => break,
                },
            };
            if action != Action::Tick && action != Action::Render {
                debug!(?action);
            }

            // Handle app-level actions
            match &action {
                Action::Quit => {
                    self.should_quit = true;
                }
                Action::Render => {
                    needs_render = true;
                }
                Action::Resize(w, h) => {
                    tui.resize(Rect::new(0, 0, *w, *h))?;
                    needs_render = true;
                }
                Action::UpdateStations(stations) => {
                    self.stations = stations.clone();
                    self.loading = false;
                }
                Action::SetActiveStation(idx) => {
                    self.active_station = *idx;
                    if let Some(station) = idx.and_then(|idx| self.stations.get(idx)) {
                        self.media_session.set_station(station.clone());
                    }
                }
                Action::SetPlaybackState(state) => {
                    self.playback_state = state.clone();
                    self.media_session.set_playback_state(state.clone());
                }
                Action::SetVolume(level) => {
                    self.volume = level.clamp(0.0, 2.0);
                    if let Some(ref sink) = self.sink {
                        if let Ok(sink) = sink.lock() {
                            sink.set_volume(self.volume);
                        }
                    }
                    self.media_session.set_volume(self.volume);
                }
                Action::MetadataUpdate { station, title } => {
                    if let Some(active_station) = self
                        .active_station
                        .and_then(|index| self.stations.get(index))
                        .filter(|active_station| active_station.title == *station)
                    {
                        self.media_session
                            .set_track_title(active_station.clone(), title.clone());
                    }
                }
                Action::Error(msg) => {
                    self.add_history_message(msg.clone(), MessageType::Error);
                }
                Action::ToggleHelp => {
                    // Show help visibility synchronously in components
                    for component in self.components.iter_mut() {
                        let _ = component.update(Action::ToggleHelp);
                    }
                    // Mark that we need to render immediately
                    needs_render = true;
                }
                Action::Play => {
                    self.play_station()?;
                    // Trigger render to show playback state
                    self.action_tx.send(Action::Render)?;
                }
                Action::Stop => {
                    self.stop_playback();
                    // Trigger render to show playback state
                    self.action_tx.send(Action::Render)?;
                }
                Action::TogglePlayStop => {
                    match self.playback_state {
                        PlaybackState::Stopped => self.play_station()?,
                        PlaybackState::Playing | PlaybackState::Paused => self.stop_playback(),
                    }
                    self.action_tx.send(Action::Render)?;
                }
                Action::TogglePause => {
                    self.toggle_pause()?;
                    // Trigger render to show playback state
                    self.action_tx.send(Action::Render)?;
                }
                Action::Pause => {
                    self.pause_playback();
                    // Trigger render to show playback state
                    self.action_tx.send(Action::Render)?;
                }
                Action::ResumePlayback => {
                    self.resume_playback()?;
                    // Trigger render to show playback state
                    self.action_tx.send(Action::Render)?;
                }
                Action::VolumeUp => {
                    self.volume_up();
                    // Sync volume to components
                    self.action_tx.send(Action::SetVolume(self.volume))?;
                    // Trigger render to show volume
                    self.action_tx.send(Action::Render)?;
                }
                Action::VolumeDown => {
                    self.volume_down();
                    // Sync volume to components
                    self.action_tx.send(Action::SetVolume(self.volume))?;
                    // Trigger render to show volume
                    self.action_tx.send(Action::Render)?;
                }
                Action::TuneStation(station_id) => {
                    if let Some(index) = self.stations.iter().position(|s| s.id == *station_id) {
                        self.selected_station = index;
                        // Update NowPlaying component with the selected station details
                        if let Some(now_playing) = self.components.get_mut(COMPONENT_NOW_PLAYING) {
                            if let Some(station) = self.stations.get(self.selected_station).cloned()
                            {
                                let _ =
                                    now_playing.update(Action::SetSelectedStation(Some(station)));
                            } else {
                                let _ = now_playing.update(Action::SetSelectedStation(None));
                            }
                        }
                        self.play_station()?;
                    }
                }
                Action::TuneNext => {
                    if !self.stations.is_empty() {
                        let current = self.selected_station;
                        self.selected_station = if current == self.stations.len() - 1 {
                            0
                        } else {
                            current + 1
                        };
                        // Update NowPlaying component with the selected station details
                        if let Some(now_playing) = self.components.get_mut(COMPONENT_NOW_PLAYING) {
                            if let Some(station) = self.stations.get(self.selected_station).cloned()
                            {
                                let _ =
                                    now_playing.update(Action::SetSelectedStation(Some(station)));
                            } else {
                                let _ = now_playing.update(Action::SetSelectedStation(None));
                            }
                        }
                        self.play_station()?;
                    }
                }
                Action::TunePrev => {
                    if !self.stations.is_empty() {
                        let current = self.selected_station;
                        self.selected_station = if current == 0 {
                            self.stations.len() - 1
                        } else {
                            current - 1
                        };
                        // Update NowPlaying component with the selected station details
                        if let Some(now_playing) = self.components.get_mut(COMPONENT_NOW_PLAYING) {
                            if let Some(station) = self.stations.get(self.selected_station).cloned()
                            {
                                let _ =
                                    now_playing.update(Action::SetSelectedStation(Some(station)));
                            } else {
                                let _ = now_playing.update(Action::SetSelectedStation(None));
                            }
                        }
                        self.play_station()?;
                    }
                }
                Action::StationUp => {
                    if self.selected_station > 0 {
                        self.selected_station -= 1;
                    }
                    // Sync with StationList component
                    if let Some(station_list) = self.components.get_mut(COMPONENT_STATION_LIST) {
                        let _ = station_list.update(Action::SelectStation(self.selected_station));
                    }
                    // Update NowPlaying component with the selected station details
                    if let Some(now_playing) = self.components.get_mut(COMPONENT_NOW_PLAYING) {
                        if let Some(station) = self.stations.get(self.selected_station).cloned() {
                            let _ = now_playing.update(Action::SetSelectedStation(Some(station)));
                        } else {
                            let _ = now_playing.update(Action::SetSelectedStation(None));
                        }
                    }
                    // Trigger render to update selection highlight
                    self.action_tx.send(Action::Render)?;
                }
                Action::StationDown => {
                    if self.selected_station < self.stations.len().saturating_sub(1) {
                        self.selected_station += 1;
                    }
                    // Sync with StationList component
                    if let Some(station_list) = self.components.get_mut(COMPONENT_STATION_LIST) {
                        let _ = station_list.update(Action::SelectStation(self.selected_station));
                    }
                    // Update NowPlaying component with the selected station details
                    if let Some(now_playing) = self.components.get_mut(COMPONENT_NOW_PLAYING) {
                        if let Some(station) = self.stations.get(self.selected_station).cloned() {
                            let _ = now_playing.update(Action::SetSelectedStation(Some(station)));
                        } else {
                            let _ = now_playing.update(Action::SetSelectedStation(None));
                        }
                    }
                    // Trigger render to update selection highlight
                    self.action_tx.send(Action::Render)?;
                }
                Action::SelectStation(idx) => {
                    if *idx < self.stations.len() {
                        self.selected_station = *idx;
                    }
                    // Sync with StationList component
                    if let Some(station_list) = self.components.get_mut(COMPONENT_STATION_LIST) {
                        let _ = station_list.update(Action::SelectStation(self.selected_station));
                    }
                    // Update NowPlaying component with the selected station details
                    if let Some(now_playing) = self.components.get_mut(COMPONENT_NOW_PLAYING) {
                        if let Some(station) = self.stations.get(self.selected_station).cloned() {
                            let _ = now_playing.update(Action::SetSelectedStation(Some(station)));
                        } else {
                            let _ = now_playing.update(Action::SetSelectedStation(None));
                        }
                    }
                }
                _ => {}
            }

            // Forward to all components, but skip actions already handled at app level
            // to avoid double-processing
            let should_forward = match &action {
                // These actions are handled at app level and should not be forwarded to components
                Action::Play
                | Action::Stop
                | Action::TogglePlayStop
                | Action::TogglePause
                | Action::Pause
                | Action::ResumePlayback
                | Action::VolumeUp
                | Action::VolumeDown
                | Action::SetVolume(_)
                | Action::TuneStation(_)
                | Action::TuneNext
                | Action::TunePrev
                | Action::StationUp
                | Action::StationDown
                | Action::Tick
                | Action::Render
                | Action::Quit => false,
                _ => true,
            };

            if should_forward {
                for component in self.components.iter_mut() {
                    if let Some(new_action) = component.update(action.clone())? {
                        self.action_tx.send(new_action)?;
                    }
                }
            }

            // Always update components with state changes (but not as actions)
            // This ensures components reflect the current app state
            match &action {
                Action::UpdateStations(ref stations) => {
                    self.stations = stations.clone();
                    // Update StationList component with new stations
                    if let Some(station_list) = self.components.get_mut(COMPONENT_STATION_LIST) {
                        let _ = station_list.update(action.clone());
                    }
                    // Update NowPlaying component with the selected station if it exists
                    if let Some(now_playing) = self.components.get_mut(COMPONENT_NOW_PLAYING) {
                        if let Some(station) = self.stations.get(self.selected_station).cloned() {
                            let _ = now_playing.update(Action::SetSelectedStation(Some(station)));
                        } else {
                            let _ = now_playing.update(Action::SetSelectedStation(None));
                        }
                    }

                    // Handle initial station selection (from CLI or config)
                    if !self.auto_played {
                        if let Some(ref station_id) = self.initial_station {
                            if let Some(idx) = stations.iter().position(|s| s.id == *station_id) {
                                self.selected_station = idx;
                                // Update components with the selection
                                if let Some(station_list) =
                                    self.components.get_mut(COMPONENT_STATION_LIST)
                                {
                                    let _ = station_list.update(Action::SelectStation(idx));
                                }
                                if let Some(station) = stations.get(idx).cloned() {
                                    if let Some(now_playing) =
                                        self.components.get_mut(COMPONENT_NOW_PLAYING)
                                    {
                                        let _ = now_playing
                                            .update(Action::SetSelectedStation(Some(station)));
                                    }
                                }
                                // Start playback
                                let _ = self.play_station();
                                self.auto_played = true;
                            }
                        }
                    }
                }
                Action::SetActiveStation(_idx) => {
                    // Update StationList component with active station
                    if let Some(station_list) = self.components.get_mut(COMPONENT_STATION_LIST) {
                        let _ = station_list.update(action.clone());
                    }
                    // Update NowPlaying component
                    if let Some(now_playing) = self.components.get_mut(COMPONENT_NOW_PLAYING) {
                        let _ = now_playing.update(action.clone());
                    }
                }
                Action::SetPlaybackState(state) => {
                    // Update the app's playback state first
                    let old_state = self.playback_state.clone();
                    self.playback_state = state.clone();

                    // Check if we need to start/stop tracking play time
                    if matches!(old_state, PlaybackState::Stopped | PlaybackState::Paused)
                        && matches!(state, PlaybackState::Playing)
                    {
                        // Starting playback - start tracking play time
                        if let Some(history) = self.components.get_mut(COMPONENT_HISTORY) {
                            let _ = history.update(Action::SetTotalPlayed(self.total_played));
                            let _ = history.update(Action::StartTrackingPlayTime);
                        }
                    } else if matches!(old_state, PlaybackState::Playing)
                        && !matches!(state, PlaybackState::Playing)
                    {
                        // Stopping playback - stop tracking and accumulate time
                        if let Some(history) = self.components.get_mut(COMPONENT_HISTORY) {
                            let _ = history.update(Action::StopTrackingPlayTime);
                            // Update with the accumulated time
                            let _ = history.update(Action::SetTotalPlayed(self.total_played));
                        }
                    } else {
                        // Just update the timing information normally
                        if let Some(history) = self.components.get_mut(COMPONENT_HISTORY) {
                            let _ = history.update(Action::SetTotalPlayed(self.total_played));
                        }
                    }

                    // Update NowPlaying component
                    if let Some(now_playing) = self.components.get_mut(COMPONENT_NOW_PLAYING) {
                        let _ = now_playing.update(action.clone());
                    }
                    // Update History component with the state change
                    if let Some(history) = self.components.get_mut(COMPONENT_HISTORY) {
                        let _ = history.update(action.clone());
                    }
                }
                Action::SetVolume(_level) => {
                    // Update NowPlaying component
                    if let Some(now_playing) = self.components.get_mut(COMPONENT_NOW_PLAYING) {
                        let _ = now_playing.update(action.clone());
                    }
                    // Update BottomControls component
                    if let Some(bottom_controls) =
                        self.components.get_mut(COMPONENT_BOTTOM_CONTROLS)
                    {
                        let _ = bottom_controls.update(action.clone());
                    }
                }
                Action::Tick => {
                    // Update all components with tick
                    for component in self.components.iter_mut() {
                        let _ = component.update(action.clone());
                    }
                    // Update History component with latest timing information
                    if let Some(history) = self.components.get_mut(COMPONENT_HISTORY) {
                        // Calculate current played time including current session
                        let current_played = if self.playback_state == PlaybackState::Playing {
                            if let Some(start) = self.playback_start_time {
                                self.total_played + start.elapsed()
                            } else {
                                self.total_played
                            }
                        } else {
                            self.total_played
                        };
                        let _ = history.update(Action::SetTotalPlayed(current_played));
                        // Update the playback state as well to ensure it's in sync
                        let _ =
                            history.update(Action::SetPlaybackState(self.playback_state.clone()));
                    }
                    // Animate only while visible state changes. Idle stays event-driven.
                    needs_render |= self.loading || self.playback_state == PlaybackState::Playing;
                }
                _ => {}
            }

            if !matches!(
                action,
                Action::Tick | Action::Render | Action::MetadataUpdate { .. } | Action::Quit
            ) {
                needs_render = true;
            }
        }

        // Render immediately if needed (for UI actions like ToggleHelp)
        if needs_render {
            self.render(tui)?;
        }

        Ok(())
    }

    /// Play the currently selected station
    fn play_station(&mut self) -> Result<()> {
        debug!("play_station called");
        if let Some(station) = self.stations.get(self.selected_station) {
            let station = station.clone();
            info!(station_id = %station.id, station_title = %station.title, "Starting playback");

            if let Some(sink) = self.sink.clone() {
                self.active_station = Some(self.selected_station);
                let current_time = Instant::now();
                self.playback_start_time = Some(current_time);
                self.playback_start_time_for_underrun = Some(current_time);
                self.station_loading = true;

                if let Some(pause_time) = self.last_pause_time.take() {
                    if let Some(start) = self.playback_start_time {
                        self.total_played += pause_time.duration_since(start);
                    }
                }

                self.abort_playback_task();

                // Stop any existing playback
                if let Ok(locked_sink) = sink.lock() {
                    locked_sink.stop();
                }

                self.add_history_message(
                    crate::i18n::t("starting-playback").replace("{$station}", &station.title),
                    MessageType::System,
                );
                self.add_history_message(
                    crate::i18n::t("connecting-to-stream"),
                    MessageType::System,
                );

                let log_tx = self.log_tx.clone();
                let metadata_tx = self.metadata_tx.clone();
                let volume = self.volume;
                let action_tx = self.action_tx.clone();
                let stream_config = audio::stream::StreamConfig::from_app_config(&self.config);

                let handle = audio::start_playback(
                    station.clone(),
                    sink,
                    metadata_tx,
                    log_tx,
                    action_tx,
                    volume,
                    stream_config,
                );
                self.audio_manager.set_handle(handle);
                self.audio_manager.set_current_station(station.id.clone());

                self.playback_state = PlaybackState::Playing;

                // Sync state to components
                let _ = self
                    .action_tx
                    .send(Action::SetActiveStation(self.active_station));
                let _ = self
                    .action_tx
                    .send(Action::SetPlaybackState(self.playback_state.clone()));
            }
        }
        Ok(())
    }

    /// Stop playback
    fn stop_playback(&mut self) {
        debug!("stop_playback called");
        let old_state = self.playback_state.clone();
        self.abort_playback_task();
        if let Some(ref sink) = self.sink {
            if let Ok(sink) = sink.lock() {
                match self.playback_state {
                    PlaybackState::Playing => {
                        sink.stop();
                        self.playback_state = PlaybackState::Stopped;
                        if let Some(start) = self.playback_start_time.take() {
                            self.total_played += start.elapsed();
                        }
                        self.last_pause_time = None;
                    }
                    PlaybackState::Paused => {
                        sink.stop();
                        self.playback_state = PlaybackState::Stopped;
                        self.last_pause_time = None;
                    }
                    PlaybackState::Stopped => {}
                }
            }
        }
        self.restart_attempts = 0;
        self.last_restart_time = None;

        // Sync state to components if it changed
        if old_state != PlaybackState::Stopped {
            let _ = self
                .action_tx
                .send(Action::SetPlaybackState(self.playback_state.clone()));
        }
    }

    /// Toggle pause/resume
    fn toggle_pause(&mut self) -> Result<()> {
        match self.playback_state {
            PlaybackState::Playing => {
                self.pause_playback();
            }
            PlaybackState::Paused => {
                self.resume_playback()?;
            }
            PlaybackState::Stopped => {
                self.play_station()?;
            }
        }
        Ok(())
    }

    /// Pause playback
    fn pause_playback(&mut self) {
        debug!("pause_playback called");
        if let Some(ref sink) = self.sink {
            if let Ok(sink) = sink.lock() {
                if matches!(self.playback_state, PlaybackState::Playing) {
                    sink.pause();
                    self.playback_state = PlaybackState::Paused;
                    if let Some(start) = self.playback_start_time.take() {
                        self.total_played += start.elapsed();
                    }
                    self.last_pause_time = Some(Instant::now());

                    // Sync state to components
                    let _ = self
                        .action_tx
                        .send(Action::SetPlaybackState(self.playback_state.clone()));
                }
            }
        }
        self.restart_attempts = 0;
        self.last_restart_time = None;
    }

    /// Resume playback
    fn resume_playback(&mut self) -> Result<()> {
        debug!("resume_playback called");
        if matches!(self.playback_state, PlaybackState::Paused) {
            if let Some(ref sink) = self.sink {
                if let Ok(sink) = sink.lock() {
                    sink.play();
                    self.playback_state = PlaybackState::Playing;
                    self.playback_start_time = Some(Instant::now());
                    self.last_pause_time = None;

                    // Sync state to components
                    let _ = self
                        .action_tx
                        .send(Action::SetPlaybackState(self.playback_state.clone()));
                }
            }
        } else if matches!(self.playback_state, PlaybackState::Stopped) {
            self.play_station()?;
        }
        Ok(())
    }

    /// Increase volume
    fn volume_up(&mut self) {
        self.volume = (self.volume + 0.05).min(2.0);
        if let Some(ref sink) = self.sink {
            if let Ok(sink) = sink.lock() {
                sink.set_volume(self.volume);
            }
        }
    }

    /// Decrease volume
    fn volume_down(&mut self) {
        self.volume = (self.volume - 0.05).max(0.0);
        if let Some(ref sink) = self.sink {
            if let Ok(sink) = sink.lock() {
                sink.set_volume(self.volume);
            }
        }
    }

    /// Render the UI
    fn render(&mut self, tui: &mut Tui) -> Result<()> {
        tui.draw(|frame| {
            let layout = Self::calculate_layout(frame.area());

            // Render each component in its area
            for (i, component) in self.components.iter_mut().enumerate() {
                let area = match i {
                    COMPONENT_STATION_LIST => layout.left_panel,
                    COMPONENT_NOW_PLAYING => layout.right_top,
                    COMPONENT_HISTORY => layout.right_bottom,
                    COMPONENT_BOTTOM_CONTROLS => layout.bottom,
                    _ => continue, // Help renders on full screen
                };
                let _ = component.draw(frame, area);
            }

            // Render help on top (overlay) if visible
            if let Some(help_comp) = self.components.get_mut(COMPONENT_HELP) {
                let _ = help_comp.draw(frame, frame.area());
            }
        })?;
        Ok(())
    }

    /// Calculate layout rectangles
    fn calculate_layout(area: Rect) -> AppLayout {
        // Main vertical split: content area and bottom controls
        let app_layout = RatatuiLayout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Fill(1), Constraint::Length(2)].as_ref())
            .split(area);

        // Horizontal split: station list and playback/history
        let chunks = RatatuiLayout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)].as_ref())
            .split(app_layout[0]);

        // Vertical split of right panel: now playing and history
        let right_chunks = RatatuiLayout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(10), Constraint::Fill(1)].as_ref())
            .split(chunks[1]);

        AppLayout {
            bottom: app_layout[1],
            left_panel: chunks[0],
            right_top: right_chunks[0],
            right_bottom: right_chunks[1],
        }
    }

    /// Send an action to be processed
    #[allow(dead_code)]
    pub fn send_action(&self, action: Action) -> Result<()> {
        self.action_tx.send(action)?;
        Ok(())
    }
}

/// Layout areas for the application
#[derive(Debug, Clone)]
pub struct AppLayout {
    pub bottom: Rect,
    pub left_panel: Rect,
    pub right_top: Rect,
    pub right_bottom: Rect,
}
