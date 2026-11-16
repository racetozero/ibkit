use super::{App, Modal, View, form::Kind, log::Level};
use chrono::{DateTime, Local, Utc};
use ibkit_gateway::{State, TradingMode, recovery::RecoveryState, settings::SECTIONS};
use ratatui::{
    prelude::*,
    widgets::{
        Block, BorderType, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Wrap,
    },
};

pub const MIN_WIDTH: u16 = 76;
pub const MIN_HEIGHT: u16 = 22;
// Fixed 256-color values. The 16 theme colors change with the terminal
// theme (Solarized draws bright black, Color::DarkGray, in the background
// color), so only primary text uses the theme's own foreground.
const TEXT: Color = Color::Reset;
const MUTED: Color = Color::Indexed(245);
const LINE: Color = Color::Indexed(240);
const ACCENT: Color = Color::Indexed(214);
const GOOD: Color = Color::Indexed(35);
const WARN: Color = Color::Indexed(172);
const BAD: Color = Color::Indexed(167);
const PURPLE: Color = Color::Indexed(140);
const TEAL: Color = Color::Indexed(37);
/// Text on a colored badge.
const INK: Color = Color::Indexed(16);
const WHITE: Color = Color::Indexed(231);
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

fn clean(value: &str) -> String {
    value
        .chars()
        .map(|value| if value.is_control() { ' ' } else { value })
        .collect()
}

fn pane(focus: bool) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if focus { ACCENT } else { LINE }))
        .title_style(Style::default().fg(TEXT).bold())
}

fn badge(text: String, background: Color, foreground: Color) -> Span<'static> {
    Span::styled(text, Style::default().bg(background).fg(foreground).bold())
}

/// The selection mark that starts each list row.
fn mark(selected: bool) -> Span<'static> {
    Span::styled(
        if selected { "› " } else { "  " },
        Style::default().fg(ACCENT).bold(),
    )
}

fn scrollbar() -> Scrollbar<'static> {
    Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .track_symbol(Some("│"))
        .thumb_symbol("┃")
        .begin_symbol(None)
        .end_symbol(None)
        .thumb_style(Style::default().fg(MUTED))
        .track_style(Style::default().fg(LINE))
}

/// "12s", "3m 20s", "2h 14m" or "3d 4h".
fn duration(seconds: i64) -> String {
    let seconds = seconds.max(0);
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m {}s", seconds / 60, seconds % 60),
        3600..86400 => format!("{}h {}m", seconds / 3600, seconds % 3600 / 60),
        _ => format!("{}d {}h", seconds / 86400, seconds % 86400 / 3600),
    }
}

fn clock(at: DateTime<Utc>) -> String {
    at.with_timezone(&Local).format("%H:%M:%S").to_string()
}

/// Keep the end of `text`, which holds the file name of a path.
fn tail(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count <= width {
        return text.into();
    }
    let skip = count + 1 - width.max(1);
    format!("…{}", text.chars().skip(skip).collect::<String>())
}

pub fn state_name(state: State) -> &'static str {
    match state {
        State::Starting => "Starting",
        State::LoggingIn => "Logging in",
        State::AwaitingSecondFactor => "Awaiting IB Key",
        State::Configuring => "Configuring",
        State::Ready => "Ready",
        State::Restarting => "Restarting",
        State::Stopping => "Stopping",
        State::Exited => "Exited",
        State::Failed => "Failed",
        State::Recovering => "Recovering",
        State::Halted => "Halted",
        State::Paused => "Paused",
    }
}

/// The session state as the operator sees it: name, color, and whether it
/// changes by itself (a spinner shows that).
fn session_state(app: &App) -> (&'static str, Color, bool) {
    if !app.active() {
        return ("Stopped", LINE, false);
    }
    if app.stopping() {
        return ("Stopping", WARN, true);
    }
    let Some(status) = &app.status else {
        return ("Starting", WARN, true);
    };
    let color = match status.state {
        State::Ready => GOOD,
        State::Failed | State::Halted => BAD,
        State::Recovering | State::Paused => PURPLE,
        State::AwaitingSecondFactor => TEAL,
        State::Exited => LINE,
        _ => WARN,
    };
    let moving = !matches!(
        status.state,
        State::Ready | State::Failed | State::Halted | State::Paused | State::Exited
    );
    (state_name(status.state), color, moving)
}

