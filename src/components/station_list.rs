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
    /// Scroll offset to manage visible portion of list
    scroll_offset: usize,
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
            scroll_offset: 0,
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
    #[allow(dead_code)]
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
            KeyCode::Down => Ok(self.move_down()),
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
            // Calculate how many items can be displayed in the available area
            // Account for top and bottom borders (border and title share the same line)
            let available_height = area.height.saturating_sub(2) as usize; // Account for top and bottom borders

            // Adjust scroll offset to keep selected item visible
            if self.selected_index < self.scroll_offset {
                // Selected item is above visible area, scroll up to it
                self.scroll_offset = self.selected_index;
            } else if available_height > 0 && self.selected_index >= self.scroll_offset + available_height {
                // Selected item is below visible area, scroll down to it
                self.scroll_offset = self.selected_index.saturating_sub(available_height - 1);
            }

            // Create a temporary ListState for rendering
            let mut list_state = ListState::default();
            // Calculate the relative position of the selected item within the visible window
            let relative_selected = if self.selected_index >= self.scroll_offset {
                self.selected_index - self.scroll_offset
            } else {
                0
            };
            list_state.select(Some(relative_selected));

            let active_station = self.active_station;

            let selected_pos = self.selected_index + 1;
            let total_stations = self.stations.len();

            // Only take the visible stations based on scroll offset
            let visible_stations: Vec<&Station> = self.stations
                .iter()
                .skip(self.scroll_offset)
                .take(available_height)
                .collect();

            let station_items: Vec<ListItem> = visible_stations
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    // Calculate the actual index in the full list
                    let actual_index = self.scroll_offset + i;
                    let style = if Some(actual_index) == active_station {
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
