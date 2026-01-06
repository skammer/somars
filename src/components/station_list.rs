//! Station list component
//!
//! Displays the list of available SomaFM stations with selection and loading states.

use crate::{
    action::Action,
    components,
    i18n::t,
    station::Station,
};

use components::Component;
use crossterm::event::KeyEvent;
use color_eyre::eyre::Result;
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};
use ratatui::widgets::ListState;
use tokio::sync::mpsc::UnboundedSender;

/// Station list component
pub struct StationList {
    /// List of stations
    stations: Vec<Station>,
    /// Selected station index (managed by App, not component)
    selected_index: usize,
    /// Currently active station index
    active_station: Option<usize>,
    /// Loading state
    loading: bool,
    /// Spinner frame index
    spinner_state: usize,
    /// Spinner frames
    spinner_frames: Vec<&'static str>,
    /// Action sender
    action_tx: Option<UnboundedSender<Action>>,
}

impl StationList {
    /// Create a new station list component
    pub fn new() -> Self {
        Self {
            stations: Vec::new(),
            selected_index: 0,
            active_station: None,
            loading: true,
            spinner_state: 0,
            spinner_frames: vec!["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
            action_tx: None,
        }
    }

    /// Update the stations list
    pub fn set_stations(&mut self, stations: Vec<Station>) {
        self.stations = stations;
    }

    /// Set the selected station index (called by App)
    pub fn set_selected_index(&mut self, index: usize) {
        self.selected_index = index;
    }

    /// Set the active station
    pub fn set_active_station(&mut self, index: Option<usize>) {
        self.active_station = index;
    }

    /// Set the loading state
    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
    }

    /// Get the loading state
    pub fn is_loading(&self) -> bool {
        self.loading
    }

    /// Move selection up
    fn move_up(&mut self) -> Option<Action> {
        // Don't modify state here, just return the action
        // The App will handle the state change
        Some(Action::StationUp)
    }

    /// Move selection down
    fn move_down(&mut self) -> Option<Action> {
        // Don't modify state here, just return the action
        // The App will handle the state change
        Some(Action::StationDown)
    }

    /// Render the loading indicator
    fn render_loading(&self, frame: &mut Frame, area: Rect) -> Result<()> {
        let loading_text = vec![Line::from(vec![
            Span::raw(self.spinner_frames[self.spinner_state]),
            Span::raw(format!(" {}", t("loading-stations"))),
        ])];
        let loading_para = Paragraph::new(loading_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(t("loading"))
                    .padding(ratatui::widgets::Padding::new(1, 1, 0, 0)),
            )
            .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(loading_para, area);
        Ok(())
    }

}

impl Component for StationList {
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> Result<()> {
        self.action_tx = Some(tx);
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        use crossterm::event::KeyCode;

        match key.code {
            KeyCode::Up => Ok(self.move_up()),
            KeyCode::Down | KeyCode::Char('j') => Ok(self.move_down()),
            KeyCode::Char('k') => Ok(self.move_up()),
            KeyCode::Enter => Ok(Some(Action::Play)),
            _ => Ok(None),
        }
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        match action {
            Action::UpdateStations(stations) => {
                self.set_stations(stations);
                self.set_loading(false);
            }
            Action::SelectStation(idx) => {
                if idx < self.stations.len() {
                    self.set_selected_index(idx);
                }
            }
            Action::SetActiveStation(idx) => {
                self.set_active_station(idx);
            }
            Action::Tick => {
                self.spinner_state = (self.spinner_state + 1) % self.spinner_frames.len();
            }
            _ => {}
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        if self.loading {
            self.render_loading(frame, area)?;
        } else {
            // Create a temporary ListState for rendering
            let mut list_state = ListState::default();
            list_state.select(Some(self.selected_index));

            let stations = self.stations.clone();
            let active_station = self.active_station;

            let selected_pos = self.selected_index + 1;
            let total_stations = stations.len();

            let station_items: Vec<ListItem> = stations
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let style = if Some(i) == active_station {
                        Style::default().add_modifier(ratatui::style::Modifier::UNDERLINED)
                    } else {
                        Style::default()
                    };
                    ListItem::new(Span::styled(s.title.as_str(), style))
                })
                .collect();

            let stations_list = List::new(station_items)
                .block(
                    Block::bordered()
                        .title(Line::from(t("stations")))
                        .title(Line::from("[↓↑]").right_aligned())
                        .title_bottom(
                            Line::from(format!("[{} / {}]", selected_pos, total_stations))
                                .right_aligned(),
                        )
                        .padding(ratatui::widgets::Padding::new(1, 1, 0, 0)),
                )
                .repeat_highlight_symbol(true)
                .highlight_style(Style::default().bg(Color::Blue));

            frame.render_stateful_widget(stations_list, area, &mut list_state);
        }
        Ok(())
    }
}

impl Default for StationList {
    fn default() -> Self {
        Self::new()
    }
}