fn spinner() -> &'static str {
    let tenths = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        / 100;
    SPINNER[tenths as usize % SPINNER.len()]
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        frame.render_widget(Paragraph::new(format!("Enlarge the terminal to at least {MIN_WIDTH} columns and {MIN_HEIGHT} rows. Ctrl-C stops Gateway and exits.")).wrap(Wrap { trim: true }), area);
        return;
    }
    let message = clean(&app.message);
    let message_rows = ((message.chars().count() + 2) as u16)
        .div_ceil(area.width)
        .clamp(1, 4);
    let [header, body, result, keys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(message_rows),
        Constraint::Length(1),
    ])
    .areas(area);
    draw_header(frame, header, app);
    match app.view {
        View::Overview => draw_overview(frame, body, app),
        View::Settings => draw_settings(frame, body, app),
    }
    let (icon, color) = if app.error {
        ("✗ ", BAD)
    } else {
        ("› ", TEXT)
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                icon,
                Style::default().fg(if app.error { BAD } else { MUTED }),
            ),
            Span::styled(message, Style::default().fg(color)),
        ]))
        .wrap(Wrap { trim: true }),
        result,
    );
    draw_keys(frame, keys, app);
    draw_modal(frame, area, app);
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let mut left = vec![
        Span::styled(" ◆", Style::default().fg(ACCENT).bold()),
        Span::styled(" ibkit   ", Style::default().fg(TEXT).bold()),
    ];
    for (view, name) in [
        (View::Overview, " Overview "),
        (View::Settings, " Settings "),
    ] {
        left.push(if app.view == view {
            badge(name.into(), ACCENT, INK)
        } else {
            Span::styled(name, Style::default().fg(MUTED))
        });
        left.push(Span::raw(" "));
    }
    frame.render_widget(Paragraph::new(Line::from(left)), area);
    let (name, color, moving) = session_state(app);
    let mode = app
        .active_mode()
        .unwrap_or(if app.form.value("trading_mode") == "live" {
            TradingMode::Live
        } else {
            TradingMode::Paper
        });
    let mut right = Vec::new();
    if app.form.dirty {
        right.push(Span::styled("● unsaved   ", Style::default().fg(WARN)));
    }
    right.push(badge(
        format!(" {} {name} ", if moving { spinner() } else { "●" }),
        color,
        if color == LINE { WHITE } else { INK },
    ));
    right.push(Span::raw(" "));
    right.push(match mode {
        TradingMode::Live => badge(" LIVE ".into(), Color::Indexed(160), WHITE),
        TradingMode::Paper => badge(" PAPER ".into(), Color::Indexed(25), WHITE),
    });
    right.push(Span::raw(" "));
    frame.render_widget(Paragraph::new(Line::from(right)).right_aligned(), area);
}

