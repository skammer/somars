use clap::Parser;

use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen,
        LeaveAlternateScreen},
};

use ratatui::{
    backend::CrosstermBackend,
    widgets::ListState,
    Terminal,
};

use std::{
    io,
    sync::{Arc, Mutex},
    time::{Duration, Instant}
};

use rodio::{OutputStreamBuilder, Sink};

mod station;
use crate::station::Station;

mod keyboard;
mod i18n;
mod error;
mod control;
mod config;
mod ui;
mod utils;
mod audio_monitor;
use control::ControlCommand;
use i18n::t;



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
    #[arg(long)]
    log_level: Option<u8>,

    /// Station ID to automatically play on startup
    #[arg(short, long)]
    station: Option<String>,

    /// Enable UDP control
    #[arg(short = 'l', long)]
    listen: bool,

    /// Port for UDP control [default: 8069]
    #[arg(short = 'p', long, default_value_t = 8069)]
    port: u16,

    /// Broadcast a UDP command to the network and exit
    #[arg(short = 'b', long)]
    broadcast: Option<String>,

    /// Set the locale (en, ru)
    #[arg(short = 'L', long)]
    locale: Option<String>,

    /// Print the config file path and exit
    #[arg(long)]
    print_config_path: bool,

    /// Path to config file
    #[arg(long)]
    config: Option<String>,
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
    pub last_underrun_check: Option<std::time::Instant>,
    pub last_position: std::time::Duration,
    pub underrun_detected: bool,
    pub station_loading: bool,
    pub playback_start_time_for_underrun: Option<std::time::Instant>,
    pub last_restart_time: Option<std::time::Instant>,
    pub restart_attempts: u32,
}

