//! UI Components for the somars application
//!
//! Each component implements the Component trait and handles
//! a specific part of the user interface.

use crossterm::event::{KeyEvent, MouseEvent};
use color_eyre::eyre::Result;
use ratatui::{
    Frame,
    layout::{Rect, Size},
};
use tokio::sync::mpsc::UnboundedSender;

use crate::{action::Action, config::Config, event::Event};

pub mod station_list;
pub mod now_playing;
pub mod history;
pub mod help;
pub mod bottom_controls;

pub use station_list::StationList;
pub use now_playing::NowPlaying;
pub use history::History;
pub use help::Help;
pub use bottom_controls::BottomControls;

/// Component trait that represents a visual and interactive element of the user interface.
///
/// Implementors of this trait can be registered with the main application loop and will be able to
/// receive events, update state, and be rendered on the screen.
pub trait Component {
    /// Register an action handler that can send actions for processing if necessary.
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> Result<()> {
        let _ = tx; // to appease clippy
        Ok(())
    }

    /// Register a configuration handler that provides configuration settings if necessary.
    fn register_config_handler(&mut self, config: Config) -> Result<()> {
        let _ = config; // to appease clippy
        Ok(())
    }

    /// Initialize the component with a specified area if necessary.
    fn init(&mut self, area: Size) -> Result<()> {
        let _ = area; // to appease clippy
        Ok(())
    }

    /// Handle incoming events and produce actions if necessary.
    fn handle_events(&mut self, event: Option<Event>) -> Result<Option<Action>> {
        let action = match event {
            Some(Event::Key(key_event)) => self.handle_key_event(key_event)?,
            Some(Event::Mouse(mouse_event)) => self.handle_mouse_event(mouse_event)?,
            _ => None,
        };
        Ok(action)
    }

    /// Handle key events and produce actions if necessary.
    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        let _ = key; // to appease clippy
        Ok(None)
    }

    /// Handle mouse events and produce actions if necessary.
    fn handle_mouse_event(&mut self, mouse: MouseEvent) -> Result<Option<Action>> {
        let _ = mouse; // to appease clippy
        Ok(None)
    }

    /// Update the state of the component based on a received action. (REQUIRED)
    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        let _ = action; // to appease clippy
        Ok(None)
    }

    /// Render the component on the screen. (REQUIRED)
    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()>;
}

/// Layout areas for components
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct LayoutAreas {
    pub left_panel: ratatui::layout::Rect,
    pub right_top: ratatui::layout::Rect,
    pub right_bottom: ratatui::layout::Rect,
    pub bottom: ratatui::layout::Rect,
}
