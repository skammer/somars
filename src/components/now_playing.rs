//! Now playing component
//!
//! Displays information about the currently selected station and playback state.

use crate::{
    action::Action,
    components,
    i18n::t,
    PlaybackState,
    station::Station,
};

use components::Component;
use color_eyre::eyre::Result;
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use tokio::sync::mpsc::UnboundedSender;

/// Now playing component
pub struct NowPlaying {
    /// Currently selected station
    selected_station: Option<Station>,
    /// Playback state
    playback_state: PlaybackState,
    /// Current volume (0.0 to 2.0)
    volume: f32,
    /// Playback animation frames
    playback_frames: Vec<&'static str>,
    /// Current playback frame index
    playback_frame_index: usize,
    /// Action sender
    action_tx: Option<UnboundedSender<Action>>,
}

impl NowPlaying {
    /// Create a new now playing component
    pub fn new() -> Self {
        Self {
            selected_station: None,
            playback_state: PlaybackState::Stopped,
            volume: 1.0,
            playback_frames: vec!["▮▯▯▯", "▮▮▯▯", "▮▮▮▯", "▮▮▮▮"],
            playback_frame_index: 0,
            action_tx: None,
        }
    }

    /// Set the selected station
    pub fn set_selected_station(&mut self, station: Option<Station>) {
        self.selected_station = station;
    }

    /// Set the playback state
    pub fn set_playback_state(&mut self, state: PlaybackState) {
        self.playback_state = state;
    }

    /// Set the volume
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 2.0);
    }

    /// Get the volume
    #[allow(dead_code)]
    pub fn volume(&self) -> f32 {
        self.volume
    }

    /// Advance the playback animation frame
    fn advance_frame(&mut self) {
        self.playback_frame_index = (self.playback_frame_index + 1) % self.playback_frames.len();
    }
}

impl Component for NowPlaying {
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> Result<()> {
        self.action_tx = Some(tx);
        Ok(())
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        match action {
            Action::Tick => {
                self.advance_frame();
            }
            Action::SetPlaybackState(state) => {
                self.set_playback_state(state);
            }
            Action::SetVolume(level) => {
                self.set_volume(level);
            }
            Action::SetSelectedStation(station) => {
                self.set_selected_station(station);
            }
            _ => {}
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let content = if let Some(station) = &self.selected_station {
            vec![
                Line::from(vec![
                    Span::styled(
                        format!("{}: ", t("station-id")),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::raw(&station.id),
                ]),
                Line::from(vec![
                    Span::styled(
                        format!("{}: ", t("station-title")),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::raw(&station.title),
                ]),
                Line::from(vec![
                    Span::styled(
                        format!("{}: ", t("station-genre")),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::raw(&station.genre),
                ]),
                Line::from(vec![
                    Span::styled(
                        format!("{}: ", t("station-dj")),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::raw(&station.dj),
                ]),
                Line::from(""),
                Line::from(Span::raw(&station.description)),
                Line::from(""),
            ]
        } else {
            vec![Line::from(t("no-station-selected"))]
        };

        let playback_state_str = match self.playback_state {
            PlaybackState::Playing => t("playing"),
            PlaybackState::Paused => t("paused"),
            PlaybackState::Stopped => t("stopped"),
        };

        let playback_state_color = match self.playback_state {
            PlaybackState::Playing => Color::Green,
            PlaybackState::Paused => Color::Blue,
            PlaybackState::Stopped => Color::Red,
        };

        let playback_animation = if matches!(self.playback_state, PlaybackState::Playing) {
            Span::styled(
                format!(" {}", self.playback_frames[self.playback_frame_index]),
                Style::default().fg(Color::Green),
            )
        } else {
            Span::raw("")
        };

        let now_playing = Paragraph::new(content)
            .wrap(ratatui::widgets::Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(
                        Line::from(vec![Span::styled(
                            format!(
                                " ♪ {} v{}",
                                env!("CARGO_PKG_NAME"),
                                env!("CARGO_PKG_VERSION")
                            ),
                            Style::default().add_modifier(ratatui::style::Modifier::BOLD),
                        )])
                        .right_aligned(),
                    )
                    .title(Line::from(vec![
                        Span::raw("["),
                        Span::styled(
                            playback_state_str,
                            Style::default().fg(playback_state_color),
                        ),
                        Span::raw("]"),
                        playback_animation,
                    ]))
                    .title_bottom(
                        Line::from(format!("[{}: {:.0}%]", t("volume"), self.volume * 100.0)).centered(),
                    )
                    .padding(ratatui::widgets::Padding::new(1, 1, 0, 0)),
            );

        frame.render_widget(now_playing, area);
        Ok(())
    }
}

impl Default for NowPlaying {
    fn default() -> Self {
        Self::new()
    }
}
