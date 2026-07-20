//! History log component
//!
//! Displays the history of events and messages with text wrapping and caching.

use crate::{
    action::Action, components, i18n::t, utils::format_duration, HistoryMessage, MessageType,
    PlaybackState,
};

use color_eyre::eyre::Result;
use components::Component;
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListDirection, ListItem, ListState},
    Frame,
};
use std::collections::{HashMap, VecDeque};
use tokio::sync::mpsc::UnboundedSender;

/// History component
pub struct History {
    /// History messages
    messages: VecDeque<HistoryMessage>,
    /// Scroll state
    scroll_state: ListState,
    /// Log level filter
    log_level: u8,
    /// Cache for wrapped text
    wrapped_cache: HashMap<usize, Vec<String>>,
    /// Whether the cache is valid
    cache_valid: bool,
    /// Last known width
    last_width: u16,
    /// Playback state
    playback_state: PlaybackState,
    /// Total played time
    total_played: std::time::Duration,
    /// Playback start time
    playback_start_time: Option<std::time::Instant>,
    /// Action sender
    action_tx: Option<UnboundedSender<Action>>,
}

impl History {
    /// Create a new history component
    pub fn new() -> Self {
        Self {
            messages: VecDeque::with_capacity(1000),
            scroll_state: ListState::default(),
            log_level: 1,
            wrapped_cache: HashMap::new(),
            cache_valid: false,
            last_width: 0,
            playback_state: PlaybackState::Stopped,
            total_played: std::time::Duration::default(),
            playback_start_time: None,
            action_tx: None,
        }
    }

    /// Add a message to the history
    pub fn add_message(&mut self, message: HistoryMessage) {
        let preserve_position =
            self.scroll_state.selected().is_some() && self.message_is_visible(&message);

        self.messages.push_back(message);
        self.cache_valid = false;

        while self.messages.len() > 1000 {
            self.messages.pop_front();
        }

        if preserve_position {
            if let Some(selected) = self.scroll_state.selected() {
                self.scroll_state.select(Some(selected + 1));
                *self.scroll_state.offset_mut() += 1;
            }
        }

        let visible_count = self.visible_messages().len();
        if visible_count == 0 {
            self.scroll_state = ListState::default();
        } else {
            if let Some(selected) = self.scroll_state.selected() {
                self.scroll_state
                    .select(Some(selected.min(visible_count - 1)));
            }
            *self.scroll_state.offset_mut() = self.scroll_state.offset().min(visible_count - 1);
        }
    }

