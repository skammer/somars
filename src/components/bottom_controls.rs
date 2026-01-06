//! Bottom controls bar component
//!
//! Displays keyboard shortcuts and debug information at the bottom of the screen.

use crate::{
    action::Action,
    components,
    i18n::t,
};

use components::Component;
use color_eyre::eyre::Result;
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Padding, Paragraph},
    Frame,
};
use tokio::sync::mpsc::UnboundedSender;

/// Bottom controls bar component
pub struct BottomControls {
    /// Log level (affects debug display)
    log_level: u8,
    /// Audio sink length (for debug display)
    sink_len: usize,
    /// Action sender
    action_tx: Option<UnboundedSender<Action>>,
}

impl BottomControls {
    /// Create a new bottom controls component
    pub fn new() -> Self {
        Self {
            log_level: 1,
            sink_len: 0,
            action_tx: None,
        }
    }

    /// Set the log level
    pub fn set_log_level(&mut self, level: u8) {
        self.log_level = level;
    }

    /// Set the sink length (for debug display)
    pub fn set_sink_len(&mut self, len: usize) {
        self.sink_len = len;
    }
}

impl Component for BottomControls {
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> Result<()> {
        self.action_tx = Some(tx);
        Ok(())
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        // BottomControls doesn't need to react to actions
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        // Build control key spans
        let mut bottom_controls_spans = vec![
            Span::styled(
                "q",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::raw(format!(":{} ", t("controls-quit"))),
            Span::styled(
                "↵",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::raw(format!(":{} ", t("controls-play"))),
            Span::styled(
                "Space",
                Style::default()
                    .fg(Color::Blue)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::raw(format!(":{}/{} ", t("controls-stop"), t("controls-start"))),
            Span::styled(
                "+/-",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::raw(format!(":{} ", t("controls-volume"))),
            Span::styled(
                "?",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::raw(format!(":{} ", t("controls-help"))),
        ];

        // Add debug info if log level is high
        if self.log_level > 1 {
            bottom_controls_spans.extend(vec![
                Span::raw("  "),
                Span::styled(
                    format!("Sink: {}", self.sink_len),
                    Style::default().fg(Color::Cyan),
                ),
            ]);
        }

        let bottom_controls = Line::from(bottom_controls_spans);
        let bottom_bar = Paragraph::new(bottom_controls)
            .alignment(ratatui::layout::Alignment::Left)
            .block(Block::default().padding(Padding::new(1, 1, 1, 0)));

        frame.render_widget(bottom_bar, area);
        Ok(())
    }
}

impl Default for BottomControls {
    fn default() -> Self {
        Self::new()
    }
}
