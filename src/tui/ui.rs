use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use super::app::App;

const GREEN: Color = Color::Green;
const DARK_GREEN: Color = Color::Rgb(0, 100, 0);
const BG: Color = Color::Black;

fn border_style() -> Style {
    Style::default().fg(GREEN).bg(BG)
}

fn text_style() -> Style {
    Style::default().fg(GREEN).bg(BG)
}

fn dim_style() -> Style {
    Style::default().fg(Color::DarkGray).bg(BG)
}

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Title bar
            Constraint::Min(8),    // Main panels
            Constraint::Length(8), // Log
            Constraint::Length(1), // Status bar
        ])
        .split(f.area());

    draw_title(f, chunks[0]);
    draw_panels(f, chunks[1], app);
    draw_log(f, chunks[2], app);
    draw_status(f, chunks[3], app);
}

fn draw_title(f: &mut Frame, area: Rect) {
    let title = Paragraph::new(Line::from(vec![
        Span::styled("  MICRO", Style::default().fg(GREEN).add_modifier(Modifier::BOLD)),
        Span::styled("CHAIN", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled("  v0.1.0", dim_style()),
    ]))
    .block(Block::default().borders(Borders::ALL).border_style(border_style()))
    .style(text_style());
    f.render_widget(title, area);
}

fn draw_panels(f: &mut Frame, area: Rect, app: &App) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let left_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(cols[0]);

    let right_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(cols[1]);

    // Chain info
    let chain_info = Paragraph::new(vec![
        Line::from(Span::styled(format!("  Height:     {}", app.height), text_style())),
        Line::from(Span::styled(format!("  Tip:        {}...", app.tip_hash), text_style())),
        Line::from(Span::styled(format!("  Difficulty:  {}", app.difficulty), text_style())),
        Line::from(Span::styled(format!("  Mining:      {}", if app.mining { "ON" } else { "OFF" }), text_style())),
    ])
    .block(Block::default().title(" CHAIN ").borders(Borders::ALL).border_style(border_style()));
    f.render_widget(chain_info, left_rows[0]);

    // Peers
    let peers_info = Paragraph::new(vec![
        Line::from(Span::styled(format!("  Connected: {}", app.peer_count), text_style())),
    ])
    .block(Block::default().title(" PEERS ").borders(Borders::ALL).border_style(border_style()));
    f.render_widget(peers_info, left_rows[1]);

    // Mempool
    let mempool_info = Paragraph::new(vec![
        Line::from(Span::styled(format!("  Pending txs: {}", app.mempool_count), text_style())),
    ])
    .block(Block::default().title(" MEMPOOL ").borders(Borders::ALL).border_style(border_style()));
    f.render_widget(mempool_info, right_rows[0]);

    // Wallet
    let wallet_info = if !app.address.is_empty() {
        Paragraph::new(vec![
            Line::from(Span::styled(format!("  Address: {}", app.address), text_style())),
            Line::from(Span::styled(format!("  Balance: {}", app.balance), text_style())),
        ])
    } else {
        Paragraph::new(Line::from(Span::styled("  No wallet loaded", dim_style())))
    }
    .block(Block::default().title(" WALLET ").borders(Borders::ALL).border_style(border_style()));
    f.render_widget(wallet_info, right_rows[1]);
}

fn draw_log(f: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = app.logs
        .iter()
        .rev()
        .take(area.height as usize - 2)
        .rev()
        .map(|s| ListItem::new(Span::styled(format!("  {s}"), dim_style())))
        .collect();

    let log = List::new(items)
        .block(Block::default().title(" LOG ").borders(Borders::ALL).border_style(border_style()));
    f.render_widget(log, area);
}

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    let status = Paragraph::new(Line::from(vec![
        Span::styled(" [q]", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled(" Quit  ", dim_style()),
        Span::styled("[m]", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled(" Mining  ", dim_style()),
        Span::styled("[Tab]", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled(" Switch  ", dim_style()),
    ]))
    .style(Style::default().bg(DARK_GREEN).fg(Color::White));
    f.render_widget(status, area);
}
