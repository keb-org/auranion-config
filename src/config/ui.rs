use anyhow::{Result, bail};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use super::integration::Integration;

pub(super) fn select_integrations(
    detected: &[bool; 6],
    defaults: &[bool; 6],
) -> Result<Vec<Integration>> {
    ratatui::run(|terminal| {
        let mut selected = *defaults;
        let mut cursor = 0;

        loop {
            terminal.draw(|frame| {
                let chunks = Layout::vertical([
                    Constraint::Length(3),
                    Constraint::Min(12),
                    Constraint::Length(3),
                ])
                .split(frame.area());

                // Header Banner
                let header = Paragraph::new(Line::from(Span::styled(
                    "Kryat, Inc. & Kebsoft",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )))
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::BOTTOM)
                        .border_style(Style::default().fg(Color::Rgb(80, 80, 80))),
                );
                frame.render_widget(header, chunks[0]);

                // Split Body: Left List (40%), Right Info Panel (60%)
                let body_chunks =
                    Layout::horizontal([Constraint::Percentage(42), Constraint::Percentage(58)])
                        .spacing(1)
                        .split(chunks[1]);

                // Left: Integration List
                let items: Vec<ListItem> = Integration::ALL
                    .iter()
                    .enumerate()
                    .map(|(index, integration)| {
                        let is_checked = selected[index];
                        let is_detected = detected[index];

                        let checkbox = if is_checked {
                            Span::styled(" [✓] ", Style::default().fg(Color::Green).bold())
                        } else {
                            Span::styled(" [ ] ", Style::default().fg(Color::DarkGray))
                        };

                        let name = Span::styled(
                            format!("{:<15}", integration.label()),
                            Style::default().fg(Color::White).bold(),
                        );

                        let badge = if is_detected {
                            Span::styled(
                                " Detected ",
                                Style::default()
                                    .fg(Color::Rgb(0, 200, 100))
                                    .bg(Color::Rgb(20, 50, 30)),
                            )
                        } else {
                            Span::styled(
                                " Not Found ",
                                Style::default()
                                    .fg(Color::DarkGray)
                                    .bg(Color::Rgb(30, 30, 30)),
                            )
                        };

                        ListItem::new(Line::from(vec![checkbox, name, badge]))
                    })
                    .collect();

                let list = List::new(items)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_type(BorderType::Rounded)
                            .border_style(Style::default().fg(Color::Cyan))
                            .title(Span::styled(
                                " Integrations ",
                                Style::default().fg(Color::Cyan).bold(),
                            )),
                    )
                    .highlight_style(
                        Style::default()
                            .bg(Color::Rgb(45, 45, 65))
                            .add_modifier(Modifier::BOLD),
                    )
                    .highlight_symbol("▶ ");

                let mut list_state = ListState::default();
                list_state.select(Some(cursor));
                frame.render_stateful_widget(list, body_chunks[0], &mut list_state);

                // Right: Details Panel for currently highlighted integration
                let current_integration = Integration::ALL[cursor];
                let is_detected = detected[cursor];
                let is_selected = selected[cursor];

                let detail_text =
                    get_integration_details(current_integration, is_detected, is_selected);
                let details_panel = Paragraph::new(detail_text)
                    .wrap(Wrap { trim: false })
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_type(BorderType::Rounded)
                            .border_style(Style::default().fg(Color::Magenta))
                            .title(Span::styled(
                                format!(" {} Details ", current_integration.label()),
                                Style::default().fg(Color::Magenta).bold(),
                            )),
                    );
                frame.render_widget(details_panel, body_chunks[1]);

                // Footer Controls Bar
                let controls = Line::from(vec![
                    Span::styled("  ↑/↓ ", Style::default().fg(Color::Yellow).bold()),
                    Span::raw("Navigate  "),
                    Span::styled("Space ", Style::default().fg(Color::Yellow).bold()),
                    Span::raw("Toggle  "),
                    Span::styled("Enter ", Style::default().fg(Color::Green).bold()),
                    Span::raw("Apply & Save  "),
                    Span::styled("Esc ", Style::default().fg(Color::Red).bold()),
                    Span::raw("Cancel"),
                ]);

                let footer = Paragraph::new(controls).alignment(Alignment::Center).block(
                    Block::default()
                        .borders(Borders::TOP)
                        .border_style(Style::default().fg(Color::Rgb(80, 80, 80))),
                );
                frame.render_widget(footer, chunks[2]);
            })?;

            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }

            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    cursor = cursor.checked_sub(1).unwrap_or(Integration::ALL.len() - 1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    cursor = (cursor + 1) % Integration::ALL.len();
                }
                KeyCode::Char(' ') => selected[cursor] = !selected[cursor],
                KeyCode::Enter => {
                    return Ok(selected
                        .into_iter()
                        .enumerate()
                        .filter_map(|(index, is_selected)| {
                            is_selected.then_some(Integration::ALL[index])
                        })
                        .collect());
                }
                KeyCode::Esc => bail!("configuration cancelled"),
                _ => {}
            }
        }
    })
}

