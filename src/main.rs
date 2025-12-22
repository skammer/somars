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

use rodio::{OutputStream, Sink};

mod station;
use crate::station::Station;

mod mp3_stream_decoder;
mod keyboard;
mod i18n;
mod error;
mod control;
mod config;
mod ui;
mod utils;
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
    #[arg(long, default_value_t = 1)]
    log_level: u8,

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
 async fn main() -> Result<(), error::AppError> {
     let cli = Cli::parse();
     
     // Load configuration
     let config = config::Config::load().unwrap_or_default();
     
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
     let (_stream, stream_handle) = OutputStream::try_default()
         .map_err(|e| error::AppError::Audio(format!("Failed to initialize audio output stream: {}. This could be due to:\n\
                                         - No audio output device available\n\
                                         - Audio device is busy or locked by another application\n\
                                         - Missing audio system dependencies (e.g., ALSA on Linux)\n\
                                         Try checking your system's audio settings or restarting your audio service.", e)))?;

     let (sink, _stream_output) = Sink::new();
     _stream_output.append(stream_handle);

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
         log_level: cli.log_level.max(config.log_level), // Use CLI flag if higher than config
         playback_start_time: None,
         total_played: std::time::Duration::default(),
         last_pause_time: None,
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
             app.history.push(log_msg);
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
             
             if let Err(e) = config.save() {
                 eprintln!("Failed to save config: {}", e);
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