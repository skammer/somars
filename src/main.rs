use clap::Parser;
use ratatui::style::Stylize;

use crossterm::{
    event::{self, Event},
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

#[derive(Debug)]
enum ControlCommand {
    Play,
    Pause,
    Stop,
    VolumeUp,
    VolumeDown,
    SetVolume(f32),
    Tune(String),
}
use rodio::{OutputStream, Sink};

mod station;
use crate::station::Station;

mod mp3_stream_decoder;
mod keyboard;



#[derive(Clone)]
pub enum MessageType {
    Error,
    Info,
    System,
    Background,
    Playback,
}

pub struct HistoryMessage {
    pub message: String,
    pub message_type: MessageType,
    pub timestamp: String,
}

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Log level (1=minimal, 2=verbose)
    #[arg(short, long, default_value_t = 1)]
    log_level: u8,
    
    /// Station ID to automatically play on startup
    #[arg(short, long)]
    station: Option<String>,

    /// Enable UDP control on specified port
    #[arg(short = 'p', long)]
    listen: Option<u16>,
}

pub struct App {
    pub stations: Vec<Station>,
    pub selected_station: ListState,
    pub active_station: Option<usize>,
    pub playback_state: PlaybackState,
    pub history: Vec<HistoryMessage>,
    pub history_scroll_state: ListState,
    pub should_quit: bool,
    pub sink: Option<Arc<Mutex<Sink>>>,
    pub loading: bool,
    pub spinner_state: usize,
    pub spinner_frames: Vec<&'static str>,
    pub playback_frames: Vec<&'static str>,
    pub playback_frame_index: usize,
    pub volume: f32,
    pub show_help: bool,
    pub log_level: u8,
    pub playback_start_time: Option<std::time::Instant>,
    pub total_played: std::time::Duration,
    pub last_pause_time: Option<std::time::Instant>,
}

