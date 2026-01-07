//! Event types for the terminal user interface
//!
//! Events represent raw input from the terminal and system events.

#![allow(dead_code)]

use crossterm::event::{KeyEvent, MouseEvent};

#[derive(Clone, Debug)]
pub enum Event {
    /// Terminal initialized
    Init,
    /// Quit requested
    Quit,
    /// Error occurred
    Error,
    /// Event stream closed
    Closed,
    /// Tick event (periodic timer)
    Tick,
    /// Render event (frame timer)
    Render,
    /// Terminal gained focus
    FocusGained,
    /// Terminal lost focus
    FocusLost,
    /// Paste event (text pasted into terminal)
    Paste(String),
    /// Key event
    Key(KeyEvent),
    /// Mouse event
    Mouse(MouseEvent),
    /// Terminal resize
    Resize(u16, u16),
}
