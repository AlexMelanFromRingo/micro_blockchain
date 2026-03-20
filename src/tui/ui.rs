use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use super::app::{App, Tab};

// Blockstream Green palette
const CYAN: Color = Color::Rgb(0, 195, 255);    // Blockstream Blue #00C3FF
const BG: Color = Color::Rgb(17, 19, 22);        // Space Grey #111316
const BG_PANEL: Color = Color::Rgb(24, 26, 30);  // Slightly lighter panel bg
const TEXT: Color = Color::Rgb(200, 205, 215);    // Light grey text
const DIM: Color = Color::Rgb(100, 105, 115);     // Dimmed text
const GREEN: Color = Color::Rgb(0, 200, 100);     // Positive/confirmed
const RED: Color = Color::Rgb(220, 60, 60);        // Negative/error
const WHITE: Color = Color::Rgb(240, 242, 245);   // Bright white

fn accent() -> Style { Style::default().fg(CYAN) }
fn _text() -> Style { Style::default().fg(TEXT).bg(BG) }
fn dim() -> Style { Style::default().fg(DIM) }
fn panel_border() -> Style { Style::default().fg(Color::Rgb(50, 55, 65)) }
fn active_border() -> Style { Style::default().fg(CYAN) }

pub fn draw(f: &mut Frame, app: &App) {
    // Fill background
    let area = f.area();
    let bg = Block::default().style(Style::default().bg(BG));
    f.render_widget(bg, area);

    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),   // Header
            Constraint::Min(10),     // Body
            Constraint::Length(1),   // Status bar
        ])
        .split(area);

    draw_header(f, main_layout[0], app);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(20), // Sidebar
            Constraint::Min(40),   // Content
        ])
        .split(main_layout[1]);

    draw_sidebar(f, body[0], app);
    draw_content(f, body[1], app);
    draw_statusbar(f, main_layout[2], app);
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let balance_text = if !app.address.is_empty() {
        app.format_balance()
    } else {
        "No wallet".into()
    };

    let header = Paragraph::new(Line::from(vec![
        Span::styled("  MICRO", Style::default().fg(CYAN).add_modifier(Modifier::BOLD)),
        Span::styled("CHAIN ", Style::default().fg(WHITE).add_modifier(Modifier::BOLD)),
        Span::styled("                    ", dim()),
        Span::styled(&balance_text, Style::default().fg(WHITE).add_modifier(Modifier::BOLD)),
    ]))
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(panel_border())
            .style(Style::default().bg(BG_PANEL))
    )
    .alignment(Alignment::Left);
    f.render_widget(header, area);
}

fn draw_sidebar(f: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = Tab::ALL.iter().map(|tab| {
        let style = if *tab == app.active_tab {
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD)
        } else {
            dim()
        };
        let marker = if *tab == app.active_tab { ">" } else { " " };
        ListItem::new(Line::from(vec![
            Span::styled(format!(" {marker} "), style),
            Span::styled(tab.icon(), style),
            Span::styled(tab.label(), style),
        ]))
    }).collect();

    let sidebar = List::new(items)
        .block(
            Block::default()
                .borders(Borders::RIGHT)
                .border_style(panel_border())
                .style(Style::default().bg(BG))
        );
    f.render_widget(sidebar, area);
}

fn draw_content(f: &mut Frame, area: Rect, app: &App) {
    match app.active_tab {
        Tab::Home => draw_home(f, area, app),
        Tab::Transactions => draw_transactions(f, area, app),
        Tab::Network => draw_network(f, area, app),
        Tab::Mining => draw_mining(f, area, app),
    }
}

fn draw_home(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),  // Balance card
            Constraint::Length(7),  // Chain info
            Constraint::Min(4),    // Activity log
        ])
        .margin(1)
        .split(area);

    // Balance card
    let balance_lines = if !app.address.is_empty() {
        vec![
            Line::from(""),
            Line::from(Span::styled(
                format!("  {}", app.format_balance()),
                Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Address  ", dim()),
                Span::styled(&app.address, Style::default().fg(TEXT)),
            ]),
            Line::from(vec![
                Span::styled("  UTXOs    ", dim()),
                Span::styled(format!("{}", app.utxo_count), Style::default().fg(TEXT)),
            ]),
        ]
    } else {
        vec![
            Line::from(""),
            Line::from(Span::styled("  No wallet loaded", dim())),
            Line::from(Span::styled("  Start with --wallet <mnemonic>", dim())),
        ]
    };
    let balance_card = Paragraph::new(balance_lines)
        .block(
            Block::default()
                .title(Span::styled(" Balance ", accent()))
                .borders(Borders::ALL)
                .border_style(active_border())
                .style(Style::default().bg(BG_PANEL))
        );
    f.render_widget(balance_card, chunks[0]);

    // Chain info
    let chain_lines = vec![
        Line::from(vec![
            Span::styled("  Height      ", dim()),
            Span::styled(format!("{}", app.height), Style::default().fg(WHITE)),
        ]),
        Line::from(vec![
            Span::styled("  Tip         ", dim()),
            Span::styled(format!("{}...", app.tip_hash), Style::default().fg(TEXT)),
        ]),
        Line::from(vec![
            Span::styled("  Difficulty  ", dim()),
            Span::styled(format!("{:#010x}", app.difficulty), Style::default().fg(TEXT)),
        ]),
        Line::from(vec![
            Span::styled("  Mempool     ", dim()),
            Span::styled(format!("{} pending", app.mempool_count), Style::default().fg(TEXT)),
        ]),
    ];
    let chain_card = Paragraph::new(chain_lines)
        .block(
            Block::default()
                .title(Span::styled(" Chain ", accent()))
                .borders(Borders::ALL)
                .border_style(panel_border())
                .style(Style::default().bg(BG_PANEL))
        );
    f.render_widget(chain_card, chunks[1]);

    // Activity log
    draw_log(f, chunks[2], app);
}