    /// Clear all messages
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.messages.clear();
        self.cache_valid = false;
        self.scroll_state.select(Some(0));
    }

    /// Set the log level
    pub fn set_log_level(&mut self, level: u8) {
        self.log_level = level;
    }

    /// Set the playback state
    pub fn set_playback_state(&mut self, state: PlaybackState) {
        self.playback_state = state;
    }

    /// Set the total played time
    pub fn set_total_played(&mut self, duration: std::time::Duration) {
        self.total_played = duration;
    }

    /// Set the playback start time
    #[allow(dead_code)]
    pub fn set_playback_start_time(&mut self, time: Option<std::time::Instant>) {
        self.playback_start_time = time;
    }

    /// Get current played time
    fn current_played_time(&self) -> std::time::Duration {
        match self.playback_state {
            PlaybackState::Playing => {
                let base = self.total_played;
                if let Some(start) = self.playback_start_time {
                    base + start.elapsed()
                } else {
                    base
                }
            }
            _ => self.total_played,
        }
    }

    /// Set the playback start time to now
    pub fn start_tracking_play_time(&mut self) {
        self.playback_start_time = Some(std::time::Instant::now());
    }

    /// Stop tracking play time and accumulate the elapsed time
    pub fn stop_tracking_play_time(&mut self) {
        if let Some(start_time) = self.playback_start_time {
            self.total_played += start_time.elapsed();
            self.playback_start_time = None;
        }
    }

    /// Scroll toward newer messages.
    fn scroll_up(&mut self) {
        let visible_count = self.visible_messages().len();
        if visible_count == 0 {
            return;
        }
        if self.scroll_state.selected().is_none() {
            self.scroll_state.select(Some(0));
        } else {
            let i = self.scroll_state.selected().unwrap_or(0);
            if i > 0 {
                self.scroll_state.select(Some(i - 1));
            }
        }
    }

    /// Scroll toward older messages.
    fn scroll_down(&mut self) {
        let visible_count = self.visible_messages().len();
        if visible_count == 0 {
            return;
        }
        if self.scroll_state.selected().is_none() {
            self.scroll_state
                .select(Some(usize::from(visible_count > 1)));
        } else {
            let i = self.scroll_state.selected().unwrap_or(0);
            if i < visible_count - 1 {
                self.scroll_state.select(Some(i + 1));
            }
        }
    }

    /// Invalidate the cache
    #[allow(dead_code)]
    fn invalidate_cache(&mut self) {
        self.cache_valid = false;
    }

    /// Ensure cache is valid
    fn ensure_cache_valid(&mut self, width: u16) {
        if self.cache_valid && self.last_width == width {
            return;
        }

        self.wrapped_cache.clear();
        let message_width = width.saturating_sub(10) as usize;

        for (idx, msg) in self.messages.iter().enumerate() {
            let wrapped: Vec<String> = textwrap::wrap(&msg.message, message_width)
                .into_iter()
                .map(|s| s.to_string())
                .collect();
            self.wrapped_cache.insert(idx, wrapped);
        }

        self.cache_valid = true;
        self.last_width = width;
    }

    fn message_is_visible(&self, message: &HistoryMessage) -> bool {
        self.log_level > 1
            || matches!(
                message.message_type,
                MessageType::Error | MessageType::Info | MessageType::Playback
            )
    }

    /// Return visible messages from newest to oldest.
    fn visible_messages(&self) -> Vec<(usize, HistoryMessage)> {
        self.messages
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, msg)| self.message_is_visible(msg))
            .map(|(idx, msg)| (idx, msg.clone()))
            .collect()
    }
}

impl Component for History {
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> Result<()> {
        self.action_tx = Some(tx);
        Ok(())
    }