#[derive(Clone)]
pub enum PlaybackState {
    Playing,
    Paused,
    Stopped,
}

 #[tokio::main]
 async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
     let cli = Cli::parse();
     use ratatui::widgets::ListState;
     // Setup terminal
     enable_raw_mode()?;
     let mut stdout = io::stdout();
     execute!(
         stdout,
         EnterAlternateScreen,
         crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
     )?;
     let backend = CrosstermBackend::new(stdout);
     let mut terminal = Terminal::new(backend)?;
     terminal.clear()?;

     // Create app state
     let (_stream, stream_handle) = OutputStream::try_default().map_err(|e| {
         anyhow::anyhow!("Failed to initialize audio: {}. Check your system's audio devices.", e)
     })?;

     let sink = match Sink::try_new(&stream_handle) {
         Ok(s) => s,
         Err(e) => {
             return Err(anyhow::anyhow!(
                 "Failed to create audio sink: {}. Make sure another application isn't blocking audio access.",
                 e
             ).into());
         }
     };

     // Create channels for logging and control
     let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(32);
     let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(32);

     // Start UDP listener if enabled
     if let Some(port) = cli.listen {
         let command_tx = command_tx.clone();
         let log_tx = log_tx.clone();  // Add this line
         
         // Add this log message before spawning
         let _ = log_tx.send(HistoryMessage {
             message: format!("Starting UDP command listener on port {}", port),
             message_type: MessageType::Info,
             timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
         }).await;

         tokio::spawn(async move {
             if let Err(e) = handle_udp_commands(port, command_tx).await {
                 eprintln!("UDP listener error: {}", e);
                 // Add error logging here too
                 let _ = log_tx.send(HistoryMessage {
                     message: format!("UDP error: {}", e),
                     message_type: MessageType::Error,
                     timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                 }).await;
             }
         });
     }

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
         log_level: cli.log_level,
         playback_start_time: None,
         total_played: std::time::Duration::default(),
         last_pause_time: None,
     };
     
     // Store the station ID to auto-play
     let auto_play_station_id = cli.station.clone();

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

                         // If a station ID was provided via command line, find and play it
                         if let Some(station_id) = &auto_play_station_id {
                             let station_index = app.stations.iter().position(|s| s.id == *station_id);

                             if let Some(index) = station_index {
                                 // Select the station in the UI
                                 app.selected_station.select(Some(index));

                                 // Play the station
                                 keyboard::handle_play(&mut app, &log_tx);

                                 app.history.push(HistoryMessage {
                                     message: format!("Auto-playing station: {}", station_id),
                                     message_type: MessageType::System,
                                     timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                                 });
                             } else {
                                 app.history.push(HistoryMessage {
                                     message: format!("Station ID not found: {}", station_id),
                                     message_type: MessageType::Error,
                                     timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                                 });
                             }
                         }
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

         // Process control commands
         while let Ok(cmd) = command_rx.try_recv() {
             match cmd {
                 ControlCommand::Play => keyboard::handle_play(&mut app, &log_tx),
                 ControlCommand::Pause => keyboard::handle_pause(&mut app),
                 ControlCommand::Stop => keyboard::handle_stop(&mut app, false),
                 ControlCommand::VolumeUp => keyboard::handle_volume_up(&mut app),
                 ControlCommand::VolumeDown => keyboard::handle_volume_down(&mut app),
                 ControlCommand::SetVolume(level) => {
                     app.volume = level.clamp(0.0, 2.0);
                     if let Some(sink) = &app.sink {
                         if let Ok(sink) = sink.lock() {
                             sink.set_volume(app.volume);
                         }
                     }
                 }
                 ControlCommand::Tune(station_id) => {
                     if let Some(index) = app.stations.iter().position(|s| s.id == station_id) {
                         app.selected_station.select(Some(index));
                         keyboard::handle_play(&mut app, &log_tx);
                     }
                 }
             }
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
                 Event::Key(key) => {
                     keyboard::handle_key_event(key.code, &mut app, &log_tx, &mut last_tick);
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
         LeaveAlternateScreen
     )?;
     terminal.show_cursor()?;

     Ok(())
 }

 fn format_duration(d: std::time::Duration) -> String {
     let secs = d.as_secs();
     let hours = secs / 3600;
     let minutes = (secs % 3600) / 60;
     let seconds = secs % 60;
     format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
 }

 fn ui(f: &mut ratatui::Frame, app: &mut App) {

     // app layout - ui and controls
     let app_layout = Layout::default()
         .direction(Direction::Vertical)
         .constraints(
             [
             Constraint::Fill(1), // History
             Constraint::Length(1), // Bottom controls
             ]
             .as_ref(),
         )
         .split(f.area());

     // Bottom controls bar
     let bottom_controls = Line::from(vec![
         Span::styled("q", Style::default().fg(Color::Yellow).add_modifier(ratatui::style::Modifier::BOLD)),
         Span::raw(":Quit "),
         Span::styled("↵", Style::default().fg(Color::Green).add_modifier(ratatui::style::Modifier::BOLD)),
         Span::raw(":Play "),
         Span::styled("Space", Style::default().fg(Color::Blue).add_modifier(ratatui::style::Modifier::BOLD)),
         Span::raw(":Stop "),
         Span::styled("s", Style::default().fg(Color::Red).add_modifier(ratatui::style::Modifier::BOLD)),
         Span::raw(":Stop "),
         Span::styled("+/-", Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::BOLD)),
         Span::raw(":Vol "),
         Span::styled("?", Style::default().fg(Color::Magenta).add_modifier(ratatui::style::Modifier::BOLD)),
         Span::raw(":Help"),
     ]);


     let _bottom_controls_alt = Paragraph::new(vec![
         Line::from(vec![
             Span::styled("Play [↵]", Style::default().fg(Color::Green).add_modifier(ratatui::style::Modifier::REVERSED)),
             Span::raw(" "),
             Span::styled("Pause [space]", Style::default().fg(Color::Blue).add_modifier(ratatui::style::Modifier::REVERSED)),
             Span::raw(" "),
             Span::styled("Stop [s]", Style::default().fg(Color::Red).add_modifier(ratatui::style::Modifier::REVERSED)),
             Span::raw(" "),
             Span::styled("Quit [q]", Style::default().fg(Color::Yellow).add_modifier(ratatui::style::Modifier::REVERSED)),
             Span::raw(" "),
             Span::styled("Volume [+/-]", Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::REVERSED)),
             Span::raw(format!(" ({:.1})", if app.volume.abs() < 0.05 { 0.0 } else { app.volume })),
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
     ]);


     let bottom_bar = Paragraph::new(bottom_controls)
         .alignment(ratatui::layout::Alignment::Left);

     // TODO: decide which option looks better
     f.render_widget(bottom_bar, app_layout[1]);
     // f.render_widget(bottom_controls_alt, app_layout[1]);


     let chunks = Layout::default()
         .direction(Direction::Horizontal)
         .constraints([Constraint::Percentage(30), Constraint::Percentage(70)].as_ref())
         .split(app_layout[0]);

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
         .constraints([
             Constraint::Length(10), // Now Playing
             Constraint::Fill(1), // History
         ]
         .as_ref(),
         )
         .split(chunks[1]);

     // Now Playing
     let now_playing = if let Some(index) = app.selected_station.selected() {
         if let Some(station) = app.stations.get(index) {
             Paragraph::new(vec![
                 Line::from(vec![
                     Span::styled("ID: ", Style::default().fg(Color::Yellow)),
                     Span::raw(&station.id),
                 ]),
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
                 Line::from(""),
                 Line::from(vec![
                     Span::styled("Playback time: ", Style::default().fg(Color::Yellow)),
                     Span::raw({
                         let total = match app.playback_state {
                             PlaybackState::Playing => {
                                 let base = app.total_played;
                                 if let Some(start) = app.playback_start_time {
                                     base + start.elapsed()
                                 } else {
                                     base
                                 }
                             }
                             _ => app.total_played
                         };
                         format_duration(total)
                     }),
                 ]),
             ])
             .wrap(ratatui::widgets::Wrap { trim: true })
         } else {
             Paragraph::new(vec![Line::from("No station selected")])
         }
     } else {
         Paragraph::new(vec![Line::from("No station selected")])
     }
     .block(Block::default().borders(Borders::ALL)
         .title(Line::from(vec![
                 Span::styled(format!(" ♪ {} v{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")), 
                     Style::default().add_modifier(ratatui::style::Modifier::BOLD))
         ]).right_aligned())
         .title(
             Line::from(vec![
                 Span::raw("["),
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
                 Span::raw("]"),

                 if matches!(app.playback_state, PlaybackState::Playing) {
                     Span::styled(format!(" {}", app.playback_frames[app.playback_frame_index]), Style::default().fg(Color::Green))
                 } else {
                     Span::raw("")
                 },

             ]),
         )

         .title_bottom(
             Line::from(
                 format!("[Volume: {:.1}]", if app.volume.abs() < 0.05 { 0.0 } else { app.volume })
                 ).centered()
             )
         );
     f.render_widget(now_playing, right_chunks[0]);

     // History
     let history_items: Vec<ListItem> = app
         .history
         .iter()
         .rev()
         .filter(|msg| app.log_level > 1 || matches!(msg.message_type, MessageType::Error | MessageType::Info | MessageType::Playback))
         .map(|msg| {
             let width = right_chunks[1].width as usize;
             let style = match msg.message_type {
                 MessageType::Error => Style::default().fg(Color::Red),
                 MessageType::Info => Style::default().fg(Color::White),
                 MessageType::System => Style::default().fg(Color::Yellow),
                 MessageType::Background => Style::default().fg(Color::DarkGray),
                 MessageType::Playback => Style::default().fg(Color::Green),
             };

             // Format timestamp and message as separate columns
             let timestamp_span = Span::styled(msg.timestamp.clone(), style);

             // Wrap just the message part
             let message_width = width.saturating_sub(10); // Timestamp width + separator
             let wrapped_lines: Vec<String> = textwrap::wrap(&msg.message, message_width)
                 .into_iter()
                 .map(|s| s.to_string())
                 .collect();

             // Create lines with proper alignment
             let mut lines = Vec::new();
             if let Some(first_line) = wrapped_lines.first() {
                 // First line has timestamp
                 lines.push(Line::from(vec![
                     timestamp_span.clone(),
                     Span::styled("  ", style),
                     Span::styled(first_line.clone(), style),
                 ]));
             }

             // Additional lines are indented to align with first message line
             for line in wrapped_lines.iter().skip(1) {
                 lines.push(Line::from(vec![
                     Span::styled("          ", style), // Timestamp width spaces
                     Span::styled(line.clone(), style),
                 ]));
             }

             let text = Text::from(lines);
             ListItem::new(text)
         })
         .collect();

     let selected_history_pos = app.history_scroll_state.selected().unwrap_or(0) + 1;
     let total_history = app.history.iter()
         .filter(|msg| app.log_level > 1 || matches!(msg.message_type, MessageType::Error) || matches!(msg.message_type, MessageType::Playback))
         .collect::<Vec<_>>()
         .len();
     let history_list = List::new(history_items).direction(ListDirection::BottomToTop)
         .block(Block::default()
             .borders(Borders::ALL)
             .title("History")
             .title(Line::from("[jk]").right_aligned())
             .title_bottom(Line::from(format!("[{} / {}]", selected_history_pos, total_history)).right_aligned())
         )
         .highlight_style(Style::default().italic().add_modifier(ratatui::style::Modifier::UNDERLINED));
     f.render_stateful_widget(history_list, right_chunks[1], &mut app.history_scroll_state);

     if app.show_help {
         let help_text = vec![
             Line::from(vec![
                 Span::styled(format!("{} - {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_DESCRIPTION")), 
                     Style::default().add_modifier(ratatui::style::Modifier::BOLD))
             ]),
             Line::from(""),
             Line::from("Keyboard Controls:"),
             Line::from(""),
             Line::from(vec![
                 Span::styled("↵ (Enter)", Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
                 Span::raw(" - Play selected station")
             ]),
             Line::from(vec![
                 Span::styled("Space", Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
                 Span::raw(" - Stop playback")
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
             Line::from("Command Line Arguments:"),
             Line::from(""),
             Line::from(vec![
                 Span::styled("--log-level <1|2>", Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
                 Span::raw(" - Set log verbosity (1=minimal, 2=verbose)")
             ]),
             Line::from(vec![
                 Span::styled("--station <ID>", Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
                 Span::raw(" - Auto-play station with given ID on startup")
             ]),
             Line::from(vec![
                 Span::styled("--help", Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
                 Span::raw(" - Show command line help")
             ]),
             Line::from(vec![
                 Span::styled("--version", Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
                 Span::raw(" - Show version information")
             ]),
             Line::from(""),
             Line::from("Press ? to close this help screen"),
         ];

         let area = popup_area(f.area(), 60, 60);
         let help_widget = Paragraph::new(help_text)
             .block(Block::default()
                 .title("Help")
                 .title_bottom(Line::from(format!("{} v{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))).right_aligned())
                 .borders(Borders::ALL)
                 .border_type(ratatui::widgets::BorderType::Double))
             .alignment(ratatui::layout::Alignment::Left)
             .wrap(ratatui::widgets::Wrap { trim: true });

         f.render_widget(ratatui::widgets::Clear, area);
         f.render_widget(help_widget, area);
     }

 }

async fn handle_udp_commands(port: u16, tx: tokio::sync::mpsc::Sender<ControlCommand>) -> io::Result<()> {
    use tokio::net::UdpSocket;
    
    let socket = UdpSocket::bind(("0.0.0.0", port)).await?;
    let mut buf = [0; 1024];

    loop {
        let (len, _) = socket.recv_from(&mut buf).await?;
        let msg = String::from_utf8_lossy(&buf[..len]).trim().to_lowercase();
        
        let cmd = match msg.split_whitespace().collect::<Vec<_>>().as_slice() {
            ["play"] => ControlCommand::Play,
            ["pause"] => ControlCommand::Pause,
            ["stop"] => ControlCommand::Stop,
            ["volume", "up"] => ControlCommand::VolumeUp,
            ["volume", "down"] => ControlCommand::VolumeDown,
            ["volume", num] => num.parse().ok().map(ControlCommand::SetVolume).unwrap_or_else(|| {
                ControlCommand::SetVolume(1.0)
            }),
            ["tune", id] => ControlCommand::Tune(id.to_string()),
            _ => continue,
        };
        
        tx.send(cmd).await.map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
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