fn draw_transactions(f: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = if app.tx_history.is_empty() {
        vec![ListItem::new(Span::styled("  No transactions yet", dim()))]
    } else {
        app.tx_history.iter().rev().take(area.height as usize - 2).map(|tx| {
            let (arrow, color) = match tx.direction {
                super::app::TxDirection::Incoming => ("+", GREEN),
                super::app::TxDirection::Outgoing => ("-", RED),
                super::app::TxDirection::Coinbase => ("+", CYAN),
            };
            let whole = tx.amount / 1000;
            let frac = tx.amount % 1000;
            ListItem::new(Line::from(vec![
                Span::styled(format!("  {arrow}{whole}.{frac:03} MCH"), Style::default().fg(color)),
                Span::styled(format!("  #{} ", tx.height), dim()),
                Span::styled(&tx.txid_short, dim()),
            ]))
        }).collect()
    };

    let tx_list = List::new(items)
        .block(
            Block::default()
                .title(Span::styled(" Transactions ", accent()))
                .borders(Borders::ALL)
                .border_style(panel_border())
                .style(Style::default().bg(BG_PANEL))
        );
    f.render_widget(tx_list, Rect { x: area.x + 1, y: area.y + 1, width: area.width.saturating_sub(2), height: area.height.saturating_sub(2) });
}

fn draw_network(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(4),
        ])
        .margin(1)
        .split(area);

    // Peer stats
    let peer_lines = vec![
        Line::from(vec![
            Span::styled("  Connected peers  ", dim()),
            Span::styled(format!("{}", app.peer_count), Style::default().fg(WHITE)),
        ]),
        Line::from(vec![
            Span::styled("  Synced height    ", dim()),
            Span::styled(format!("{}", app.height), Style::default().fg(WHITE)),
        ]),
    ];
    let peer_card = Paragraph::new(peer_lines)
        .block(
            Block::default()
                .title(Span::styled(" Peers ", accent()))
                .borders(Borders::ALL)
                .border_style(panel_border())
                .style(Style::default().bg(BG_PANEL))
        );
    f.render_widget(peer_card, chunks[0]);

    // Network log
    draw_log(f, chunks[1], app);
}

fn draw_mining(f: &mut Frame, area: Rect, app: &App) {
    let status = if app.mining { "ACTIVE" } else { "STOPPED" };
    let status_color = if app.mining { GREEN } else { RED };

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Status        ", dim()),
            Span::styled(status, Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  Difficulty    ", dim()),
            Span::styled(format!("{:#010x}", app.difficulty), Style::default().fg(TEXT)),
        ]),
        Line::from(vec![
            Span::styled("  Block height  ", dim()),
            Span::styled(format!("{}", app.height), Style::default().fg(WHITE)),
        ]),
        Line::from(vec![
            Span::styled("  Reward        ", dim()),
            Span::styled(
                format!("{}.{:03} MCH", crate::consensus::pow::block_reward(app.height + 1) / 1000,
                    crate::consensus::pow::block_reward(app.height + 1) % 1000),
                Style::default().fg(TEXT),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled("  Press [m] to toggle mining", dim())),
    ];

    let mining_card = Paragraph::new(lines)
        .block(
            Block::default()
                .title(Span::styled(" Mining ", accent()))
                .borders(Borders::ALL)
                .border_style(if app.mining { active_border() } else { panel_border() })
                .style(Style::default().bg(BG_PANEL))
        );
    f.render_widget(mining_card, Rect { x: area.x + 1, y: area.y + 1, width: area.width.saturating_sub(2), height: area.height.saturating_sub(2).min(10) });
}

fn draw_log(f: &mut Frame, area: Rect, app: &App) {
    let max_items = area.height.saturating_sub(2) as usize;
    let items: Vec<ListItem> = app.logs
        .iter()
        .rev()
        .take(max_items)
        .rev()
        .map(|s| ListItem::new(Span::styled(format!("  {s}"), dim())))
        .collect();

    let log = List::new(items)
        .block(
            Block::default()
                .title(Span::styled(" Activity ", Style::default().fg(DIM)))
                .borders(Borders::ALL)
                .border_style(panel_border())
                .style(Style::default().bg(BG_PANEL))
        );
    f.render_widget(log, area);
}

fn draw_statusbar(f: &mut Frame, area: Rect, _app: &App) {
    let status = Paragraph::new(Line::from(vec![
        Span::styled(" [q]", Style::default().fg(CYAN).add_modifier(Modifier::BOLD)),
        Span::styled(" Quit ", dim()),
        Span::styled("[m]", Style::default().fg(CYAN).add_modifier(Modifier::BOLD)),
        Span::styled(" Mine ", dim()),
        Span::styled("[Tab]", Style::default().fg(CYAN).add_modifier(Modifier::BOLD)),
        Span::styled(" Next ", dim()),
        Span::styled("[Shift+Tab]", Style::default().fg(CYAN).add_modifier(Modifier::BOLD)),
        Span::styled(" Prev ", dim()),
    ]))
    .style(Style::default().bg(Color::Rgb(30, 33, 38)).fg(TEXT));
    f.render_widget(status, area);
}
