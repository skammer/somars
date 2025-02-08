use anyhow::{anyhow, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use futures::StreamExt;
use ratatui::widgets::ListState;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};
use rodio::{Decoder, OutputStream, Sink};
use serde::Deserialize;
use std::{
    io,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Debug, Deserialize)]
struct Channel {
    title: String,
    description: String,
    dj: String,
    playlist: String,
    songs: Vec<Song>,
}

#[derive(Debug, Deserialize, Clone)]
struct Song {
    artist: String,
    title: String,
}

struct App {
    channels: Vec<Channel>,
    selected_channel: usize,
    sink: Option<Arc<Mutex<Sink>>>,
    current_song: Option<Song>,
    song_history: Vec<Song>,
    playback_status: PlaybackStatus,
}

enum PlaybackStatus {
    Playing,
    Paused,
    Stopped,
}

impl App {
    async fn new() -> Result<Self> {
        let channels = fetch_channels().await?;
        Ok(Self {
            channels,
            selected_channel: 0,
            sink: None,
            current_song: None,
            song_history: Vec::new(),
            playback_status: PlaybackStatus::Stopped,
        })
    }

    async fn select_channel(&mut self) -> Result<()> {
        if let Some(sink) = &self.sink {
            sink.lock().unwrap().stop();
        }

        let channel = &self.channels[self.selected_channel];
        self.current_song = channel.songs.first().cloned();
        if let Some(song) = &self.current_song {
            self.song_history.insert(0, song.clone());
        }

        let (_stream, stream_handle) = OutputStream::try_default()?;
        let sink = Sink::try_new(&stream_handle)?;

        let response = reqwest::get(&channel.playlist).await?;
        let content = response.bytes().await?;
        let source = Decoder::new(io::Cursor::new(content))?;

        sink.append(source);
        self.sink = Some(Arc::new(Mutex::new(sink)));
        self.playback_status = PlaybackStatus::Playing;

        Ok(())
    }

    fn toggle_playback(&mut self) {
        if let Some(sink) = &self.sink {
            let sink = sink.lock().unwrap();
            if sink.is_paused() {
                sink.play();
                self.playback_status = PlaybackStatus::Playing;
            } else {
                sink.pause();
                self.playback_status = PlaybackStatus::Paused;
            }
        }
    }
}

async fn fetch_channels() -> Result<Vec<Channel>> {
    let response = reqwest::get("https://somafm.com/channels.json").await?;
    let channels = response.json::<Vec<Channel>>().await?;
    Ok(channels)
}


fn ui(f: &mut Frame<CrosstermBackend<std::io::Stdout>>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)].as_ref())
        .split(f.size());

    // Left panel: Channel list
    let channels_list: Vec<ListItem> = app
        .channels
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let style = if i == app.selected_channel {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };
            ListItem::new(c.title.clone()).style(style)
        })
        .collect();

    let list = List::new(channels_list)
        .block(Block::default().borders(Borders::ALL).title("Channels"))
        .highlight_style(Style::default().bg(Color::DarkGray));

    f.render_stateful_widget(list, chunks[0], &mut ListState::default());

    // Right panel: Playback info
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(5),
            Constraint::Min(0),
        ])
        .split(chunks[1]);

    // Playback controls
    let status = match app.playback_status {
        PlaybackStatus::Playing => "▶ Playing",
        PlaybackStatus::Paused => "⏸ Paused",
        PlaybackStatus::Stopped => "⏹ Stopped",
    };
    let controls = Paragraph::new(format!(
        "{}\n\n[Space] Play/Pause | [Q] Quit",
        status
    ))
    .block(Block::default().borders(Borders::ALL).title("Controls"));
    f.render_widget(controls, right_chunks[0]);

    // Current song
    let current_song = app.current_song.as_ref().map_or(
        "No song playing".to_string(),
        |s| format!("Artist: {}\nTitle: {}", s.artist, s.title),
    );
    let song_block = Paragraph::new(current_song)
        .block(Block::default().borders(Borders::ALL).title("Now Playing"));
    f.render_widget(song_block, right_chunks[1]);

    // Song history
    let history_items: Vec<ListItem> = app
        .song_history
        .iter()
        .map(|s| ListItem::new(format!("{} - {}", s.artist, s.title)))
        .collect();
    let history = List::new(history_items)
        .block(Block::default().borders(Borders::ALL).title("History"))
        .highlight_style(Style::default().bg(Color::DarkGray));
    f.render_widget(history, right_chunks[2]);
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;

    let mut app = App::new().await?;
    app.select_channel().await?;

    loop {
        terminal.draw(|f| ui(f, &app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Down => {
                            if app.selected_channel < app.channels.len() - 1 {
                                app.selected_channel += 1;
                                app.select_channel().await?;
                            }
                        }
                        KeyCode::Up => {
                            if app.selected_channel > 0 {
                                app.selected_channel -= 1;
                                app.select_channel().await?;
                            }
                        }
                        KeyCode::Char(' ') => app.toggle_playback(),
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(())
}