pub(super) fn confirm_use_saved_key(masked_key: &str) -> Result<bool> {
    ratatui::run(|terminal| {
        let mut focus_yes = true;

        loop {
            terminal.draw(|frame| {
                let area = centered_rect(60, 35, frame.area());
                frame.render_widget(Clear, area);

                let block = Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Double)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title(Span::styled(
                        " Existing Credential Found ",
                        Style::default().fg(Color::Cyan).bold(),
                    ));

                let inner_layout = Layout::vertical([
                    Constraint::Length(2),
                    Constraint::Length(2),
                    Constraint::Length(3),
                ])
                .margin(2)
                .split(area);

                let msg = Paragraph::new(vec![
                    Line::from("A saved Auranion API key was found in your secure keyring:"),
                    Line::from(Span::styled(
                        format!("  {}", masked_key),
                        Style::default().fg(Color::Yellow).bold(),
                    )),
                ]);

                let prompt =
                    Paragraph::new("Would you like to use this key?").alignment(Alignment::Center);

                let yes_btn = if focus_yes {
                    Span::styled(
                        "  [ YES ]  ",
                        Style::default().fg(Color::Black).bg(Color::Green).bold(),
                    )
                } else {
                    Span::styled("    YES    ", Style::default().fg(Color::DarkGray))
                };

                let no_btn = if !focus_yes {
                    Span::styled(
                        "  [ NO (Enter New) ]  ",
                        Style::default().fg(Color::Black).bg(Color::Red).bold(),
                    )
                } else {
                    Span::styled(
                        "    NO (Enter New)    ",
                        Style::default().fg(Color::DarkGray),
                    )
                };

                let buttons = Paragraph::new(Line::from(vec![yes_btn, Span::raw("     "), no_btn]))
                    .alignment(Alignment::Center);

                frame.render_widget(block, area);
                frame.render_widget(msg, inner_layout[0]);
                frame.render_widget(prompt, inner_layout[1]);
                frame.render_widget(buttons, inner_layout[2]);
            })?;

            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }

            match key.code {
                KeyCode::Left
                | KeyCode::Right
                | KeyCode::Tab
                | KeyCode::Char('h')
                | KeyCode::Char('l') => {
                    focus_yes = !focus_yes;
                }
                KeyCode::Char('y') | KeyCode::Char('Y') => return Ok(true),
                KeyCode::Char('n') | KeyCode::Char('N') => return Ok(false),
                KeyCode::Enter => return Ok(focus_yes),
                KeyCode::Esc => bail!("authentication cancelled"),
                _ => {}
            }
        }
    })
}

pub(super) fn prompt_api_key_tui() -> Result<String> {
    ratatui::run(|terminal| {
        let mut input = String::new();
        let mut show_plain = false;
        let mut error_msg: Option<String> = None;

        loop {
            terminal.draw(|frame| {
                let area = centered_rect(65, 45, frame.area());
                frame.render_widget(Clear, area);

                let block = Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Double)
                    .border_style(Style::default().fg(Color::Magenta))
                    .title(Span::styled(
                        " Enter Auranion API Key ",
                        Style::default().fg(Color::Magenta).bold(),
                    ));

                let inner_layout = Layout::vertical([
                    Constraint::Length(2),
                    Constraint::Length(3),
                    Constraint::Length(2),
                    Constraint::Length(2),
                ])
                .margin(2)
                .split(area);

                let label = Paragraph::new(vec![Line::from(
                    "Please enter your Auranion API key to authorize gateway access.",
                )]);

                let display_text = if show_plain {
                    input.clone()
                } else {
                    "*".repeat(input.len())
                };

                let input_widget = Paragraph::new(Line::from(vec![
                    Span::styled(" Key: ", Style::default().fg(Color::Cyan).bold()),
                    Span::styled(display_text, Style::default().fg(Color::White).bold()),
                    Span::styled("█", Style::default().fg(Color::Yellow)), // cursor
                ]))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(Color::Rgb(100, 100, 150))),
                );

                let hint = Paragraph::new(Line::from(vec![
                    Span::styled("Ctrl+T ", Style::default().fg(Color::Yellow).bold()),
                    Span::raw(if show_plain {
                        "Hide Key  "
                    } else {
                        "Show Key  "
                    }),
                    Span::styled("Enter ", Style::default().fg(Color::Green).bold()),
                    Span::raw("Confirm  "),
                    Span::styled("Esc ", Style::default().fg(Color::Red).bold()),
                    Span::raw("Cancel"),
                ]))
                .alignment(Alignment::Center);

                let status_line = if let Some(ref err) = error_msg {
                    Paragraph::new(Span::styled(err, Style::default().fg(Color::Red).bold()))
                        .alignment(Alignment::Center)
                } else {
                    Paragraph::new(Span::styled(
                        "Stored securely in your system keyring",
                        Style::default().fg(Color::DarkGray),
                    ))
                    .alignment(Alignment::Center)
                };

                frame.render_widget(block, area);
                frame.render_widget(label, inner_layout[0]);
                frame.render_widget(input_widget, inner_layout[1]);
                frame.render_widget(hint, inner_layout[2]);
                frame.render_widget(status_line, inner_layout[3]);
            })?;

            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }

            match key.code {
                KeyCode::Char('t')
                    if key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    show_plain = !show_plain;
                }
                KeyCode::Char(c) => {
                    input.push(c);
                    error_msg = None;
                }
                KeyCode::Backspace => {
                    input.pop();
                    error_msg = None;
                }
                KeyCode::Enter => {
                    if input.trim().is_empty() {
                        error_msg = Some("API Key cannot be empty!".into());
                    } else {
                        return Ok(input.trim().to_string());
                    }
                }
                KeyCode::Esc => bail!("API key input cancelled"),
                _ => {}
            }
        }
    })
}