    fn handle_key_event(&mut self, key: crossterm::event::KeyEvent) -> Result<Option<Action>> {
        use crossterm::event::KeyCode;

        match key.code {
            KeyCode::Char('j') => {
                self.scroll_down();
                Ok(None)
            }
            KeyCode::Char('k') => {
                self.scroll_up();
                Ok(None)
            }
            KeyCode::Esc => {
                self.scroll_state = ListState::default();
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        match action {
            Action::AddHistoryMessage(msg) => {
                self.add_message(msg);
            }
            Action::SetLogLevel(level) => {
                self.set_log_level(level);
            }
            Action::ScrollHistoryUp => {
                self.scroll_up();
            }
            Action::ScrollHistoryDown => {
                self.scroll_down();
            }
            Action::SetPlaybackState(state) => {
                self.set_playback_state(state);
            }
            Action::SetTotalPlayed(duration) => {
                self.set_total_played(duration);
            }
            Action::StartTrackingPlayTime => {
                self.start_tracking_play_time();
            }
            Action::StopTrackingPlayTime => {
                self.stop_tracking_play_time();
            }
            Action::SetVolume(_) => {
                // Volume changes don't affect history directly
            }
            _ => {}
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        // Ensure cache is valid
        self.ensure_cache_valid(area.width);

        let visible_messages = self.visible_messages();

        let history_items: Vec<ListItem> = visible_messages
            .iter()
            .map(|(idx, msg)| {
                let style = match msg.message_type {
                    MessageType::Error => Style::default().fg(Color::Red),
                    MessageType::Info => Style::default().fg(Color::Green),
                    MessageType::System => Style::default().fg(Color::Green),
                    MessageType::Background => Style::default().fg(Color::DarkGray),
                    MessageType::Playback => Style::default().fg(Color::White),
                };

                let timestamp_span = Span::styled(&msg.timestamp, style);

                // Get wrapped text from cache
                let wrapped_lines = self.wrapped_cache.get(idx).cloned().unwrap_or_default();

                // Create lines with proper alignment
                let mut lines = Vec::new();
                if let Some(first_line) = wrapped_lines.first() {
                    lines.push(Line::from(vec![
                        timestamp_span.clone(),
                        Span::styled("  ", style),
                        Span::styled(first_line.clone(), style),
                    ]));
                }

                for line in wrapped_lines.iter().skip(1) {
                    lines.push(Line::from(vec![
                        Span::styled("          ", style),
                        Span::styled(line.clone(), style),
                    ]));
                }

                ListItem::new(Text::from(lines))
            })
            .collect();

        let selected_pos = self.scroll_state.selected().unwrap_or(0) + 1;
        let total_history = visible_messages.len();

        let current_time = self.current_played_time();
        let time_str = format_duration(current_time);

        let history_list = List::new(history_items)
            .direction(ListDirection::TopToBottom)
            .highlight_style(Style::default().bg(Color::Blue))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(t("history"))
                    .title(Line::from("[jk Esc]").right_aligned())
                    .title_bottom(
                        Line::from(vec![Span::raw(format!(
                            "[{} / {}]",
                            selected_pos, total_history
                        ))])
                        .right_aligned(),
                    )
                    .title_bottom(
                        Line::from(vec![Span::raw(format!("[{}]", time_str))]).left_aligned(),
                    )
                    .padding(ratatui::widgets::Padding::new(1, 1, 0, 0)),
            );

        frame.render_stateful_widget(history_list, area, &mut self.scroll_state);
        Ok(())
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn message(text: &str, message_type: MessageType) -> HistoryMessage {
        HistoryMessage {
            message: text.to_string(),
            message_type,
            timestamp: "00:00:00".to_string(),
        }
    }

    #[test]
    fn shows_newest_visible_message_first() {
        let mut history = History::new();
        history.add_message(message("old", MessageType::Info));
        history.add_message(message("hidden", MessageType::Background));
        history.add_message(message("new", MessageType::Playback));

        let messages: Vec<_> = history
            .visible_messages()
            .into_iter()
            .map(|(_, message)| message.message)
            .collect();

        assert_eq!(messages, ["new", "old"]);
    }

    #[test]
    fn navigation_moves_down_to_older_and_up_to_newer() {
        let mut history = History::new();
        history.add_message(message("old", MessageType::Info));
        history.add_message(message("middle", MessageType::Info));
        history.add_message(message("new", MessageType::Info));

        history.scroll_down();
        assert_eq!(history.scroll_state.selected(), Some(1));
        history.scroll_down();
        assert_eq!(history.scroll_state.selected(), Some(2));
        history.scroll_up();
        assert_eq!(history.scroll_state.selected(), Some(1));
    }

    #[test]
    fn new_message_preserves_manually_scrolled_position() {
        let mut history = History::new();
        history.add_message(message("old", MessageType::Info));
        history.add_message(message("new", MessageType::Info));
        history.scroll_down();

        history.add_message(message("newest", MessageType::Info));

        assert_eq!(history.scroll_state.selected(), Some(2));
        assert_eq!(history.visible_messages()[2].1.message, "old");
    }

    #[test]
    fn escape_returns_to_latest_message() {
        let mut history = History::new();
        history.add_message(message("old", MessageType::Info));
        history.add_message(message("new", MessageType::Info));
        history.scroll_down();

        history
            .handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();

        assert_eq!(history.scroll_state, ListState::default());
    }
}
