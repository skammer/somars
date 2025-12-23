use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph, ListDirection},
    text::{Line, Span, Text},
};
use crate::{App, PlaybackState, MessageType, t, utils::format_duration};

pub fn ui(f: &mut ratatui::Frame, app: &mut App) {
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
    let sink_len = if let Some(ref sink) = app.sink {
        if let Ok(sink_guard) = sink.lock() {
            sink_guard.len()
        } else {
            0
        }
    } else {
        0
    };

    let bottom_controls = Line::from(vec![
        Span::styled("q", Style::default().fg(Color::Yellow).add_modifier(ratatui::style::Modifier::BOLD)),
        Span::raw(format!(":{} ", t("controls-quit"))),
        Span::styled("↵", Style::default().fg(Color::Green).add_modifier(ratatui::style::Modifier::BOLD)),
        Span::raw(format!(":{} ", t("controls-play"))),
        Span::styled("Space", Style::default().fg(Color::Blue).add_modifier(ratatui::style::Modifier::BOLD)),
        Span::raw(format!(":{}: {}/{} ", t("controls-pause"), t("controls-stop"), t("controls-start"))),
        Span::styled("+/-", Style::default().fg(Color::Cyan).add_modifier(ratatui::style::Modifier::BOLD)),
        Span::raw(format!(":{} ", t("controls-volume"))),
        Span::styled("?", Style::default().fg(Color::Magenta).add_modifier(ratatui::style::Modifier::BOLD)),
        Span::raw(format!(":{} ", t("controls-help"))),
        Span::raw("  "),
        Span::styled(format!("Sink: {}", sink_len), Style::default().fg(Color::Cyan)),
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
            Span::raw(format!(" ({:.0}%)", app.volume * 100.0)),
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
                Span::raw(format!(" {}", t("loading-stations"))),
            ]),
        ];
        let loading_para = Paragraph::new(loading_text)
            .block(Block::default().borders(Borders::ALL).title(t("loading")).padding(ratatui::widgets::Padding::new(1, 1, 0, 0)))
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
                    .title(Line::from(t("stations")))
                    .title(Line::from("[↓↑]").right_aligned())
                    .title_bottom(Line::from(format!("[{} / {}]", selected_pos, total_stations)).right_aligned())
                    .padding(ratatui::widgets::Padding::new(1, 1, 0, 0))
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
                    Span::styled(format!("{}: ", t("station-id")), Style::default().fg(Color::Yellow)),
                    Span::raw(&station.id),
                ]),
                Line::from(vec![
                    Span::styled(format!("{}: ", t("station-title")), Style::default().fg(Color::Yellow)),
                    Span::raw(&station.title),
                ]),
                Line::from(vec![
                    Span::styled(format!("{}: ", t("station-genre")), Style::default().fg(Color::Yellow)),
                    Span::raw(&station.genre),
                ]),
                Line::from(vec![
                    Span::styled(format!("{}: ", t("station-dj")), Style::default().fg(Color::Yellow)),
                    Span::raw(&station.dj),
                ]),
                Line::from(""),
                Line::from(Span::raw(&station.description)),
                Line::from(""),
                Line::from(vec![
                    Span::styled(format!("{}: ", t("playback-time")), Style::default().fg(Color::Yellow)),
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
            Paragraph::new(vec![Line::from(t("no-station-selected"))])
        }
    } else {
        Paragraph::new(vec![Line::from(t("no-station-selected"))])
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
                        PlaybackState::Playing => t("playing"),
                        PlaybackState::Paused => t("paused"),
                        PlaybackState::Stopped => t("stopped"),
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
                format!("[{}: {:.0}%]", t("volume"), app.volume * 100.0)
                ).centered()
            )
        .padding(ratatui::widgets::Padding::new(1, 1, 0, 0)));
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
        .filter(|msg| app.log_level > 1 || matches!(msg.message_type, MessageType::Error | MessageType::Info | MessageType::Playback))
        .count();
    let history_list = List::new(history_items).direction(ListDirection::BottomToTop)
        .block(Block::default()
            .borders(Borders::ALL)
            .title(t("history"))
            .title(Line::from("[jk]").right_aligned())
            .title_bottom(Line::from(format!("[{} / {}]", selected_history_pos, total_history)).right_aligned())
            .padding(ratatui::widgets::Padding::new(1, 1, 0, 0))
        )
        .highlight_style(Style::default().add_modifier(ratatui::style::Modifier::ITALIC).add_modifier(ratatui::style::Modifier::UNDERLINED));
    f.render_stateful_widget(history_list, right_chunks[1], &mut app.history_scroll_state);

    if app.show_help {
        let help_text = vec![
            Line::from(vec![
                Span::styled(format!("{} - {}", env!("CARGO_PKG_NAME"), t("app-description")), 
                    Style::default().add_modifier(ratatui::style::Modifier::BOLD))
            ]),
            Line::from(""),
            Line::from(t("help-keyboard")),
            Line::from(""),
            Line::from(vec![
                Span::styled("↵ (Enter)", Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
                Span::raw(format!(" - {}", t("help-enter")))
            ]),
            Line::from(vec![
                Span::styled("Space", Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
                Span::raw(format!(" - {} ({}/{})", t("help-space"), t("controls-pause"), t("controls-stop")))
            ]),
            Line::from(vec![
                Span::styled("+/-", Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
                Span::raw(format!(" - {}", t("help-volume")))
            ]),
            Line::from(vec![
                Span::styled("↑/↓", Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
                Span::raw(format!(" - {}", t("help-arrows")))
            ]),
            Line::from(vec![
                Span::styled("q", Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
                Span::raw(format!(" - {}", t("help-quit")))
            ]),
            Line::from(vec![
                Span::styled("?", Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
                Span::raw(format!(" - {}", t("help-toggle-help")))
            ]),
            Line::from(""),
            Line::from(t("help-cli")),
            Line::from(""),
            Line::from(vec![
                Span::styled("--log-level <1|2>", Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
                Span::raw(format!(" - {}", t("help-log-level")))
            ]),
            Line::from(vec![
                Span::styled("--station <ID>", Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
                Span::raw(format!(" - {}", t("help-station")))
            ]),
            Line::from(vec![
                Span::styled("--listen", Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
                Span::raw(format!(" - {}", t("help-listen")))
            ]),
            Line::from(vec![
                Span::styled("--port <NUM>", Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
                Span::raw(format!(" - {}", t("help-port")))
            ]),
            Line::from(vec![
                Span::styled("--help", Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
                Span::raw(format!(" - {}", t("help-show-help")))
            ]),
            Line::from(vec![
                Span::styled("--version", Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
                Span::raw(format!(" - {}", t("help-version")))
            ]),
            Line::from(vec![
                Span::styled("--broadcast <MSG>", Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
                Span::raw(format!(" - {}", t("help-broadcast")))
            ]),
            Line::from(vec![
                Span::styled("--locale <LOCALE>", Style::default().add_modifier(ratatui::style::Modifier::BOLD)),
                Span::raw(format!(" - {}", t("help-locale")))
            ]),
            Line::from(""),
            Line::from(t("help-close")),
        ];

        let area = crate::ui::popup_area(f.area(), 60, 60);
        let help_widget = Paragraph::new(help_text)
            .block(Block::default()
                .title(t("help-title"))
                .title_bottom(Line::from(format!("{} v{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))).right_aligned())
                .borders(Borders::ALL)
                .border_type(ratatui::widgets::BorderType::Double)
                .padding(ratatui::widgets::Padding::new(1, 1, 0, 0)))
            .alignment(ratatui::layout::Alignment::Left)
            .wrap(ratatui::widgets::Wrap { trim: true });

        f.render_widget(ratatui::widgets::Clear, area);
        f.render_widget(help_widget, area);
    }
}

/// helper function to create a centered rect using up certain percentage of the available rect `r`
pub fn popup_area(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let vertical = Layout::vertical([Constraint::Percentage(percent_y)]).flex(ratatui::layout::Flex::Center);
    let horizontal = Layout::horizontal([Constraint::Percentage(percent_x)]).flex(ratatui::layout::Flex::Center);
    let [area] = vertical.areas(area);
    let [area] = horizontal.areas(area);
    area
}