fn draw_overview(frame: &mut Frame, area: Rect, app: &mut App) {
    let status = app.status.as_ref().filter(|_| app.active());
    let error = status
        .filter(|status| status.last_start_result.has_error())
        .map(|status| {
            let text = &status.last_start_result.error_message;
            // Credentials from files or the environment are not in the form.
            let text = app
                .session
                .as_ref()
                .map_or(text.clone(), |session| session.credentials.redact(text));
            clean(&app.form.redact(&text))
        });
    let note = match status.map(|status| status.state) {
        Some(State::AwaitingSecondFactor) => "Approve the IB Key request on your phone.",
        Some(State::Recovering) => "The controller retries under the recovery settings.",
        Some(State::Paused) => "Paused by pause or closedown_at. Press c to log in again.",
        Some(State::Halted) => {
            "Halted: correct the cause below, then press c. Automatic retries stay off for this fault."
        }
        Some(State::Ready) => "Gateway is ready. The controller checks the API and restarts daily.",
        _ if app.stopping() => "Gateway stops. Wait for the controller to exit.",
        _ => "Gateway starts. Settings changes apply after x (stop) and r (start).",
    };
    let inner_width = area.width.saturating_sub(2).max(1);
    let rows = |text: &str| ((text.chars().count() + 2) as u16).div_ceil(inner_width);
    let error_rows = error.as_ref().map_or(0, |error| rows(error).min(4));
    let [card, log] = Layout::vertical([
        Constraint::Length(if app.active() {
            5 + rows(note) + error_rows
        } else {
            4
        }),
        Constraint::Min(3),
    ])
    .areas(area);
    let block = pane(false).title(" Session ");
    let inside = block.inner(card);
    frame.render_widget(block, card);
    if !app.active() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled("Gateway is stopped.", Style::default().bold()),
                Line::styled(
                    "Press r to start, or Tab to edit the settings. The log keeps the events of earlier runs.",
                    Style::default().fg(MUTED),
                ),
            ])
            .wrap(Wrap { trim: true }),
            inside,
        );
    } else {
        let config = app.session.as_ref().map(|session| &session.config);
        let (name, color, moving) = session_state(app);
        let label =
            |text: &'static str| Span::styled(format!("{text:<10}"), Style::default().fg(MUTED));
        let since = app
            .since
            .filter(|_| status.is_some())
            .map_or(String::new(), |(_, at)| {
                format!("  for {}", duration(at.elapsed().as_secs() as i64))
            });
        let api = config.map_or(String::new(), |config| match config.api_listen {
            Some(listen) => format!("127.0.0.1:{} ← {listen}", config.api_port),
            None => format!("127.0.0.1:{}", config.api_port),
        });
        let now = Utc::now();
        let (recovery, recovery_color) = match status.and_then(|status| status.recovery) {
            // Running: a launch is under way, the first one included.
            None | Some(RecoveryState::Idle | RecoveryState::Running) => {
                ("none".to_string(), MUTED)
            }
            Some(RecoveryState::AwaitingAgent { deadline }) => (
                format!("waiting for the agent until {}", clock(deadline)),
                PURPLE,
            ),
            Some(RecoveryState::Waiting { retry_at, .. }) => (
                format!(
                    "retry at {} (in {})",
                    clock(retry_at),
                    duration((retry_at - now).num_seconds())
                ),
                PURPLE,
            ),
            Some(RecoveryState::Exhausted) => ("hourly attempt budget used".into(), BAD),
            Some(RecoveryState::Blocked { .. }) => ("blocked until you act".into(), BAD),
        };
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)]).areas(
                Rect {
                    height: 3,
                    ..inside
                },
            );
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(vec![
                    label("State"),
                    Span::styled(
                        format!("{} {name}", if moving { spinner() } else { "●" }),
                        Style::default().fg(color).bold(),
                    ),
                    Span::styled(since, Style::default().fg(MUTED)),
                ]),
                Line::from(vec![
                    label("Session"),
                    Span::raw(status.map_or("starting".into(), |status| {
                        format!(
                            "{} · pid {}",
                            status.session.chars().take(8).collect::<String>(),
                            status.controller_pid
                        )
                    })),
                ]),
                Line::from(vec![
                    label("Restart"),
                    Span::raw(config.map_or(String::new(), |config| {
                        format!("daily at {}", config.restart_time)
                    })),
                ]),
            ]),
            left,
        );
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(vec![
                    label("API"),
                    Span::raw(api),
                    if app.api_down {
                        Span::styled("  not answering", Style::default().fg(BAD))
                    } else {
                        Span::raw("")
                    },
                ]),
                Line::from(vec![
                    label("Recovery"),
                    Span::styled(recovery, Style::default().fg(recovery_color)),
                ]),
                Line::from(vec![
                    label("Updated"),
                    Span::raw(status.map_or("no status yet".into(), |status| {
                        format!("{} ago", duration((now - status.at).num_seconds()))
                    })),
                ]),
            ]),
            right,
        );
        let mut lines = vec![Line::from(vec![
            Span::styled("→ ", Style::default().fg(ACCENT)),
            Span::raw(note),
        ])];
        if let Some(error) = error {
            lines.push(Line::from(vec![
                Span::styled("✗ ", Style::default().fg(BAD)),
                Span::styled(error, Style::default().fg(BAD)),
            ]));
        }
        frame.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: true }),
            Rect {
                y: inside.y + 3,
                height: inside.height.saturating_sub(3),
                ..inside
            },
        );
    }
    draw_log(frame, log, app);
}

