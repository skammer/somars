 use std::num::NonZeroUsize;
 use std::error::Error;

 use icy_metadata::{IcyHeaders, IcyMetadataReader, RequestIcyMetadata};

 use stream_download::http::HttpStream;
 use stream_download::http::reqwest::Client;
 use stream_download::{Settings, StreamDownload};
 use stream_download::storage::bounded::BoundedStorageProvider;
 use stream_download::storage::memory::MemoryStorageProvider;

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
     widgets::{Block, Borders, List, ListItem, Paragraph, ListState},
     Terminal,
 };
 use std::{
     io,
     sync::{Arc, Mutex},
     time::{Duration, Instant}
 };
 use rodio::{OutputStream, Sink};

 mod station;
 use crate::station::Station;

 mod mp3_stream_decoder;



 #[derive(Clone)]
 enum MessageType {
     Error,
     Info,
     System,
     Background,
     Playback,
 }

 struct HistoryMessage {
     message: String,
     message_type: MessageType,
     timestamp: String,
 }

 struct App {
     stations: Vec<Station>,
     selected_station: ListState,
     active_station: Option<usize>,
     playback_state: PlaybackState,
     history: Vec<HistoryMessage>,
     should_quit: bool,
     sink: Option<Arc<Mutex<Sink>>>,
     loading: bool,
     spinner_state: usize,
     spinner_frames: Vec<&'static str>,
     playback_frames: Vec<&'static str>,
     playback_frame_index: usize,
 }

 #[derive(Clone)]
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
         active_station: None,
         playback_state: PlaybackState::Stopped,
         history: Vec::new(),
         should_quit: false,
         sink: Some(Arc::new(Mutex::new(sink))),
         loading: true,
         spinner_state: 0,
         spinner_frames: vec!["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
         playback_frames: vec!["▮▯▯▯", "▮▮▯▯", "▮▮▮▯", "▮▮▮▮"],
         playback_frame_index: 0,
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
             if matches!(app.playback_state, PlaybackState::Playing) {
                 app.playback_frame_index = (app.playback_frame_index + 1) % app.playback_frames.len();
             }
         }

         if event::poll(timeout)? {
             if let Event::Key(key) = event::read()? {
                 match key.code {
                     KeyCode::Char('q') => app.should_quit = true,
                     KeyCode::Char('p') => {
                         if let Some(index) = app.selected_station.selected() {
                             if let Some(station) = app.stations.get(index).cloned() {
                                 if let Some(original_sink) = &app.sink {

                                     app.active_station = Some(index);

                                     // let sink = Arc::clone(sink);
                                     let station_url = station.url.clone();

                                     // Stop any existing playback before starting new stream
                                     if let Ok(locked_sink) = original_sink.lock() {
                                         locked_sink.stop();
                                     }

                                     let sink = original_sink.clone();
                                     let log_tx_clone = log_tx.clone();
                                     let handle: tokio::task::JoinHandle<Result<(), Box<dyn std::error::Error + Send + Sync>>> = tokio::spawn(async move {


                                         // Spawn a new task to handle audio playback

                                         let add_log = {
                                             let log_tx_clone = log_tx_clone.clone();
                                             move |msg: String, msg_type: MessageType| {
                                                 let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();
                                                 let log_tx_clone = log_tx_clone.clone();
                                                 async move {
                                                     let history_message = HistoryMessage {
                                                         message: msg,
                                                         message_type: msg_type,
                                                         timestamp,
                                                     };
                                                     let _ = log_tx_clone.send(history_message).await;
                                                 }
                                             }
                                         };

                                         add_log(format!("Initializing stream from: {}", &station_url), MessageType::System).await;
                                         add_log(format!("Async shenanigans for: {}", &station_url), MessageType::Background).await;

                                         // We need to add a header to tell the Icecast server that we can parse the metadata embedded
                                         // within the stream itself.
                                         let client = Client::builder().request_icy_metadata().build()?;

                                         let stream = HttpStream::new(client, station_url.to_string().parse()?).await?;

                                         let icy_headers = IcyHeaders::parse_from_headers(stream.headers());

                                         // buffer 5 seconds of audio
                                         // bitrate (in kilobits) / bits per byte * bytes per kilobyte * 5 seconds
                                         let prefetch_bytes = icy_headers.bitrate().unwrap() / 8 * 1024 * 5;

                                         let reader = match StreamDownload::from_stream(
                                             stream,
                                             BoundedStorageProvider::new(
                                                 MemoryStorageProvider,
                                                 NonZeroUsize::new(512 * 1024).unwrap(),
                                             ),
                                             Settings::default().prefetch_bytes(prefetch_bytes as u64),
                                         )

                                         .await {
                                             Ok(reader) => {
                                                 add_log("Got response, starting stream...".to_string()).await;
                                                 Ok(reader)
                                             },
                                             Err(e) => {
                                                 add_log(format!("Error: {}", e)).await;
                                                 Err(e)
                                             }
                                         };

                                         add_log("Playback started".to_string(), MessageType::System);

                                         add_log(format!("bit rate={:?}\n", icy_headers.bitrate().unwrap()));


                                         // Start new playback
                                         let playback_success = match reader {
                                             Ok(reader) => {

                                                 // Clone add_log for use in the metadata handler
                                                 let add_log_clone = add_log.clone();

                                                 // Create a channel for metadata updates
                                                 let (metadata_tx, mut metadata_rx) = tokio::sync::mpsc::channel(32);
                                                
                                                 let decoder = tokio::task::spawn_blocking(move || {
                                                     rodio::Decoder::new_mp3(IcyMetadataReader::new(
                                                         reader,
                                                         icy_headers.metadata_interval(),
                                                         move |metadata| {
                                                             if let Ok(metadata) = metadata {
                                                                 if let Some(title) = metadata.stream_title() {
                                                                     let _ = metadata_tx.blocking_send(title.to_string());
                                                                 }
                                                             }
                                                         }
                                                     ))
                                                 }).await?;

                                                 // Spawn a task to handle metadata updates
                                                 tokio::spawn({
                                                     let add_log = add_log.clone();
                                                     async move {
                                                         while let Some(title) = metadata_rx.recv().await {
                                                             add_log(format!("Now Playing: {}", title), MessageType::Playback).await;
                                                         }
                                                     }
                                                 });

                                                 // Start playback with the new decoder
                                                 {
                                                     let locked_sink = sink.lock().unwrap();
                                                     locked_sink.append(decoder.unwrap());
                                                     locked_sink.play();
                                                 }
                                                 true
                                             },
                                             Err(_) => {
                                                 let _ = add_log("Failed to start playback".to_string()).await;
                                                 false
                                             },
                                         };

                                         if playback_success {
                                             add_log("Playback started".to_string(), MessageType::System).await;
                                         } else {
                                             add_log("Failed to lock audio sink".to_string(), MessageType::Error).await;
                                         }

                                         Ok::<_, Box<dyn Error + Send + Sync>>(())
                                     });

                                     let log_tx_clone = log_tx.clone();
                                     app.playback_state = PlaybackState::Playing;

                                     tokio::spawn(async move {
                                         let log_tx_clone_2 = log_tx_clone.clone();
                                         if let Err(e) = handle.await {
                                             let _ = log_tx_clone_2.send(format!("{}: Playback error: {}",
                                                 chrono::Local::now().format("%H:%M:%S"), e)).await;

                                             let _ = log_tx_clone_2.send(format!("{}: Starting playback of {}",
                                                 chrono::Local::now().format("%H:%M:%S"), &station.title)).await;
                                             let _ = log_tx_clone_2.send(format!("{}: Connecting to stream...",
                                                 chrono::Local::now().format("%H:%M:%S"))).await;
                                         } else {
                                             let _ = log_tx_clone_2.send(format!("{}: No audio sink available",
                                                 chrono::Local::now().format("%H:%M:%S"))).await;
                                         }
                                     });


                                 }
                             }
                         }
                     }
                     KeyCode::Char('s') => {
                         if let Some(sink) = &app.sink {
                             if let Ok(sink) = sink.lock() {
                                 sink.stop();
                                 sink.empty();
                                 app.playback_state = PlaybackState::Stopped;
                             }
                         }
                     }
                     KeyCode::Char(' ') => {
                         if let Some(sink) = &app.sink {
                             if let Ok(sink) = sink.lock() {
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
                         if !app.loading {
                             if let Some(selected) = app.selected_station.selected() {
                                 if selected < app.stations.len() - 1 {
                                     app.selected_station.select(Some(selected + 1));
                                 }
                             } else if !app.stations.is_empty() {
                                 app.selected_station.select(Some(0));
                             }
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
             .enumerate()
             .map(|(i, s)| {
                 let style = if Some(i) == app.active_station {
                     Style::default().add_modifier(ratatui::style::Modifier::UNDERLINED)
                 } else {
                     Style::default()
                 };
                 ListItem::new(Span::styled(s.title.as_str(), style))
             })
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
                 Constraint::Percentage(30), // Now Playing
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
             Span::raw(" ".repeat((right_chunks[0].width as usize).saturating_sub(50))), // 50 is approximate width of other elements
             if matches!(app.playback_state, PlaybackState::Playing) {
                 Span::styled(app.playback_frames[app.playback_frame_index], Style::default().fg(Color::Green))
             } else {
                 Span::raw("    ")
             },
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
         .map(|msg| {
             let width = right_chunks[2].width as usize;
             let style = match msg.message_type {
                 MessageType::Error => Style::default().fg(Color::Red),
                 MessageType::Info => Style::default().fg(Color::White),
                 MessageType::System => Style::default().fg(Color::Yellow),
                 MessageType::Background => Style::default().fg(Color::DarkGray),
                 MessageType::Playback => Style::default().fg(Color::Green),
             };
             let formatted_msg = format!("{}: {}", msg.timestamp, msg.message);
             let wrapped_lines: Vec<Line> = textwrap::wrap(&formatted_msg, width.saturating_sub(2))
                 .into_iter()
                 .map(|line| Line::from(Span::styled(line, style)))
                 .collect();
             let text = Text::from(wrapped_lines);
             ListItem::new(text)
         })
         .collect();

     let history_list = List::new(history_items)
         .block(Block::default().borders(Borders::ALL).title("History"));
     f.render_widget(history_list, right_chunks[2]);
 }
