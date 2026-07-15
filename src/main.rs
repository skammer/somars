use clap::Parser;
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

use ratatui::{backend::CrosstermBackend, Terminal};

use std::{
    collections::HashMap,
    io, mem,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use rodio::{OutputStreamBuilder, Sink};

mod station;
use crate::station::Station;

mod action;
mod app;
mod audio;
mod components;
mod config;
mod control;
mod error;
mod event;
mod i18n;
mod logging;
mod mpris;
mod tui;
mod utils;
use app::App;
use control::ControlCommand;
use i18n::t;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MessageType {
    Error,
    Info,
    System,
    Background,
    Playback,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    #[arg(short = 'p', long)]
    port: Option<u16>,

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PlaybackState {
    Playing,
    Paused,
    Stopped,
}

#[tokio::main]
async fn main() -> color_eyre::eyre::Result<()> {
    // Initialize logging early (before other operations)
    logging::init_logging();

    let cli = Cli::parse();

    // Handle print config path mode
    if cli.print_config_path {
        match config::Config::config_path() {
            Ok(path) => {
                println!("{}", path.display());
                return Ok(());
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                error!("Failed to get config path: {}", e);
                return Err(color_eyre::eyre::eyre!("Failed to get config path: {}", e));
            }
        }
    }

    // Load configuration using the simplified method
    let mut config = if let Some(path) = cli.config.clone() {
        config::Config::load_from_path(Some(path.clone())).unwrap_or_else(|e| {
            warn!("Failed to load configuration from {}: {}", path, e);
            eprintln!("Warning: Failed to load configuration: {}", e);
            eprintln!("Using default configuration.");
            config::Config::default()
        })
    } else {
        config::Config::load_or_default()
    };

    // Apply CLI overrides
    if let Some(log_level) = cli.log_level {
        config.log_level = log_level;
    }
    let config_file_path = cli.config.clone();

    // Determine initial station: CLI argument takes priority over config
    let initial_station = cli.station.or_else(|| config.last_station.clone());

    // Initialize i18n
    i18n::init(cli.locale.clone());

    // Handle broadcast mode
    if let Some(message) = cli.broadcast {
        send_udp_broadcast(&message, cli.port.unwrap_or(config.udp_port))
            .await
            .map_err(|e| color_eyre::eyre::eyre!("Failed to send UDP broadcast: {}", e))?;
        return Ok(());
    }

    // Setup terminal
    enable_raw_mode().map_err(|e| color_eyre::eyre::eyre!("Failed to enable raw mode: {}", e))?;
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
    let stream = OutputStreamBuilder::from_default_device()
         .and_then(|builder| {
             builder
                 .with_buffer_size(rodio::cpal::BufferSize::Fixed(config.audio_output_buffer_frames))
                 .with_error_callback(|err| error!("Audio output stream error: {}", err))
                 .open_stream()
         })
         .map_err(|e| {
         error::AppError::Audio(format!("Failed to initialize audio output stream: {}. This could be due to:\n\
                                         - No audio output device available\n\
                                         - Audio device is busy or locked by another application\n\
                                         - Missing audio system dependencies (e.g., ALSA on Linux)\n\
                                         Try checking your system's audio settings or restarting your audio service.", e))
     })?;

    let mixer = stream.mixer();
    let sink = Sink::connect_new(mixer);

    // Create channels for logging and control
    let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(32);
    let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(32);

    // Start UDP listener if enabled
    let udp_enabled = cli.listen || config.udp_enabled;
    let udp_port = cli.port.unwrap_or(config.udp_port);
    if udp_enabled {
        let port = udp_port;
        let command_tx = command_tx.clone();
        let log_tx = log_tx.clone();

        // Add this log message before spawning
        let _ = log_tx
            .send(HistoryMessage {
                message: t("udp-starting").replace("{$port}", &port.to_string()),
                message_type: MessageType::Info,
                timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
            })
            .await;

        tokio::spawn(async move {
            if let Err(e) = handle_udp_commands(port, command_tx).await {
                error!("UDP listener error: {}", e);
                // Add error logging here too
                let _ = log_tx
                    .send(HistoryMessage {
                        message: t("udp-error").replace("{$error}", &e.to_string()),
                        message_type: MessageType::Error,
                        timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                    })
                    .await;
            }
        });
        info!("UDP listener started on port {}", port);
    }

    // Create metadata channel for audio playback
    let (metadata_tx, mut metadata_rx) = tokio::sync::mpsc::channel(32);

    // Create the new App
    let sink = Arc::new(Mutex::new(sink));
    let mut app = App::new(
        4.0,  // tick_rate
        60.0, // frame_rate
        sink,
        metadata_tx,
        log_tx.clone(),
        config.clone(),
        initial_station,
    );

    // Forward ICY stream metadata into application state and MPRIS.
    let metadata_action_tx = app.action_tx.clone();
    tokio::spawn(async move {
        while let Some(event) = metadata_rx.recv().await {
            if let audio::MetadataEvent::Track { station, title } = event {
                let _ = metadata_action_tx.send(action::Action::MetadataUpdate { station, title });
            }
        }
    });

    // Spawn station fetching task
    let action_tx_clone = app.action_tx.clone();
    tokio::spawn(async move {
        match Station::fetch_all().await {
            Ok(stations) => {
                let _ = action_tx_clone.send(action::Action::UpdateStations(stations));
            }
            Err(e) => {
                let _ = action_tx_clone.send(action::Action::Error(format!(
                    "Error loading stations: {}",
                    e
                )));
            }
        }
    });

    // Handle UDP commands by converting them to Actions
    let udp_action_tx = app.action_tx.clone();
    let _udp_log_tx = log_tx.clone();
    tokio::spawn(async move {
        while let Some(cmd) = command_rx.recv().await {
            match cmd {
                ControlCommand::Play => {
                    let _ = udp_action_tx.send(action::Action::Play);
                }
                ControlCommand::Stop => {
                    let _ = udp_action_tx.send(action::Action::Stop);
                }
                ControlCommand::TogglePause => {
                    let _ = udp_action_tx.send(action::Action::TogglePause);
                }
                ControlCommand::VolumeUp => {
                    let _ = udp_action_tx.send(action::Action::VolumeUp);
                }
                ControlCommand::VolumeDown => {
                    let _ = udp_action_tx.send(action::Action::VolumeDown);
                }
                ControlCommand::SetVolume(level) => {
                    let _ = udp_action_tx.send(action::Action::SetVolume(level));
                }
                ControlCommand::Tune(station_id) => {
                    let _ = udp_action_tx.send(action::Action::TuneStation(station_id));
                }
                ControlCommand::TuneNext => {
                    let _ = udp_action_tx.send(action::Action::TuneNext);
                }
                ControlCommand::TunePrev => {
                    let _ = udp_action_tx.send(action::Action::TunePrev);
                }
                ControlCommand::SelectUp => {
                    let _ = udp_action_tx.send(action::Action::StationUp);
                }
                ControlCommand::SelectDown => {
                    let _ = udp_action_tx.send(action::Action::StationDown);
                }
                ControlCommand::Toggle => {
                    let _ = udp_action_tx.send(action::Action::TogglePlayStop);
                }
                ControlCommand::ToggleHelp => {
                    let _ = udp_action_tx.send(action::Action::ToggleHelp);
                }
                ControlCommand::ScrollHistoryUp => {
                    let _ = udp_action_tx.send(action::Action::ScrollHistoryUp);
                }
                ControlCommand::ScrollHistoryDown => {
                    let _ = udp_action_tx.send(action::Action::ScrollHistoryDown);
                }
                ControlCommand::Quit => {
                    let _ = udp_action_tx.send(action::Action::Quit);
                }
            }
        }
    });

    // Handle log messages by updating the app
    let app_action_tx = app.action_tx.clone();
    tokio::spawn(async move {
        while let Some(log_msg) = log_rx.recv().await {
            // Check if this is a special message to clear station loading flag
            if log_msg.message == "CLEAR_STATION_LOADING" {
                // This will be handled in the app
            } else {
                let _ = app_action_tx.send(action::Action::AddHistoryMessage(log_msg));
            }
        }
    });

    // Run the application
    app.run().await?;

    if let Some(handle) = app.audio_manager.take_handle() {
        handle.abort();
    }

    // Stop audio playback to prevent the OutputStream warning
    if let Some(sink) = &app.sink {
        if let Ok(sink) = sink.lock() {
            sink.stop();
        }
    }
    // Drop the sink explicitly to stop audio before OutputStream is dropped
    drop(app.sink.take());

    // Give the audio system time to finish
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Save configuration before quitting
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

    let save_result = if let Some(path) = &config_file_path {
        config.save_to_path(path)
    } else {
        config.save()
    };

    if let Err(e) = save_result {
        warn!("Failed to save configuration: {}", e);
    }

    info!("Application shutdown completed");

    // Cleanup terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    // Forget the OutputStream to prevent the warning message
    // This is safe since we're about to exit anyway
    mem::forget(stream);

    Ok(())
}

async fn send_udp_broadcast(message: &str, port: u16) -> Result<(), error::AppError> {
    use tokio::net::UdpSocket;

    // Validate message length to prevent potential abuse
    const MAX_BROADCAST_MESSAGE_LEN: usize = 256;
    if message.len() > MAX_BROADCAST_MESSAGE_LEN {
        return Err(error::AppError::Udp(format!(
            "Broadcast message too long: {} bytes (max: {})",
            message.len(),
            MAX_BROADCAST_MESSAGE_LEN
        )));
    }

    let socket = UdpSocket::bind("0.0.0.0:0")
        .await
        .map_err(|e| error::AppError::Udp(format!("Failed to bind UDP socket: {}", e)))?;
    socket
        .set_broadcast(true)
        .map_err(|e| error::AppError::Udp(format!("Failed to enable broadcast: {}", e)))?;
    let target_addr = format!("255.255.255.255:{}", port);
    socket
        .send_to(message.as_bytes(), &target_addr)
        .await
        .map_err(|e| {
            error::AppError::Udp(format!(
                "Failed to send UDP packet to {}: {}",
                target_addr, e
            ))
        })?;
    Ok(())
}

async fn handle_udp_commands(
    port: u16,
    tx: tokio::sync::mpsc::Sender<ControlCommand>,
) -> Result<(), error::AppError> {
    use tokio::net::UdpSocket;

    let socket = UdpSocket::bind(("0.0.0.0", port))
        .await
        .map_err(|e| error::AppError::Udp(format!("Failed to bind to port {}: {}", port, e)))?;
    let mut buf = [0; 1024];

    // Rate limiting: max 10 requests per second per IP
    const MAX_REQUESTS_PER_SECOND: u32 = 10;
    const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(1);
    let mut rate_tracker: HashMap<SocketAddr, Vec<Instant>> = HashMap::new();
    let mut seq_tracker: HashMap<SocketAddr, Vec<(u32, Instant)>> = HashMap::new();

    loop {
        let (len, addr) = socket
            .recv_from(&mut buf)
            .await
            .map_err(|e| error::AppError::Udp(format!("Failed to receive UDP packet: {}", e)))?;

        // Validate message length before processing
        const MAX_UDP_MESSAGE_LEN: usize = 256;
        if len > MAX_UDP_MESSAGE_LEN {
            warn!(
                "UDP packet from {} too large: {} bytes (max: {})",
                addr, len, MAX_UDP_MESSAGE_LEN
            );
            continue;
        }

        // Rate limiting check
        let now = Instant::now();
        let timestamps = rate_tracker.entry(addr).or_insert_with(Vec::new);

        // Remove timestamps older than the rate limit window
        timestamps.retain(|&ts| now.duration_since(ts) < RATE_LIMIT_WINDOW);

        // Check if rate limit exceeded
        if timestamps.len() >= MAX_REQUESTS_PER_SECOND as usize {
            warn!(
                "UDP rate limit exceeded for {}: {} requests in last second",
                addr,
                timestamps.len()
            );
            continue;
        }

        // Record this request
        timestamps.push(now);

        // Clean up old entries from rate tracker periodically
        if rate_tracker.len() > 100 {
            rate_tracker.retain(|_, times| !times.is_empty());
        }

        let msg = String::from_utf8_lossy(&buf[..len]).trim().to_lowercase();

        let (seq, cmd) = match parse_udp_packet(&msg) {
            Ok(packet) => packet,
            Err(reason) => {
                warn!("Ignoring UDP command from {}: {}", addr, reason);
                continue;
            }
        };

        if let Some(seq) = seq {
            if seen_sequence(&mut seq_tracker, addr, seq, Instant::now()) {
                send_udp_ack(&socket, addr, seq).await;
                continue;
            }
        }

        info!("Received UDP command from {}: {:?}", addr, cmd);

        tx.send(cmd)
            .await
            .map_err(|e| error::AppError::Udp(format!("Failed to send command to app: {}", e)))?;

        if let Some(seq) = seq {
            send_udp_ack(&socket, addr, seq).await;
        }
    }
}

fn parse_udp_packet(input: &str) -> Result<(Option<u32>, ControlCommand), String> {
    let words = input.split_whitespace().collect::<Vec<_>>();
    if words.is_empty() {
        return Err("empty command".to_string());
    }

    let (seq, command_words) = if words.first() == Some(&"seq") {
        if words.len() < 3 {
            return Err("sequenced command missing sequence or body".to_string());
        }
        let seq = words[1]
            .parse::<u32>()
            .map_err(|_| format!("invalid sequence: {}", words[1]))?;
        (Some(seq), &words[2..])
    } else {
        (None, &words[..])
    };

    let cmd = match command_words {
        ["play"] => ControlCommand::Play,
        ["stop"] => ControlCommand::Stop,
        ["toggle"] => ControlCommand::Toggle,
        ["toggle", "pause"] => ControlCommand::TogglePause,
        ["volume", "up"] => ControlCommand::VolumeUp,
        ["volume", "down"] => ControlCommand::VolumeDown,
        ["volume", num] => {
            let value = num
                .parse::<f32>()
                .map_err(|_| format!("invalid volume value: {}", num))?;
            if !(0.0..=2.0).contains(&value) {
                return Err(format!("volume value out of range (0.0-2.0): {}", value));
            }
            ControlCommand::SetVolume(value)
        }
        ["tune", "next"] => ControlCommand::TuneNext,
        ["tune", "prev"] => ControlCommand::TunePrev,
        ["tune", id] => {
            if id.len() > 32
                || !id
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
            {
                return Err(format!("invalid station ID format: {}", id));
            }
            ControlCommand::Tune(id.to_string())
        }
        ["select", "up"] => ControlCommand::SelectUp,
        ["select", "down"] => ControlCommand::SelectDown,
        _ => return Err(format!("unknown command: {}", input)),
    };

    Ok((seq, cmd))
}

fn seen_sequence(
    seq_tracker: &mut HashMap<SocketAddr, Vec<(u32, Instant)>>,
    addr: SocketAddr,
    seq: u32,
    now: Instant,
) -> bool {
    const SEQ_RETENTION: Duration = Duration::from_secs(60);
    let entries = seq_tracker.entry(addr).or_default();
    entries.retain(|(_, seen_at)| now.duration_since(*seen_at) < SEQ_RETENTION);
    if entries.iter().any(|(seen_seq, _)| *seen_seq == seq) {
        return true;
    }
    entries.push((seq, now));
    false
}

async fn send_udp_ack(socket: &tokio::net::UdpSocket, addr: SocketAddr, seq: u32) {
    let ack = format!("ok {}", seq);
    if let Err(e) = socket.send_to(ack.as_bytes(), addr).await {
        warn!("Failed to send UDP ack to {}: {}", addr, e);
    }
}

#[cfg(test)]
mod udp_tests {
    use super::*;

    #[test]
    fn parses_plain_toggle_play_stop() {
        assert_eq!(
            parse_udp_packet("toggle").unwrap(),
            (None, ControlCommand::Toggle)
        );
    }

    #[test]
    fn parses_sequenced_toggle_pause() {
        assert_eq!(
            parse_udp_packet("seq 42 toggle pause").unwrap(),
            (Some(42), ControlCommand::TogglePause)
        );
    }

    #[test]
    fn rejects_bad_volume() {
        assert!(parse_udp_packet("volume 9.0").is_err());
        assert!(parse_udp_packet("volume loud").is_err());
    }

    #[test]
    fn detects_duplicate_sequence() {
        let addr = "127.0.0.1:8069".parse().unwrap();
        let mut tracker = HashMap::new();
        let now = Instant::now();
        assert!(!seen_sequence(&mut tracker, addr, 7, now));
        assert!(seen_sequence(&mut tracker, addr, 7, now));
    }
}