fn draw_log(frame: &mut Frame, area: Rect, app: &mut App) {
    let log = &app.log;
    let following = log.following();
    let (items, position, count) = {
        let view = log.view();
        let position = log.position(&view);
        let items: Vec<ListItem> = view
            .iter()
            .enumerate()
            .map(|(at, entry)| {
                let selected = !following && Some(at) == position;
                let (icon, color) = match entry.level {
                    Level::Info => ("•", MUTED),
                    Level::Good => ("✓", GOOD),
                    Level::Warn => ("▲", WARN),
                    Level::Error => ("✗", BAD),
                };
                let mut text = Style::default().fg(if entry.level == Level::Error {
                    BAD
                } else {
                    TEXT
                });
                if selected {
                    text = text.bold();
                }
                let mut line = vec![
                    mark(selected),
                    Span::styled(
                        entry.at.format("%H:%M:%S ").to_string(),
                        Style::default().fg(MUTED),
                    ),
                    Span::styled(format!("{icon} "), Style::default().fg(color)),
                    Span::styled(entry.text.clone(), text),
                ];
                if entry.repeats > 1 {
                    line.push(Span::styled(
                        format!("  ×{}", entry.repeats),
                        Style::default().fg(MUTED),
                    ));
                }
                ListItem::new(Line::from(line))
            })
            .collect();
        (items, position, view.len())
    };
    let mut title = vec![
        Span::raw(" Events "),
        Span::styled(format!("{count} "), Style::default().fg(MUTED).not_bold()),
    ];
    if log.problems {
        title.push(Span::styled("problems only ", Style::default().fg(WARN)));
    }
    if log.typing || !log.filter.is_empty() {
        title.push(Span::styled(
            format!(
                "/{}{} ",
                clean(&log.filter),
                if log.typing { "▏" } else { "" }
            ),
            Style::default().fg(ACCENT),
        ));
    }
    let place = if following {
        Span::styled(" ● following ", Style::default().fg(GOOD))
    } else {
        Span::styled(
            format!(
                " ↑ {}/{count} · End follows ",
                position.map_or(0, |at| at + 1)
            ),
            Style::default().fg(ACCENT),
        )
    };
    let block = pane(true)
        .title(Line::from(title))
        .title_top(Line::from(place).right_aligned());
    let inner = block.inner(area);
    app.log.page = inner.height as usize;
    if count == 0 {
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new(if app.log.len() == 0 {
                "No events yet. Controller events and log lines appear here."
            } else {
                "No events match the filter. Press Esc to clear it."
            })
            .style(Style::default().fg(MUTED))
            .centered(),
            Rect {
                y: inner.y + inner.height / 2,
                height: 1,
                ..inner
            },
        );
        return;
    }
    app.log.list.select(position);
    // Fill the pane after a resize; follow mode keeps the newest row last.
    let bottom = count.saturating_sub(inner.height as usize);
    if following || app.log.list.offset() > bottom {
        *app.log.list.offset_mut() = bottom;
    }
    frame.render_stateful_widget(List::new(items).block(block), area, &mut app.log.list);
    if count > inner.height as usize {
        frame.render_stateful_widget(
            scrollbar(),
            area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut ScrollbarState::new(count).position(position.unwrap_or(0)),
        );
    }
}

