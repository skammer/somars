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
      loading: bool,
      spinner_state: usize,
      spinner_frames: Vec<&'static str>,
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
      execute!(stdout, EnterAlternateScreen, crossterm::terminal::Clear(crossterm::terminal::ClearType::All))?;
      let backend = CrosstermBackend::new(stdout);
      let mut terminal = Terminal::new(backend)?;
      terminal.clear()?;

      // Create app state
      let (_stream, stream_handle) = OutputStream::try_default()?;
      let sink = Sink::try_new(&stream_handle)?;

      // Create channels for logging
      let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(32);
      
      let mut selected_station = ListState::default();
      selected_station.select(Some(0));
      
      let mut app = App {
          stations: Vec::new(),
          selected_station,
          playback_state: PlaybackState::Stopped,
          history: Vec::new(),
          should_quit: false,
          sink: Some(Arc::new(Mutex::new(sink))),
          loading: true,
          spinner_state: 0,
          spinner_frames: vec!["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
      };

      // Spawn station fetching task
      let (tx, mut rx) = tokio::sync::mpsc::channel(1);
      tokio::spawn(async move {
          match Station::fetch_all().await {
              Ok(stations) => tx.send(Ok(stations)).await,
              Err(e) => tx.send(Err(e)).await,
          }
      });

      // Main event loop
      let tick_rate = Duration::from_millis(250);
      let mut last_tick = Instant::now();
      loop {
          terminal.draw(|f| ui(f, &mut app))?;

          let timeout = tick_rate
              .checked_sub(last_tick.elapsed())
              .unwrap_or_else(|| Duration::from_secs(0));

          // Check for completed station fetch
          if app.loading {
              if let Ok(result) = rx.try_recv() {
                  match result {
                      Ok(stations) => {
                          app.stations = stations;
                          app.loading = false;
                      }
                      Err(e) => {
                          app.history.insert(0, format!("Error loading stations: {}", e));
                          app.loading = false;
                      }
                  }
              }
              // Update spinner
              app.spinner_state = (app.spinner_state + 1) % app.spinner_frames.len();
          }

          // Check for log messages
          while let Ok(log_msg) = log_rx.try_recv() {
              app.history.insert(0, log_msg);
          }

          if last_tick.elapsed() >= tick_rate {
              last_tick = Instant::now();
              app.spinner_state = (app.spinner_state + 1) % app.spinner_frames.len();
          }

          if event::poll(timeout)? {
              if let Event::Key(key) = event::read()? {
                  match key.code {
                      KeyCode::Char('q') => app.should_quit = true,
                      KeyCode::Char('p') => {
                          if let Some(index) = app.selected_station.selected() {
                              if let Some(station) = app.stations.get(index) {
                                  if let Some(sink) = &app.sink {
                                      let sink = Arc::clone(sink);
                                      let station_url = station.url.clone();

                                      // Spawn a new task to handle audio playback
                                      let log_tx = log_tx.clone();
                                      
                                      tokio::spawn(async move {
                                          let log_tx = log_tx.clone();
                                          let add_log = move |msg: String| {
                                              let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();
                                              let log_tx = log_tx.clone();
                                              async move {
                                                  let _ = log_tx.send(format!("{}: {}", timestamp, msg)).await;
                                              }
                                          };

                                          add_log(format!("Fetching stream from: {}", &station_url)).await;

                                          match reqwest::get(&station_url).await {
                                              Ok(response) => {
                                                  add_log("Got response, starting stream...".to_string()).await;
                                                  let bytes = response.bytes().await?;
                                                  let cursor = std::io::Cursor::new(bytes);
                                                  match Decoder::new(cursor) {
                                                      Ok(source) => {
                                                          add_log("Created audio decoder, starting playback".to_string()).await;
                                                          // Stop any existing playback
                                                          {
                                                              if let Ok(sink) = sink.lock() {
                                                                  sink.stop();
                                                              }
                                                          }
                                                          
                                                          // Start new playback
                                                          let playback_success = {
                                                              if let Ok(sink) = sink.lock() {
                                                                  sink.append(source);
                                                                  sink.play();
                                                                  true
                                                              } else {
                                                                  false
                                                              }
                                                          };

                                                          if playback_success {
                                                              add_log("Playback started".to_string()).await;
                                                          } else {
                                                              add_log("Failed to lock audio sink".to_string()).await;
                                                          }
                                                      }
                                                      Err(e) => {
                                                          add_log(format!("Failed to create decoder: {}", e)).await;
                                                      }
                                                  }
                                              }
                                              Err(e) => {
                                                  add_log(format!("Failed to connect: {}", e)).await;
                                              }
                                          }
                                      });

                                      app.playback_state = PlaybackState::Playing;
                                      app.history.insert(0, format!("{}: Starting playback of {}",
                                          chrono::Local::now().format("%H:%M:%S"),
                                          &station.title));
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
                      KeyCode::Char(' ') => {
                          if let Some(sink) = &app.sink {
                              let sink = sink.lock().unwrap();
                              match app.playback_state {
                                  PlaybackState::Playing => {
                                      sink.pause();
                                      app.playback_state = PlaybackState::Paused;
                                  }
                                  PlaybackState::Paused => {
                                      sink.play();
                                      app.playback_state = PlaybackState::Playing;
                                  }
                                  PlaybackState::Stopped => {}
                              }
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

      // Left panel - Station list or loading indicator
      if app.loading {
          let loading_text = vec![
              Line::from(vec![
                  Span::raw(app.spinner_frames[app.spinner_state]),
                  Span::raw(" Loading stations..."),
              ]),
          ];
          let loading_para = Paragraph::new(loading_text)
              .block(Block::default().borders(Borders::ALL).title("Loading"))
              .alignment(ratatui::layout::Alignment::Center);
          f.render_widget(loading_para, chunks[0]);
      } else {
          let station_items: Vec<ListItem> = app
              .stations
              .iter()
              .map(|s| ListItem::new(s.title.as_str()))
              .collect();

          let stations_list = List::new(station_items)
              .block(Block::default().borders(Borders::ALL).title("Stations"))
              .highlight_style(Style::default().bg(Color::Blue));

          f.render_stateful_widget(stations_list, chunks[0], &mut app.selected_station);
      }

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
              Span::styled("[Space] Pause", Style::default().fg(Color::Blue)),
              Span::raw(" "),
              Span::styled("[S] Stop", Style::default().fg(Color::Red)),
              Span::raw(" "),
              Span::styled("[Q] Quit", Style::default().fg(Color::Yellow)),
          ]),
          Line::from(vec![
              Span::raw("Status: "),
              Span::styled(
                  match app.playback_state {
                      PlaybackState::Playing => "Playing",
                      PlaybackState::Paused => "Paused",
                      PlaybackState::Stopped => "Stopped",
                  },
                  match app.playback_state {
                      PlaybackState::Playing => Style::default().fg(Color::Green),
                      PlaybackState::Paused => Style::default().fg(Color::Blue),
                      PlaybackState::Stopped => Style::default().fg(Color::Red),
                  },
              ),
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