fn get_integration_details<'a>(
    integration: Integration,
    _is_detected: bool,
    _is_selected: bool,
) -> Text<'a> {
    let mut lines = Vec::new();

    lines.push(Line::from(Span::styled(
        "* Where will be configured",
        Style::default().fg(Color::Yellow).bold(),
    )));

    match integration {
        Integration::ClaudeDesktop => {
            if cfg!(windows) {
                lines.push(Line::from(
                    "  • %LOCALAPPDATA%\\Claude-3p\\configLibrary\\_meta.json",
                ));
                lines.push(Line::from(
                    "  • %LOCALAPPDATA%\\Claude-3p\\configLibrary\\<id>.json",
                ));
            } else if cfg!(target_os = "macos") {
                lines.push(Line::from(
                    "  • ~/Library/Application Support/Claude-3p/configLibrary/_meta.json",
                ));
                lines.push(Line::from(
                    "  • ~/Library/Application Support/Claude-3p/configLibrary/<id>.json",
                ));
                lines.push(Line::from(
                    "    (app data: ~/Library/Application Support/Claude)",
                ));
            } else {
                lines.push(Line::from(
                    "  • ~/.config/Claude-3p/configLibrary/_meta.json",
                ));
                lines.push(Line::from(
                    "  • ~/.config/Claude-3p/configLibrary/<id>.json",
                ));
            }
        }
        Integration::ClaudeCode => {
            lines.push(Line::from("  • ~/.claude/settings.json (env)"));
        }
        Integration::CodexDesktop => {
            lines.push(Line::from("  • ~/.codex/config.toml"));
            lines.push(Line::from("  • ~/.codex/model-catalogs/auranion.json"));
            lines.push(Line::from("  • ~/.codex/desktop-model-providers.json"));
        }
        Integration::CodexCli => {
            lines.push(Line::from("  • ~/.codex/config.toml"));
            lines.push(Line::from("  • ~/.codex/model-catalogs/auranion.json"));
        }
        Integration::OpenCode => {
            lines.push(Line::from("  • ~/.config/opencode/opencode.jsonc"));
            lines.push(Line::from("  • ~/.local/share/opencode/auth.json"));
        }
        Integration::Hermes => {
            if cfg!(windows) {
                lines.push(Line::from("  • %LOCALAPPDATA%\\hermes\\config.yaml"));
            } else {
                lines.push(Line::from("  • ~/.hermes/config.yaml"));
                lines.push(Line::from("    (or $HERMES_HOME/config.yaml)"));
            }
            lines.push(Line::from(format!(
                "  • provider `{}` → {}",
                "auranion",
                super::BASE_URL
            )));
        }
    }

    lines.push(Line::from(""));
    match integration {
        Integration::CodexDesktop => {
            lines.push(Line::from(Span::styled(
                "* App alias → Auranion target",
                Style::default().fg(Color::Green).bold(),
            )));
            for (alias, target) in super::codex_desktop_routes() {
                lines.push(Line::from(format!("  • {alias} → {target}")));
            }
        }
        _ => {
            lines.push(Line::from(Span::styled(
                "* Models will be added",
                Style::default().fg(Color::Green).bold(),
            )));
            for model in crate::catalog::MODELS {
                lines.push(Line::from(format!(
                    "  • {} ({})",
                    model.upstream, model.label
                )));
            }
        }
    }

    Text::from(lines)
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(r);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(popup_layout[1])[1]
}
