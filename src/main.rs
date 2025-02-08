 use crossterm::{
      event::{self, Event, KeyCode},
      execute,
      terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen,
  LeaveAlternateScreen},
  };

  use ratatui::text::{Line, Span};

  use ratatui::{
      backend::CrosstermBackend,
      layout::{Constraint, Direction, Layout},
      style::{Color, Style},
      widgets::{Block, Borders, List, ListItem, Paragraph, ListState},
      Terminal,
  };
  use std::{
      io,
      sync::{Arc, Mutex},
      time::{Duration, Instant}
  };
  use rodio::{Decoder, OutputStream, Sink};

  mod station;
  use crate::station::Station;

  struct App {
      stations: Vec<Station>,
      selected_station: ListState,
      playback_state: PlaybackState,
      history: Vec<String>,
      should_quit: bool,
      sink: Option<Arc<Mutex<Sink>>>,
  }

  enum PlaybackState {
      Playing,
      Paused,
      Stopped,
  }

  #[tokio::main]
  async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
      use ratatui::widgets::ListState;
      // Setup terminal
      enable_raw_mode()?;
      let mut stdout = io::stdout();
      execute!(stdout, EnterAlternateScreen)?;
      let backend = CrosstermBackend::new(stdout);
      let mut terminal = Terminal::new(backend)?;

      // Fetch stations
      let stations = Station::fetch_all().await?;

      // Create app state
      let (_stream, stream_handle) = OutputStream::try_default()?;
      let sink = Sink::try_new(&stream_handle)?;

      let mut app = App {
          stations,
          selected_station: ListState::default(),
          playback_state: PlaybackState::Stopped,
          history: Vec::new(),
          should_quit: false,
          sink: Some(Arc::new(Mutex::new(sink))),
      };

      // Main event loop
      let tick_rate = Duration::from_millis(250);
      let last_tick = Instant::now();
      loop {
          terminal.draw(|f| ui(f, &mut app))?;

          let timeout = tick_rate
              .checked_sub(last_tick.elapsed())
              .unwrap_or_else(|| Duration::from_secs(0));

          if event::poll(timeout)? {
              if let Event::Key(key) = event::read()? {
                  match key.code {
                      KeyCode::Char('q') => app.should_quit = true,
                      KeyCode::Char('p') => {
                          if let Some(index) = app.selected_station.selected() {
                              if let Some(station) = app.stations.get(index) {
                                  if let Some(sink) = &app.sink {
                                      let sink = sink.lock().unwrap();
                                      sink.stop();

                                      // Start playing the station
                                      let response = reqwest::get(&station.url).await?;
                                      let bytes = response.bytes().await?;
                                      let cursor = std::io::Cursor::new(bytes);
                                      let source = Decoder::new(cursor)?;
                                      sink.append(source);

                                      app.playback_state = PlaybackState::Playing;
                                  }
                              }
                          }
                      }
                      KeyCode::Char('s') => {
                          if let Some(sink) = &app.sink {
                              let sink = sink.lock().unwrap();
                              sink.stop();
                              app.playback_state = PlaybackState::Stopped;
                          }
                      }
                      KeyCode::Up => {
                          if let Some(selected) = app.selected_station.selected() {
                              if selected > 0 {
                                  app.selected_station.select(Some(selected - 1));
                              }
                          } else if !app.stations.is_empty() {
                              app.selected_station.select(Some(0));
                          }
                      }
                      KeyCode::Down => {
                          if let Some(selected) = app.selected_station.selected() {
                              if selected < app.stations.len() - 1 {
                                  app.selected_station.select(Some(selected + 1));
                              }
                          } else if !app.stations.is_empty() {
                              app.selected_station.select(Some(0));
                          }
                      }
                      _ => {}
                  }
              }
          }

          if app.should_quit {
              break;
          }
      }

      // Cleanup terminal and audio
      if let Some(sink) = app.sink {
          let sink = sink.lock().unwrap();
          sink.stop();
      }

      disable_raw_mode()?;
      execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
      terminal.show_cursor()?;

      Ok(())
  }

  fn ui<B: ratatui::backend::Backend>(f: &mut ratatui::Frame<B>, app: &mut App) {
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
          .block(Block::default().borders(Borders::ALL).title("Stations"))
          .highlight_style(Style::default().bg(Color::Blue));

      f.render_stateful_widget(stations_list, chunks[0], &mut app.selected_station);

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
      let now_playing = if let Some(index) = app.selected_station.selected() {
          if let Some(station) = app.stations.get(index) {
              Paragraph::new(vec![
                  Line::from(vec![
                      Span::styled("Title: ", Style::default().fg(Color::Yellow)),
                      Span::raw(&station.title),
                  ]),
                  Line::from(vec![
                      Span::styled("Genre: ", Style::default().fg(Color::Yellow)),
                      Span::raw(&station.genre),
                  ]),
                  Line::from(vec![
                      Span::styled("DJ: ", Style::default().fg(Color::Yellow)),
                      Span::raw(&station.dj),
                  ]),
                  Line::from(vec![
                      Span::styled("Now Playing: ", Style::default().fg(Color::Yellow)),
                      Span::raw(&station.last_playing),
                  ]),
                  Line::from(""),
                  Line::from(Span::raw(&station.description)),
              ])
              .wrap(ratatui::widgets::Wrap { trim: true })
          } else {
              Paragraph::new(vec![Line::from("No station selected")])
          }
      } else {
          Paragraph::new(vec![Line::from("No station selected")])
      }
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