#[derive(Clone)]
pub enum PlaybackState {
    Playing,
    Paused,
    Stopped,
}

 #[tokio::main]
 async fn main() -> Result<(), error::AppError> {
     let cli = Cli::parse();

     // Handle print config path mode
     if cli.print_config_path {
         match config::Config::config_path() {
             Ok(path) => {
                 println!("{}", path.display());
                 return Ok(());
             }
             Err(e) => {
                 eprintln!("Error getting config path: {}", e);
                 return Err(error::AppError::Generic(format!("Failed to get config path: {}", e)));
             }
         }
     }

     // Load configuration
     let config = config::Config::load_from_path(cli.config.clone()).unwrap_or_default();
     let config_file_path = cli.config.clone();

     // Initialize i18n
     i18n::init(cli.locale.clone());

     // Handle broadcast mode
     if let Some(message) = cli.broadcast {
         send_udp_broadcast(&message, cli.port).await?;
         return Ok(());
     }
     
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
     let stream_handle = OutputStreamBuilder::open_default_stream().map_err(|e| {
         error::AppError::Audio(format!("Failed to initialize audio output stream: {}. This could be due to:\n\
                                         - No audio output device available\n\
                                         - Audio device is busy or locked by another application\n\
                                         - Missing audio system dependencies (e.g., ALSA on Linux)\n\
                                         Try checking your system's audio settings or restarting your audio service.", e))
     })?;

     let sink = Sink::connect_new(stream_handle.mixer());

     // Create channels for logging and control
     let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(32);
     let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(32);

     // Start UDP listener if enabled
     let udp_enabled = cli.listen || config.udp_enabled;
     let udp_port = cli.port.max(config.udp_port); // Use CLI port if specified, otherwise config port
     if udp_enabled {
         let port = udp_port;
         let command_tx = command_tx.clone();
         let log_tx = log_tx.clone();
         
         // Add this log message before spawning
         let _ = log_tx.send(HistoryMessage {
             message: t("udp-starting").replace("{$port}", &port.to_string()),
             message_type: MessageType::Info,
             timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
         }).await;

         tokio::spawn(async move {
             if let Err(e) = handle_udp_commands(port, command_tx).await {
                 eprintln!("UDP listener error: {}", e);
                 // Add error logging here too
                 let _ = log_tx.send(HistoryMessage {
                     message: t("udp-error").replace("{$error}", &e.to_string()),
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
         volume: config.volume,
         spinner_frames: vec!["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
         playback_frames: vec!["▮▯▯▯", "▮▮▯▯", "▮▮▮▯", "▮▮▮▮"],
         playback_frame_index: 0,
         show_help: false,
         log_level: cli.log_level.unwrap_or(config.log_level), // Use CLI value if provided, otherwise config value
         playback_start_time: None,
         total_played: std::time::Duration::default(),
         last_pause_time: None,
         last_underrun_check: None,
         last_position: std::time::Duration::default(),
         underrun_detected: false,
         station_loading: false,
         playback_start_time_for_underrun: None,
         last_restart_time: None,
         restart_attempts: 0,
     };
     
     // Store the station ID to auto-play
     let auto_play_station_id = cli.station.clone().or(config.last_station);

     // Spawn station fetching task
     let (tx, mut rx) = tokio::sync::mpsc::channel(1);
     tokio::spawn(async move {
         match Station::fetch_all().await {
             Ok(stations) => tx.send(Ok(stations)).await,
             Err(e) => tx.send(Err(e)).await,
         }
     });

     // Initialize variables for audio monitoring
     let mut last_position = std::time::Duration::default();

     // Main event loop
     let tick_rate = Duration::from_millis(250);
     let mut last_tick = Instant::now();
     loop {
         terminal.draw(|f| ui::ui(f, &mut app))?;

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
                                     message: t("auto-playing").replace("{$id}", station_id),
                                     message_type: MessageType::System,
                                     timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                                 });
                             } else {
                                 app.history.push(HistoryMessage {
                                     message: t("station-not-found").replace("{$id}", station_id),
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
             // Check if this is a special message to clear station loading flag
             if log_msg.message == "CLEAR_STATION_LOADING" {
                 app.station_loading = false;
                 // Reset restart attempts when station loads successfully
                 app.restart_attempts = 0;
             } else {
                 app.history.push(log_msg);
             }
         }

         // Process control commands
         while let Ok(cmd) = command_rx.try_recv() {
             keyboard::execute_command(cmd, &mut app, &log_tx);
         }

         if last_tick.elapsed() >= tick_rate {
             last_tick = Instant::now();
             app.spinner_state = (app.spinner_state + 1) % app.spinner_frames.len();
             if matches!(app.playback_state, PlaybackState::Playing) {
                 app.playback_frame_index = (app.playback_frame_index + 1) % app.playback_frames.len();
             }

             // Check for audio underruns
             if matches!(app.playback_state, PlaybackState::Playing) {
                 let mut potential_underrun = false;
                 let mut current_pos = std::time::Duration::default();
                 let now = Instant::now();

                 if let Some(ref sink) = app.sink {
                     if let Ok(sink_guard) = sink.lock() {
                         // Check if the queue is running dry
                         let queue_empty = sink_guard.empty();
                         let queue_len = sink_guard.len();

                         // Get current playback position
                         current_pos = sink_guard.get_pos();

                         // Check if playback has stalled
                         let should_have_progressed = if let Some(start_time) = app.playback_start_time {
                             now.duration_since(start_time)
                         } else {
                             std::time::Duration::default()
                         };

                         // Calculate if we're falling behind
                         let pos_diff = if current_pos > last_position {
                             current_pos - last_position
                         } else {
                             std::time::Duration::default()
                         };

                         // Check for potential underrun conditions
                         potential_underrun = queue_empty ||
                             (queue_len == 0 && !sink_guard.is_paused()) ||
                             (pos_diff.as_millis() == 0 && should_have_progressed.as_millis() > 5000); // No progress in 5 seconds
                     }
                 }

                 // Update last position for next check
                 last_position = current_pos;

                 // Check if we're past the grace period (first 5 seconds after playback starts)
                 let past_grace_period = if let Some(start_time) = app.playback_start_time_for_underrun {
                     now.duration_since(start_time).as_secs() > 5
                 } else {
                     false // If no start time recorded, assume we're in grace period
                 };

                 // Calculate the required backoff time (exponential backoff: 0.5s, 1s, 2s, 4s, 8s, 16s, max 30s)
                 let required_backoff = std::time::Duration::from_secs_f64(
                     (0.5 * (2_f64.powi(app.restart_attempts as i32))).min(30.0)
                 );

                 // Check if enough time has passed since the last restart
                 let past_backoff_period = if let Some(last_restart) = app.last_restart_time {
                     now.duration_since(last_restart) >= required_backoff
                 } else {
                     true // If no previous restart, we can restart immediately
                 };

                 if potential_underrun && !app.station_loading && past_grace_period && past_backoff_period {
                     app.underrun_detected = true;

                     // Log the underrun detection
                     let _ = log_tx.send(HistoryMessage {
                         message: t("underrun-detected"),
                         message_type: MessageType::Error,
                         timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                     }).await;

                     // Update restart tracking
                     app.last_restart_time = Some(now);
                     app.restart_attempts = app.restart_attempts.saturating_add(1);

                     // Restart playback to recover from underrun
                     audio_monitor::restart_playback(&mut app, &log_tx);
                 } else {
                     app.underrun_detected = false;
                 }

                 app.last_underrun_check = Some(now);
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
             // Save configuration before quitting
             let mut config = config::Config::load().unwrap_or_default();
             config.volume = app.volume;
             config.log_level = app.log_level;
             config.udp_port = udp_port;
             config.udp_enabled = udp_enabled;
             
             // Save the last played station
             if let Some(index) = app.active_station {
                 if let Some(station) = app.stations.get(index) {
                     config.last_station = Some(station.id.clone());
                 }
             }
             
             if let Some(path) = &config_file_path {
                 if let Err(e) = config.save_to_path(path) {
                     eprintln!("Failed to save config to {}: {}", path, e);
                 }
             } else {
                 if let Err(e) = config.save() {
                     eprintln!("Failed to save config: {}", e);
                 }
             }
             
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

async fn send_udp_broadcast(message: &str, port: u16) -> Result<(), error::AppError> {
    use tokio::net::UdpSocket;
    
    let socket = UdpSocket::bind("0.0.0.0:0").await
        .map_err(|e| error::AppError::Udp(format!("Failed to bind UDP socket: {}", e)))?;
    socket.set_broadcast(true)
        .map_err(|e| error::AppError::Udp(format!("Failed to enable broadcast: {}", e)))?;
    let target_addr = format!("255.255.255.255:{}", port);
    socket.send_to(message.as_bytes(), &target_addr).await
        .map_err(|e| error::AppError::Udp(format!("Failed to send UDP packet to {}: {}", target_addr, e)))?;
    Ok(())
}

async fn handle_udp_commands(port: u16, tx: tokio::sync::mpsc::Sender<ControlCommand>) -> Result<(), error::AppError> {
    use tokio::net::UdpSocket;
    
    let socket = UdpSocket::bind(("0.0.0.0", port)).await
        .map_err(|e| error::AppError::Udp(format!("Failed to bind to port {}: {}", port, e)))?;
    let mut buf = [0; 1024];

    loop {
        let (len, addr) = socket.recv_from(&mut buf).await
            .map_err(|e| error::AppError::Udp(format!("Failed to receive UDP packet: {}", e)))?;
        let msg = String::from_utf8_lossy(&buf[..len]).trim().to_lowercase();
        
        // Log received command
        println!("Received UDP command from {}: {}", addr, msg);
        
        let cmd = match msg.split_whitespace().collect::<Vec<_>>().as_slice() {
            ["play"] => ControlCommand::Play,
            ["stop"] => ControlCommand::Stop,
            ["volume", "up"] => ControlCommand::VolumeUp,
            ["volume", "down"] => ControlCommand::VolumeDown,
            ["volume", num] => {
                match num.parse::<f32>() {
                    Ok(value) => {
                        if value >= 0.0 && value <= 2.0 {
                            ControlCommand::SetVolume(value)
                        } else {
                            eprintln!("Volume value out of range (0.0-2.0): {}", value);
                            continue;
                        }
                    },
                    Err(_) => {
                        eprintln!("Invalid volume value: {}", num);
                        continue;
                    }
                }
            },
            ["tune", "next"] => ControlCommand::TuneNext,
            ["tune", "prev"] => ControlCommand::TunePrev,
            ["tune", id] => ControlCommand::Tune(id.to_string()),
            ["select", "up"] => ControlCommand::SelectUp,
            ["select", "down"] => ControlCommand::SelectDown,
            ["toggle"] => ControlCommand::Toggle,
            _ => {
                eprintln!("Unknown UDP command: {}", msg);
                continue;
            },
        };
        
        tx.send(cmd).await
            .map_err(|e| error::AppError::Udp(format!("Failed to send command to app: {}", e)))?;
    }
}