fn draw_settings(frame: &mut Frame, area: Rect, app: &mut App) {
    let wide = area.width >= 110;
    let [nav, main] = Layout::horizontal([Constraint::Length(14), Constraint::Min(0)]).areas(area);
    let [fields_area, help_area] = if wide {
        Layout::horizontal([Constraint::Min(0), Constraint::Length(36)]).areas(main)
    } else {
        Layout::vertical([Constraint::Min(0), Constraint::Length(7)]).areas(main)
    };
    let invalid = |index: usize| {
        let field = &app.form.fields[index];
        !field.key.starts_with('_')
            && ibkit_gateway::settings::parse_value(field.key, &field.value).is_err()
    };
    let sections: Vec<ListItem> = SECTIONS
        .iter()
        .enumerate()
        .map(|(section, name)| {
            let bad = (0..app.form.fields.len())
                .any(|index| app.form.fields[index].section == section && invalid(index));
            let selected = section == app.form.section;
            ListItem::new(Line::from(vec![
                mark(selected),
                Span::styled(
                    *name,
                    if selected {
                        Style::default().fg(ACCENT).bold()
                    } else {
                        Style::default().fg(TEXT)
                    },
                ),
                if bad {
                    Span::styled(" !", Style::default().fg(BAD).bold())
                } else {
                    Span::raw("")
                },
            ]))
        })
        .collect();
    frame.render_widget(
        List::new(sections).block(pane(false).title(" Sections ")),
        nav,
    );

    let rows = app.form.rows();
    let block = pane(app.modal.is_none() || matches!(app.modal, Some(Modal::Edit(_))))
        .title(format!(" {} ", SECTIONS[app.form.section]))
        .title_top(
            Line::styled(
                format!(" {}/{} ", app.form.selected + 1, rows.len()),
                Style::default().fg(MUTED),
            )
            .right_aligned(),
        );
    let inner = block.inner(fields_area);
    let label_width = rows
        .iter()
        .map(|&index| app.form.fields[index].label.chars().count())
        .max()
        .unwrap_or(0)
        .min(32);
    // The value column: after the selection mark, the label and a gap; the
    // scrollbar takes the last column.
    let value_width = (inner.width as usize)
        .saturating_sub(2 + label_width + 2 + 1)
        .max(4);
    let editor = match &app.modal {
        Some(Modal::Edit(editor)) => Some(editor),
        _ => None,
    };
    let mut cursor = None;
    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(row, &index)| {
            let field = &app.form.fields[index];
            let selected = row == app.form.selected;
            let label = Span::styled(
                format!(
                    "{:<label_width$}  ",
                    if field.label.chars().count() > label_width {
                        field
                            .label
                            .chars()
                            .take(label_width - 1)
                            .chain(['…'])
                            .collect()
                    } else {
                        field.label.to_string()
                    }
                ),
                if selected {
                    Style::default().fg(ACCENT).bold()
                } else {
                    Style::default().fg(TEXT)
                },
            );
            let value = match (editor.filter(|editor| editor.field == index), &field.kind) {
                (Some(editor), kind) => {
                    let text: Vec<char> = if matches!(kind, Kind::Secret) {
                        vec!['•'; editor.text.chars().count()]
                    } else {
                        clean(&app.form.redact(&editor.text)).chars().collect()
                    };
                    let start = editor.cursor.saturating_sub(value_width - 1);
                    cursor = Some(editor.cursor - start);
                    // A scrolled value starts with … in place of its hidden start.
                    let shown: String = text
                        .iter()
                        .enumerate()
                        .skip(start)
                        .take(value_width)
                        .map(|(at, &ch)| if start > 0 && at == start { '…' } else { ch })
                        .collect();
                    Span::styled(shown, Style::default().fg(ACCENT))
                }
                (None, Kind::Secret) if field.value.is_empty() => {
                    Span::styled("not entered", Style::default().fg(MUTED))
                }
                (None, Kind::Secret) => Span::raw("••••••••"),
                (None, Kind::Choice(["false", "true"])) if field.value == "true" => {
                    Span::styled("● on", Style::default().fg(GOOD))
                }
                (None, Kind::Choice(["false", "true"])) => {
                    Span::styled("○ off", Style::default().fg(MUTED))
                }
                (None, Kind::Choice(_)) => {
                    Span::styled(field.value.clone(), Style::default().fg(TEAL))
                }
                (None, _) if field.value.is_empty() => {
                    Span::styled("not set", Style::default().fg(MUTED))
                }
                (None, _) => Span::styled(
                    tail(&clean(&app.form.redact(&field.value)), value_width),
                    if invalid(index) {
                        Style::default().fg(BAD)
                    } else {
                        Style::default()
                    },
                ),
            };
            ListItem::new(Line::from(vec![mark(selected), label, value]))
        })
        .collect();
    app.fields.select(Some(app.form.selected));
    frame.render_stateful_widget(List::new(items).block(block), fields_area, &mut app.fields);
    if let Some(column) = cursor {
        frame.set_cursor_position(Position {
            x: inner.x + (2 + label_width + 2 + column) as u16,
            y: inner.y + (app.form.selected - app.fields.offset()) as u16,
        });
    }
    if rows.len() > inner.height as usize {
        frame.render_stateful_widget(
            scrollbar(),
            fields_area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut ScrollbarState::new(rows.len()).position(app.form.selected),
        );
    }

    let field = &app.form.fields[app.form.current()];
    let mut help = vec![Line::styled(field.label, Style::default().bold())];
    // The list cuts long values such as paths; show this one in full.
    if !matches!(field.kind, Kind::Secret) && field.value.chars().count() > value_width {
        help.push(Line::styled(
            clean(&app.form.redact(&field.value)),
            Style::default().fg(TEAL),
        ));
    }
    help.push(Line::styled(field.help, Style::default().fg(TEXT)));
    if !field.key.starts_with('_') {
        help.push(Line::from(vec![
            Span::styled("config key ", Style::default().fg(MUTED)),
            Span::styled(field.key, Style::default().fg(TEAL)),
        ]));
    }
    if !field.key.starts_with('_')
        && let Err(error) = ibkit_gateway::settings::parse_value(field.key, &field.value)
    {
        help.push(Line::styled(
            format!("✗ {}", clean(&app.form.redact(&format!("{error:#}")))),
            Style::default().fg(BAD),
        ));
    }
    help.push(Line::raw(""));
    help.push(match app.form.config() {
        Ok(_) => Line::styled("✓ All settings are valid.", Style::default().fg(GOOD)),
        Err(error) => Line::styled(
            format!("✗ {}", clean(&app.form.redact(&format!("{error:#}")))),
            Style::default().fg(BAD),
        ),
    });
    help.push(Line::styled(
        format!("file {}", clean(&app.path.display().to_string())),
        Style::default().fg(MUTED),
    ));
    frame.render_widget(
        Paragraph::new(help)
            .wrap(Wrap { trim: true })
            .block(pane(false).title(" Help ")),
        help_area,
    );
}

