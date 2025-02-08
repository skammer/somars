 use crossterm::{
      event::{self, Event, KeyCode},
      execute,
      terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen,
  LeaveAlternateScreen},
  };

  use ratatui::text::{Line, Span, Text};

  use ratatui::{
      backend::CrosstermBackend,
      layout::{Constraint, Direction, Layout},
      style::{Color, Style},
      widgets::{Block, Borders, List, ListItem, Paragraph},
      Terminal,
  };
  use rodio::{Decoder, OutputStream, Sink};
  use serde::Deserialize;
  use std::{
      io,
      sync::mpsc,
      thread,
      time::{Duration, Instant},
  };

  #[derive(Debug, Deserialize)]
  struct Station {
      title: String,
      description: String,
      dj: String,
      url: String,
  }

  struct App {
      stations: Vec<Station>,
      current_station: Option<usize>,
      playback_state: PlaybackState,
      history: Vec<String>,
      should_quit: bool,
  }

  enum PlaybackState {
      Playing,
      Paused,
      Stopped,
  }

  fn main() -> Result<(), Box<dyn std::error::Error>> {
      // Setup terminal
      enable_raw_mode()?;
      let mut stdout = io::stdout();
      execute!(stdout, EnterAlternateScreen)?;
      let backend = CrosstermBackend::new(stdout);
      let mut terminal = Terminal::new(backend)?;

      // Create app state
      let mut app = App {
          stations: vec![], // Will be populated from SomaFM API
          current_station: None,
          playback_state: PlaybackState::Stopped,
          history: Vec::new(),
          should_quit: false,
      };

      // Main event loop
      let tick_rate = Duration::from_millis(250);
      let last_tick = Instant::now();
      loop {
          terminal.draw(|f| ui(f, &app))?;

          let timeout = tick_rate
              .checked_sub(last_tick.elapsed())
              .unwrap_or_else(|| Duration::from_secs(0));

          if event::poll(timeout)? {
              if let Event::Key(key) = event::read()? {
                  match key.code {
                      KeyCode::Char('q') => app.should_quit = true,
                      _ => {}
                  }
              }
          }

          if app.should_quit {
              break;
          }
      }

      // Cleanup terminal
      disable_raw_mode()?;
      execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
      terminal.show_cursor()?;

      Ok(())
  }

  fn ui<B: ratatui::backend::Backend>(f: &mut ratatui::Frame<B>, app: &App) {
      let chunks = Layout::default()
          .direction(Direction::Horizontal)
          .constraints([Constraint::Percentage(30), Constraint::Percentage(70)].as_ref())
          .split(f.size());

      // Left panel - Station list
      let station_items: Vec<ListItem> = app
          .stations
          .iter()
          .map(|s| ListItem::new(s.title.as_str()))
          .collect();
      let stations_list = List::new(station_items)
          .block(Block::default().borders(Borders::ALL).title("Stations"));
      f.render_widget(stations_list, chunks[0]);

      // Right panel - Playback controls and info
      let right_chunks = Layout::default()
          .direction(Direction::Vertical)
          .constraints(
              [
                  Constraint::Length(3), // Controls
                  Constraint::Percentage(40), // Now Playing
                  Constraint::Percentage(60), // History
              ]
              .as_ref(),
          )
          .split(chunks[1]);

      // Controls
      let controls = Paragraph::new(vec![
          Line::from(vec![
              Span::styled("[P] Play", Style::default().fg(Color::Green)),
              Span::raw(" "),
              Span::styled("[S] Stop", Style::default().fg(Color::Red)),
              Span::raw(" "),
              Span::styled("[Q] Quit", Style::default().fg(Color::Yellow)),
          ]),
      ])
      .block(Block::default().borders(Borders::ALL).title("Controls"));
      f.render_widget(controls, right_chunks[0]);

      // Now Playing
      let now_playing = Paragraph::new(vec![
          Line::from(Span::styled("Now Playing: ...", Style::default()))
      ])
      .block(Block::default().borders(Borders::ALL).title("Now Playing"));
      f.render_widget(now_playing, right_chunks[1]);

      // History
      let history_items: Vec<ListItem> = app
           .history
           .iter()
           .map(|s| ListItem::new(Line::from(Span::styled(s, Style::default()))))
           .collect();

      let history_list = List::new(history_items)
          .block(Block::default().borders(Borders::ALL).title("History"));
      f.render_widget(history_list, right_chunks[2]);
  }
