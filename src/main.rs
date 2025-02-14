 use std::num::NonZeroUsize;
 use std::error::Error;

 use icy_metadata::{IcyHeaders, IcyMetadataReader, RequestIcyMetadata};

 use stream_download::http::HttpStream;
 use stream_download::http::reqwest::Client;
 use stream_download::{Settings, StreamDownload};
 use stream_download::storage::bounded::BoundedStorageProvider;
 use stream_download::storage::memory::MemoryStorageProvider;

 use crossterm::{
     event::{self, Event, KeyCode, MouseEvent, MouseEventKind},
     execute,
     terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen,
         LeaveAlternateScreen},
 };

 use ratatui::text::{Line, Span, Text};

 use ratatui::{
     backend::CrosstermBackend,
     layout::{Constraint, Direction, Layout, Flex, Rect},
     style::{Color, Style},
     widgets::{Block, Borders, List, ListItem, Paragraph, ListState, ListDirection},
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
     history_scroll_state: ListState,
     should_quit: bool,
     sink: Option<Arc<Mutex<Sink>>>,
     loading: bool,
     spinner_state: usize,
     spinner_frames: Vec<&'static str>,
     playback_frames: Vec<&'static str>,
     playback_frame_index: usize,
     volume: f32,
     show_help: bool,
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
     execute!(
         stdout,
         EnterAlternateScreen,
         crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
         crossterm::event::EnableMouseCapture
     )?;
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
         history_scroll_state: ListState::default(),
         should_quit: false,
         sink: Some(Arc::new(Mutex::new(sink))),
         loading: true,
         spinner_state: 0,
         volume: 1.0,
         spinner_frames: vec!["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
         playback_frames: vec!["▮▯▯▯", "▮▮▯▯", "▮▮▮▯", "▮▮▮▮"],
         playback_frame_index: 0,
         show_help: false,
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
                         app.history.push(HistoryMessage {
                             message: format!("Error loading stations: {}", e),
                             message_type: MessageType::Error,
                             timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                         });
                         app.loading = false;
                     }
                 }
             }
             // Update spinner
             app.spinner_state = (app.spinner_state + 1) % app.spinner_frames.len();
         }

         // Check for log messages
         while let Ok(log_msg) = log_rx.try_recv() {
             app.history.push(log_msg);
         }

         if last_tick.elapsed() >= tick_rate {
             last_tick = Instant::now();
             app.spinner_state = (app.spinner_state + 1) % app.spinner_frames.len();
             if matches!(app.playback_state, PlaybackState::Playing) {
                 app.playback_frame_index = (app.playback_frame_index + 1) % app.playback_frames.len();
             }
         }

         if event::poll(timeout)? {
             match event::read()? {
                 Event::Key(key) => match key.code {
                     KeyCode::Char('q') => app.should_quit = true,
                     KeyCode::Char('p') => {
                         if let Some(index) = app.selected_station.selected() {
                             if let Some(station) = app.stations.get(index).cloned() {
                                 if let Some(original_sink) = &app.sink {

                                     app.active_station = Some(index);

                                     // let sink = Arc::clone(sink);
                                     let station_url = station.url.clone();
                                     let station_title = station.title.clone();
                                     let station_title_error = station_title.clone();

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
                                                 add_log("Got response, starting stream...".to_string(), MessageType::Info).await;
                                                 Ok(reader)
                                             },
                                             Err(e) => {
                                                 add_log(format!("Error: {}", e), MessageType::Error).await;
                                                 Err(e)
                                             }
                                         };

                                         add_log("Playback started".to_string(), MessageType::System);

                                         add_log(format!("bit rate={:?}\n", icy_headers.bitrate().unwrap()), MessageType::Info).await;


                                         // Start new playback
                                         let playback_success = match reader {
                                             Ok(reader) => {

                                                 // Clone add_log for use in the metadata handler
                                                 let _add_log_clone = add_log.clone();

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
                                                             add_log(format!("{} :: {}", station_title, title), MessageType::Playback).await;
                                                         }
                                                     }
                                                 });

                                                 // Start playback with the new decoder
                                                 {
                                                     let locked_sink = sink.lock().unwrap();
                                                     locked_sink.append(decoder.unwrap());
                                                     locked_sink.set_volume(app.volume);
                                                     locked_sink.play();
                                                 }
                                                 true
                                             },
                                             Err(_) => {
                                                 let _ = add_log("Failed to start playback".to_string(), MessageType::Error).await;
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
                                             let _ = log_tx_clone_2.send(HistoryMessage {
                                                 message: format!("Playback error: {}", e),
                                                 message_type: MessageType::Error,
                                                 timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                                             }).await;

                                             let _ = log_tx_clone_2.send(HistoryMessage {
                                                 message: format!("Starting playback of {}", &station_title_error),
                                                 message_type: MessageType::System,
                                                 timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                                             }).await;
                                             let _ = log_tx_clone_2.send(HistoryMessage {
                                                 message: "Connecting to stream...".to_string(),
                                                 message_type: MessageType::System,
                                                 timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                                             }).await;
                                         } else {
                                             let _ = log_tx_clone_2.send(HistoryMessage {
                                                 message: "No audio sink available".to_string(),
                                                 message_type: MessageType::Error,
                                                 timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                                             }).await;
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
                     KeyCode::Char('+') | KeyCode::Char('=') => {
                         if app.volume < 2.0 {
                             app.volume += 0.1;
                             if let Some(sink) = &app.sink {
                                 if let Ok(sink) = sink.lock() {
                                     sink.set_volume(app.volume);
                                 }
                             }
                         }
                     }
                     KeyCode::Char('-') => {
                         if app.volume > 0.0 {
                             app.volume -= 0.1;
                             if let Some(sink) = &app.sink {
                                 if let Ok(sink) = sink.lock() {
                                     sink.set_volume(app.volume);
                                 }
                             }
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
                     KeyCode::Char('?') => {
                         app.show_help = !app.show_help;
                     }
                     KeyCode::Char('j') => {
                         if !app.history.is_empty() {
                             let i = app.history_scroll_state.selected().unwrap_or(0);
                             if i < app.history.len() - 1 {
                                 app.history_scroll_state.select(Some(i + 1));
                             }
                         }
                     }
                     KeyCode::Char('k') => {
                         if !app.history.is_empty() {
                             if let Some(i) = app.history_scroll_state.selected() {
                                 if i > 0 {
                                     app.history_scroll_state.select(Some(i - 1));
                                 }
                             } else {
                                 app.history_scroll_state.select(Some(0));
                             }
                         }
                     }
                     _ => {}
                 },
                 Event::Mouse(MouseEvent { kind: MouseEventKind::Down(event::MouseButton::Left), column, row, ..}) => {
                     // Check if click is in controls area
                     if row == 1 { // First row of controls
                         match column {
                             2..=3 => { // Play button
                                 if let Some(index) = app.selected_station.selected() {
                                     if let Some(station) = app.stations.get(index).cloned() {
                                         // Existing play logic...
                                     }
                                 }
                             },
                             11..=16 => { // Pause button
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
                             },
                             24..=25 => { // Stop button
                                 if let Some(sink) = &app.sink {
                                     if let Ok(sink) = sink.lock() {
                                         sink.stop();
                                         sink.empty();
                                         app.playback_state = PlaybackState::Stopped;
                                     }
                                 }
                             },
                             33..=34 => { // Quit button
                                 app.should_quit = true;
                             },
                             42..=47 => { // Volume controls
                                 // Click area for volume controls
                             },
                             _ => {}
                         }
                     }
                 }
                 _ => {}
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
     execute!(
         terminal.backend_mut(),
         LeaveAlternateScreen,
         crossterm::event::DisableMouseCapture
     )?;
     terminal.show_cursor()?;

     Ok(())
 }

 fn ui(f: &mut ratatui::Frame, app: &mut App) {
     let chunks = Layout::default()
         .direction(Direction::Horizontal)
         .constraints([Constraint::Percentage(30), Constraint::Percentage(70)].as_ref())
         .split(f.area());

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

         let selected_pos = app.selected_station.selected().unwrap_or(0) + 1;
         let total_stations = app.stations.len();
         let stations_list = List::new(station_items)
             .block(
                 Block::bordered()
                     .title(Line::from(format!("Stations")))
                     .title(Line::from("[↓↑]").right_aligned())
                     .title_bottom(Line::from(format!("[{} / {}]", selected_pos, total_stations)).right_aligned())
             )
             // .highlight_style(Style::default())
             // .highlight_symbol(">>")
             .repeat_highlight_symbol(true)
             .highlight_style(Style::default().bg(Color::Blue))
         ;


         f.render_stateful_widget(stations_list, chunks[0], &mut app.selected_station);
     }

     // Right panel - Playback controls and info
     let right_chunks = Layout::default()
         .direction(Direction::Vertical)
         .constraints(
             [
                 Constraint::Length(3), // Controls
                 Constraint::Length(10), // Now Playing
                 Constraint::Fill(1), // History
                 ]
                 .as_ref(),
         )
         .split(chunks[1]);

     // Controls
     let controls = Paragraph::new(vec![
         Line::from(vec![
             Span::styled("Play [p]", Style::default().fg(Color::Green).add_modifier(ratatui::style::Modifier::REVERSED)),
             Span::raw(" "),
             Span::styled("Pause [space]", Style::default().fg(Color::Blue).add_modifier(ratatui::style::Modifier::REVERSED)),
             Span::raw(" "),
             Span::styled("Stop [s]", Style::default().fg(Color::Red).add_modifier(ratatui::style::Modifier::REVERSED)),
             Span::raw(" "),
             Span::styled("Quit [q]", Style::default().fg(Color::Yellow).add_modifier(ratatui::style::Modifier::REVERSED)),
             Span::raw(" "),
             Span::styled("Volume [+/-]", Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::REVERSED)),
             Span::raw(format!(" ({:.1})", app.volume)),
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
     .block(Block::default()
         .borders(Borders::ALL)
         .title("Controls")
         .title(Line::from(vec![
             if matches!(app.playback_state, PlaybackState::Playing) {
                 Span::styled(app.playback_frames[app.playback_frame_index], Style::default().fg(Color::Green))
             } else {
                 Span::raw("")
             },
         ]).right_aligned())
     );
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
     .block(Block::default().borders(Borders::ALL).title("Details"));
     f.render_widget(now_playing, right_chunks[1]);

     // History
     let history_items: Vec<ListItem> = app
         .history
         .iter()
         .rev()
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
             let wrapped_lines: Vec<String> = textwrap::wrap(&formatted_msg, width.saturating_sub(2))
                 .into_iter()
                 .map(|s| s.to_string())
                 .collect();
             let lines: Vec<Line> = wrapped_lines
                 .into_iter()
                 .map(|line| Line::from(Span::styled(line, style)))
                 .collect();
             let text = Text::from(lines);
             ListItem::new(text)
         })
         .collect();

     let selected_history_pos = app.history_scroll_state.selected().unwrap_or(0) + 1;
     let total_history = app.history.len();
     let history_list = List::new(history_items).direction(ListDirection::BottomToTop)
         .block(Block::default()
             .borders(Borders::ALL)
             .title("History")
             .title(Line::from("[jk]").right_aligned())
             .title_bottom(Line::from(format!("[{} / {}]", selected_history_pos, total_history)).right_aligned())
         )
         .highlight_style(Style::default().bg(Color::DarkGray));
     f.render_stateful_widget(history_list, right_chunks[2], &mut app.history_scroll_state);


     if app.show_help {
         let help_text = vec![
             Line::from(vec![
                 Span::styled("SomaRS - SomaFM Terminal Client", Style::default().add_modifier(ratatui::style::Modifier::BOLD))
             ]),
             Line::from(""),
             Line::from("Keyboard Controls:"),
             Line::from(""),
             Line::from(vec![
                 Span::styled("p", Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
                 Span::raw(" - Play selected station")
             ]),
             Line::from(vec![
                 Span::styled("Space", Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
                 Span::raw(" - Pause/Resume playback")
             ]),
             Line::from(vec![
                 Span::styled("s", Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
                 Span::raw(" - Stop playback")
             ]),
             Line::from(vec![
                 Span::styled("+/-", Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
                 Span::raw(" - Adjust volume")
             ]),
             Line::from(vec![
                 Span::styled("↑/↓", Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
                 Span::raw(" - Navigate stations")
             ]),
             Line::from(vec![
                 Span::styled("q", Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
                 Span::raw(" - Quit application")
             ]),
             Line::from(vec![
                 Span::styled("?", Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
                 Span::raw(" - Toggle this help screen")
             ]),
             Line::from(""),
             Line::from("Press ? to close this help screen"),
         ];

         let area = popup_area(f.area(), 60, 60);
         let help_widget = Paragraph::new(help_text)
             .block(Block::default()
                 .title("Help")
                 .title_bottom(Line::from("somars v0.0.1").right_aligned())
                 .borders(Borders::ALL)
                 .border_type(ratatui::widgets::BorderType::Double))
             .alignment(ratatui::layout::Alignment::Left)
             .wrap(ratatui::widgets::Wrap { trim: true });

         f.render_widget(ratatui::widgets::Clear, area);
         f.render_widget(help_widget, area);
     }

 }

/// helper function to create a centered rect using up certain percentage of the available rect `r`
fn popup_area(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let vertical = Layout::vertical([Constraint::Percentage(percent_y)]).flex(Flex::Center);
    let horizontal = Layout::horizontal([Constraint::Percentage(percent_x)]).flex(Flex::Center);
    let [area] = vertical.areas(area);
    let [area] = horizontal.areas(area);
    area
}