fn draw_keys(frame: &mut Frame, area: Rect, app: &App) {
    let keys: Vec<(&str, &str)> = if matches!(app.modal, Some(Modal::Edit(_))) {
        vec![
            ("Enter", "apply"),
            ("Esc", "cancel"),
            ("Ctrl-U", "clear"),
            ("←→", "move"),
        ]
    } else if matches!(app.modal, Some(Modal::Live | Modal::Quit)) {
        vec![("Enter", "confirm"), ("Esc", "cancel")]
    } else if app.modal.is_some() {
        vec![("any key", "close")]
    } else if app.view == View::Overview && app.log.typing {
        vec![("type", "filter"), ("Enter", "keep"), ("Esc", "clear")]
    } else {
        let mut keys = if app.active() {
            vec![("x", "stop"), ("c", "restart")]
        } else {
            vec![("r", "start")]
        };
        keys.extend(match app.view {
            View::Overview => [
                ("↑↓", "scroll"),
                ("Enter", "details"),
                ("/", "filter"),
                ("Tab", "settings"),
            ],
            View::Settings => [
                ("↑↓", "field"),
                ("←→", "section"),
                ("Enter", "edit"),
                ("Tab", "overview"),
            ],
        });
        keys.extend([("s", "save"), ("?", "keys"), ("q", "quit")]);
        if app.view == View::Overview {
            keys.push(("e", "problems"));
        }
        if app.active() {
            keys.extend([
                ("t", "soft restart"),
                ("n", "reconnect"),
                ("l", "export logs"),
            ]);
        }
        keys
    };
    let mut spans = Vec::new();
    let mut width = 1;
    for (key, action) in keys {
        let size = key.chars().count() + action.chars().count() + 4;
        if width + size > area.width as usize {
            break;
        }
        width += size;
        spans.push(Span::styled(
            format!(" {key}"),
            Style::default().fg(ACCENT).bold(),
        ));
        spans.push(Span::styled(
            format!(" {action}  "),
            Style::default().fg(MUTED),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_modal(frame: &mut Frame, area: Rect, app: &App) {
    let (title, lines, color, width): (String, Vec<Line>, Color, u16) = match &app.modal {
        None | Some(Modal::Edit(_)) => return,
        Some(Modal::Live) => (
            " Start LIVE trading ".into(),
            vec![
                Line::raw(
                    "These settings connect Gateway to your LIVE account. Orders sent through the API trade real money.",
                ),
                Line::raw(""),
                Line::raw("Check the API permissions and the port before you continue."),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("Enter", Style::default().fg(BAD).bold()),
                    Span::raw(" start live   "),
                    Span::styled("Esc", Style::default().bold()),
                    Span::raw(" cancel"),
                ]),
            ],
            BAD,
            70,
        ),
        Some(Modal::Quit) => (
            " Quit ".into(),
            vec![
                Line::raw(if app.active() {
                    "Gateway stops before this terminal closes."
                } else {
                    "Unsaved settings and session-only credentials are discarded."
                }),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("Enter", Style::default().fg(ACCENT).bold()),
                    Span::raw(" stop and quit   "),
                    Span::styled("Esc", Style::default().bold()),
                    Span::raw(" stay"),
                ]),
            ],
            ACCENT,
            64,
        ),
        Some(Modal::Keys) => {
            let group = |name: &'static str, keys: &[(&'static str, &'static str)]| {
                let mut lines = vec![Line::styled(name, Style::default().bold())];
                lines.extend(keys.iter().map(|(key, action)| {
                    Line::from(vec![
                        Span::styled(format!("  {key:<14}"), Style::default().fg(ACCENT)),
                        Span::styled(*action, Style::default().fg(TEXT)),
                    ])
                }));
                lines
            };
            let mut lines = group(
                "Session",
                &[
                    ("r", "start Gateway with these settings"),
                    ("x", "stop Gateway"),
                    ("c / t", "restart (new login) / soft restart"),
                    ("n / l", "reconnect account / export Gateway logs"),
                ],
            );
            lines.extend(group(
                "Views",
                &[
                    ("Tab  1  2", "overview and event log / settings"),
                    ("s", "save settings to the config file"),
                    ("q  Esc", "quit (Gateway stops first)"),
                ],
            ));
            lines.extend(group(
                "Event log",
                &[
                    ("↑↓ j k", "select; PgUp PgDn Home End (g G)"),
                    ("Enter", "show the selected event in full"),
                    ("/  e  Esc", "filter text / problems only / clear"),
                ],
            ));
            lines.extend(group(
                "Settings",
                &[
                    ("↑↓  ←→", "field / section"),
                    ("Enter Space", "edit a value or switch a choice"),
                ],
            ));
            (" Keys ".into(), lines, ACCENT, 62)
        }
        Some(Modal::Entry) => {
            let Some(entry) = app.log.selected() else {
                return;
            };
            let mut lines = vec![
                Line::styled(
                    format!(
                        "{}  {:?}{}",
                        entry.at.format("%Y-%m-%d %H:%M:%S"),
                        entry.level,
                        if entry.repeats > 1 {
                            format!("  ×{} (last shown)", entry.repeats)
                        } else {
                            String::new()
                        }
                    ),
                    Style::default().fg(MUTED),
                ),
                Line::raw(""),
                Line::raw(entry.text.clone()),
            ];
            if let Some(detail) = &entry.detail {
                lines.push(Line::raw(""));
                lines.extend(
                    detail
                        .lines()
                        .map(|line| Line::styled(clean(line), Style::default().fg(MUTED))),
                );
            }
            (" Event ".into(), lines, ACCENT, 100)
        }
    };
    let width = width.min(area.width.saturating_sub(4));
    // Rows after wrapping, as an estimate from the character counts.
    let rows: u16 = lines
        .iter()
        .map(|line| {
            (line.width() as u16)
                .div_ceil(width.saturating_sub(4).max(1))
                .max(1)
        })
        .sum();
    let height = (rows + 2).min(area.height.saturating_sub(2));
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(color))
                .title(Span::styled(title, Style::default().fg(color).bold()))
                .padding(ratatui::widgets::Padding::horizontal(1)),
        ),
        rect,
    );
}
