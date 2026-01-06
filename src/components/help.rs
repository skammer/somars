//! Help popup component
//!
//! Displays keyboard shortcuts and usage information.

use crate::{
    action::Action,
    components,
    i18n::t,
};

use components::Component;
use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Flex, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, BorderType, Paragraph, Padding, Wrap},
    Frame,
};
use tokio::sync::mpsc::UnboundedSender;
use tracing::info;

/// Help popup component
pub struct Help {
    /// Whether the help popup is visible
    visible: bool,
    /// Action sender
    action_tx: Option<UnboundedSender<Action>>,
}

impl Help {
    /// Create a new help component
    pub fn new() -> Self {
        Self {
            visible: false,
            action_tx: None,
        }
    }

    /// Toggle the help visibility
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        info!("Help toggled, visible={}", self.visible);
    }

    /// Show the help popup
    pub fn show(&mut self) {
        self.visible = true;
        info!("Help shown");
    }

    /// Hide the help popup
    pub fn hide(&mut self) {
        self.visible = false;
        info!("Help hidden");
    }

    /// Check if the help is visible
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Calculate the popup area
    fn popup_area(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
        let vertical = Layout::vertical([Constraint::Percentage(percent_y)]).flex(Flex::Center);
        let horizontal = Layout::horizontal([Constraint::Percentage(percent_x)]).flex(Flex::Center);
        let [area] = vertical.areas(area);
        let [area] = horizontal.areas(area);
        area
    }

    /// Build the help text content
    fn build_help_text() -> Vec<Line<'static>> {
        vec![
            Line::from(vec![Span::styled(
                format!("{} - {}", env!("CARGO_PKG_NAME"), t("app-description")),
                ratatui::style::Style::default().add_modifier(ratatui::style::Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(t("help-keyboard")),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    "↵ (Enter)",
                    ratatui::style::Style::default().add_modifier(ratatui::style::Modifier::BOLD),
                ),
                Span::raw(format!(" - {}", t("help-enter"))),
            ]),
            Line::from(vec![
                Span::styled(
                    "Space",
                    ratatui::style::Style::default().add_modifier(ratatui::style::Modifier::BOLD),
                ),
                Span::raw(format!(
                    " - {} ({}/{})",
                    t("help-space"),
                    t("controls-pause"),
                    t("controls-stop")
                )),
            ]),
            Line::from(vec![
                Span::styled(
                    "+/-",
                    ratatui::style::Style::default().add_modifier(ratatui::style::Modifier::BOLD),
                ),
                Span::raw(format!(" - {}", t("help-volume"))),
            ]),
            Line::from(vec![
                Span::styled(
                    "↑/↓",
                    ratatui::style::Style::default().add_modifier(ratatui::style::Modifier::BOLD),
                ),
                Span::raw(format!(" - {}", t("help-arrows"))),
            ]),
            Line::from(vec![
                Span::styled(
                    "q",
                    ratatui::style::Style::default().add_modifier(ratatui::style::Modifier::BOLD),
                ),
                Span::raw(format!(" - {}", t("help-quit"))),
            ]),
            Line::from(vec![
                Span::styled(
                    "?",
                    ratatui::style::Style::default().add_modifier(ratatui::style::Modifier::BOLD),
                ),
                Span::raw(format!(" - {}", t("help-toggle-help"))),
            ]),
            Line::from(""),
            Line::from(t("help-cli")),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    "--log-level <1|2>",
                    ratatui::style::Style::default().add_modifier(ratatui::style::Modifier::BOLD),
                ),
                Span::raw(format!(" - {}", t("help-log-level"))),
            ]),
            Line::from(vec![
                Span::styled(
                    "--station <ID>",
                    ratatui::style::Style::default().add_modifier(ratatui::style::Modifier::BOLD),
                ),
                Span::raw(format!(" - {}", t("help-station"))),
            ]),
            Line::from(""),
            Line::from(t("help-homepage")),
            Line::from(vec![Span::styled(
                env!("CARGO_PKG_HOMEPAGE"),
                ratatui::style::Style::default().fg(ratatui::style::Color::Blue),
            )]),
        ]
    }
}

impl Component for Help {
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> Result<()> {
        self.action_tx = Some(tx);
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        match key.code {
            KeyCode::Char('?') => {
                // Toggle help visibility
                Ok(Some(Action::ToggleHelp))
            }
            KeyCode::Esc => {
                // Close help if visible
                if self.visible {
                    Ok(Some(Action::ToggleHelp))
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        match action {
            Action::ToggleHelp => {
                self.toggle();
            }
            Action::Help => {
                self.show();
            }
            _ => {}
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        if !self.visible {
            return Ok(());
        }

        info!("Drawing help overlay (visible={})", self.visible);

        let help_text = Self::build_help_text();
        let popup_area = Self::popup_area(area, 60, 60);
        let help_widget = Paragraph::new(help_text)
            .block(
                Block::default()
                    .title(t("help-title"))
                    .title_bottom(
                        Line::from(format!(
                            "{} v{}",
                            env!("CARGO_PKG_NAME"),
                            env!("CARGO_PKG_VERSION")
                        ))
                        .right_aligned(),
                    )
                    .borders(Borders::ALL)
                    .border_type(BorderType::Double)
                    .padding(Padding::new(1, 1, 0, 0)),
            )
            .alignment(ratatui::layout::Alignment::Left)
            .wrap(Wrap { trim: true });

        frame.render_widget(ratatui::widgets::Clear, popup_area);
        frame.render_widget(help_widget, popup_area);
        Ok(())
    }
}

impl Default for Help {
    fn default() -> Self {
        Self::new()
    }
}
