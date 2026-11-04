use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    layout::{Constraint, Flex, Layout, Position, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, List, ListItem, ListState, Paragraph},
    DefaultTerminal, Frame,
};
use std::path::Path;
use unicode_width::UnicodeWidthStr;

use crate::config::{looks_like_sudo, Config, Entry, Runner};
use crate::privilege;

pub enum Action {
    Run,
    Quit,
}

#[derive(PartialEq, Clone, Copy)]
enum Focus {
    List,
    Run,
    Cancel,
}

#[derive(Default)]
struct Input {
    text: String,
    cursor: usize, // 文字単位
}

impl Input {
    fn new(s: &str) -> Self {
        Input {
            text: s.to_string(),
            cursor: s.chars().count(),
        }
    }
    fn byte_idx(&self) -> usize {
        self.text
            .char_indices()
            .nth(self.cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.text.len())
    }
    fn insert(&mut self, c: char) {
        let i = self.byte_idx();
        self.text.insert(i, c);
        self.cursor += 1;
    }
    fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            let i = self.byte_idx();
            self.text.remove(i);
        }
    }
    fn delete(&mut self) {
        if self.cursor < self.text.chars().count() {
            let i = self.byte_idx();
            self.text.remove(i);
        }
    }
    fn width_before_cursor(&self) -> u16 {
        self.text[..self.byte_idx()].width() as u16
    }
}

const FIELDS: usize = 6;

struct Form {
    index: Option<usize>,
    name: Input,
    command: Input,
    interactive: bool,
    run_last: bool,
    requires_root: bool,
    wsl: bool,
    root_touched: bool,
    field: usize,
    error: Option<String>,
}

impl Form {
    fn new() -> Self {
        Form {
            index: None,
            name: Input::default(),
            command: Input::default(),
            interactive: false,
            run_last: false,
            requires_root: false,
            wsl: false,
            root_touched: false,
            field: 0,
            error: None,
        }
    }
    fn edit(i: usize, e: &Entry) -> Self {
        Form {
            index: Some(i),
            name: Input::new(&e.name),
            command: Input::new(&e.command),
            interactive: e.interactive,
            run_last: e.run_last,
            requires_root: e.requires_root,
            wsl: e.runner == Runner::Wsl,
            root_touched: true,
            field: 0,
            error: None,
        }
    }
    fn text_input(&mut self) -> Option<&mut Input> {
        match self.field {
            0 => Some(&mut self.name),
            1 => Some(&mut self.command),
            _ => None,
        }
    }
    fn toggle(&mut self) {
        match self.field {
            2 => self.interactive = !self.interactive,
            3 => self.run_last = !self.run_last,
            4 => {
                self.requires_root = !self.requires_root;
                self.root_touched = true;
            }
            5 => self.wsl = !self.wsl,
            _ => {}
        }
    }
}

enum Mode {
    Normal,
    Edit(Form),
    ConfirmDelete,
}

struct App<'a> {
    cfg: &'a mut Config,
    path: &'a Path,
    state: ListState,
    focus: Focus,
    mode: Mode,
    message: Option<String>,
}

pub fn run(cfg: &mut Config, path: &Path) -> Result<Action> {
    let mut terminal = ratatui::init();
    let mut app = App {
        state: ListState::default().with_selected((!cfg.commands.is_empty()).then_some(0)),
        cfg,
        path,
        focus: Focus::List,
        mode: Mode::Normal,
        message: None,
    };
    let res = app.event_loop(&mut terminal);
    ratatui::restore();
    res
}

impl App<'_> {
    fn event_loop(&mut self, terminal: &mut DefaultTerminal) -> Result<Action> {
        loop {
            terminal.draw(|f| self.draw(f))?;
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    return Ok(Action::Quit);
                }
                if let Some(a) = self.handle_key(key)? {
                    return Ok(a);
                }
            }
        }
    }

    fn save(&mut self) {
        if let Err(e) = self.cfg.save(self.path) {
            self.message = Some(format!("保存失敗: {e:#}"));
        }
    }

    fn selected(&self) -> Option<usize> {
        self.state
            .selected()
            .filter(|&i| i < self.cfg.commands.len())
    }

    fn handle_key(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        self.message = None;
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Normal => return Ok(self.handle_normal(key)),
            Mode::ConfirmDelete => {
                if matches!(key.code, KeyCode::Char('y') | KeyCode::Char('Y')) {
                    if let Some(i) = self.selected() {
                        self.cfg.commands.remove(i);
                        if self.cfg.commands.is_empty() {
                            self.state.select(None);
                        } else {
                            self.state.select(Some(i.min(self.cfg.commands.len() - 1)));
                        }
                        self.save();
                    }
                }
            }
            Mode::Edit(mut form) => {
                if !self.handle_form(&mut form, key) {
                    self.mode = Mode::Edit(form);
                }
            }
        }
        Ok(None)
    }

    fn handle_normal(&mut self, key: KeyEvent) -> Option<Action> {
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::List => Focus::Run,
                    Focus::Run => Focus::Cancel,
                    Focus::Cancel => Focus::List,
                }
            }
            KeyCode::BackTab => {
                self.focus = match self.focus {
                    Focus::List => Focus::Cancel,
                    Focus::Run => Focus::List,
                    Focus::Cancel => Focus::Run,
                }
            }
            KeyCode::Left | KeyCode::Right if self.focus != Focus::List => {
                self.focus = if self.focus == Focus::Run {
                    Focus::Cancel
                } else {
                    Focus::Run
                };
            }
            KeyCode::Esc | KeyCode::Char('q') => return Some(Action::Quit),
            KeyCode::Char('r') => return Some(Action::Run),
            KeyCode::Enter => match self.focus {
                Focus::Run => return Some(Action::Run),
                Focus::Cancel => return Some(Action::Quit),
                Focus::List => {
                    if let Some(i) = self.selected() {
                        self.mode = Mode::Edit(Form::edit(i, &self.cfg.commands[i]));
                    }
                }
            },
            KeyCode::Char('K') => self.move_entry(-1),
            KeyCode::Char('J') => self.move_entry(1),
            KeyCode::Up if shift => self.move_entry(-1),
            KeyCode::Down if shift => self.move_entry(1),
            KeyCode::Up | KeyCode::Char('k') => {
                self.focus = Focus::List;
                self.state.select_previous();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.focus = Focus::List;
                self.state.select_next();
            }
            KeyCode::Char(' ') => {
                if let Some(i) = self.selected() {
                    self.cfg.commands[i].enabled = !self.cfg.commands[i].enabled;
                    self.save();
                }
            }
            KeyCode::Char('a') => self.mode = Mode::Edit(Form::new()),
            KeyCode::Char('e') => {
                if let Some(i) = self.selected() {
                    self.mode = Mode::Edit(Form::edit(i, &self.cfg.commands[i]));
                }
            }
            KeyCode::Char('d') | KeyCode::Delete if self.selected().is_some() => {
                self.mode = Mode::ConfirmDelete;
            }
            _ => {}
        }
        None
    }

    fn move_entry(&mut self, delta: isize) {
        let Some(i) = self.selected() else { return };
        let j = i as isize + delta;
        if j < 0 || j as usize >= self.cfg.commands.len() {
            return;
        }
        self.cfg.commands.swap(i, j as usize);
        self.state.select(Some(j as usize));
        self.save();
    }

    /// 戻り値: フォームを閉じるなら true
    fn handle_form(&mut self, form: &mut Form, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => return true,
            KeyCode::Enter => return self.submit(form),
            KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % FIELDS,
            KeyCode::BackTab | KeyCode::Up => form.field = (form.field + FIELDS - 1) % FIELDS,
            _ => {
                if let Some(input) = form.text_input() {
                    match key.code {
                        KeyCode::Char(c) => input.insert(c),
                        KeyCode::Backspace => input.backspace(),
                        KeyCode::Delete => input.delete(),
                        KeyCode::Left => input.cursor = input.cursor.saturating_sub(1),
                        KeyCode::Right => {
                            input.cursor = (input.cursor + 1).min(input.text.chars().count())
                        }
                        KeyCode::Home => input.cursor = 0,
                        KeyCode::End => input.cursor = input.text.chars().count(),
                        _ => {}
                    }
                    if form.field == 1 && !form.root_touched {
                        form.requires_root = looks_like_sudo(&form.command.text);
                    }
                } else if key.code == KeyCode::Char(' ') {
                    form.toggle();
                }
            }
        }
        false
    }

    fn submit(&mut self, form: &mut Form) -> bool {
        if form.command.text.trim().is_empty() {
            form.error = Some("コマンドを入力してください".into());
            form.field = 1;
            return false;
        }
        let mut e = Entry::new(form.name.text.trim(), form.command.text.trim());
        e.interactive = form.interactive;
        e.run_last = form.run_last;
        e.requires_root = form.requires_root;
        e.runner = if form.wsl {
            Runner::Wsl
        } else {
            Runner::Native
        };
        match form.index {
            Some(i) => {
                e.enabled = self.cfg.commands[i].enabled;
                self.cfg.commands[i] = e;
            }
            None => {
                self.cfg.commands.push(e);
                self.state.select(Some(self.cfg.commands.len() - 1));
            }
        }
        self.save();
        true
    }

    // ---------- 描画 ----------

    fn draw(&mut self, f: &mut Frame) {
        let [title, list, help, buttons] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(2),
            Constraint::Length(3),
        ])
        .areas(f.area());

        let mut t = vec![
            Span::styled(
                " tuc ",
                Style::new().bold().fg(Color::Black).bg(Color::Cyan),
            ),
            Span::raw(format!(
                " Tool Update Collector v{}",
                env!("CARGO_PKG_VERSION")
            )),
        ];
        if privilege::is_root() {
            t.push(Span::styled(
                "  [root: 対話なしで実行]",
                Style::new().fg(Color::Yellow).bold(),
            ));
        }
        f.render_widget(Line::from(t), title);

        self.draw_list(f, list);

        let help_text = match (&self.mode, &self.message) {
            (_, Some(m)) => Line::from(m.clone()).fg(Color::Red),
            (Mode::ConfirmDelete, _) => Line::from("選択中のコマンドを削除しますか? (y/N)").fg(Color::Yellow),
            _ => Line::from(
                "Tab:フォーカス移動  ↑↓:選択  Space:有効/無効  a:追加  e/Enter:編集  d:削除  J/K:並べ替え  r:実行  q:終了",
            )
            .dim(),
        };
        f.render_widget(
            Paragraph::new(help_text).wrap(ratatui::widgets::Wrap { trim: true }),
            help,
        );

        self.draw_buttons(f, buttons);

        if let Mode::Edit(form) = &self.mode {
            draw_form(f, form);
        }
    }

    fn draw_list(&mut self, f: &mut Frame, area: Rect) {
        let name_w = self
            .cfg
            .commands
            .iter()
            .map(|e| e.name.width())
            .max()
            .unwrap_or(0)
            .min(20);
        let items: Vec<ListItem> = self
            .cfg
            .commands
            .iter()
            .map(|e| {
                let check = if e.enabled { "[x] " } else { "[ ] " };
                let pad = name_w.saturating_sub(e.name.width());
                let mut spans = vec![
                    Span::styled(
                        check,
                        Style::new().fg(if e.enabled {
                            Color::Green
                        } else {
                            Color::DarkGray
                        }),
                    ),
                    Span::styled(
                        format!("{}{}  ", e.name, " ".repeat(pad)),
                        Style::new().bold(),
                    ),
                    Span::styled(e.command.clone(), Style::new().fg(Color::Gray)),
                ];
                for tag in e.tags() {
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(
                        format!("[{tag}]"),
                        Style::new().fg(Color::Magenta),
                    ));
                }
                let item = ListItem::new(Line::from(spans));
                if e.enabled {
                    item
                } else {
                    item.add_modifier(Modifier::DIM)
                }
            })
            .collect();

        let border = if self.focus == Focus::List {
            Color::Cyan
        } else {
            Color::DarkGray
        };
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(border))
            .title(" 登録コマンド (上から順に実行 / [最後] は末尾) ");
        if items.is_empty() {
            f.render_widget(
                Paragraph::new("コマンドが未登録です。a キーで追加してください。")
                    .dim()
                    .block(block),
                area,
            );
            return;
        }
        let list = List::new(items)
            .block(block)
            .highlight_style(Style::new().bg(Color::Rgb(40, 50, 70)))
            .highlight_symbol("▶ ");
        f.render_stateful_widget(list, area, &mut self.state);
    }

    fn draw_buttons(&self, f: &mut Frame, area: Rect) {
        let [run, cancel] = Layout::horizontal([Constraint::Length(14), Constraint::Length(14)])
            .flex(Flex::Center)
            .spacing(4)
            .areas(area);
        for (rect, label, focus, color) in [
            (run, "実行", Focus::Run, Color::Green),
            (cancel, "キャンセル", Focus::Cancel, Color::Red),
        ] {
            let active = self.focus == focus;
            let style = if active {
                Style::new().fg(Color::Black).bg(color).bold()
            } else {
                Style::new().fg(color)
            };
            let block = Block::bordered()
                .border_type(if active {
                    BorderType::Thick
                } else {
                    BorderType::Rounded
                })
                .border_style(Style::new().fg(color));
            f.render_widget(
                Paragraph::new(label).centered().style(style).block(block),
                rect,
            );
        }
    }
}

fn draw_form(f: &mut Frame, form: &Form) {
    let area = centered(f.area(), 70, 14);
    f.render_widget(Clear, area);
    let title = if form.index.is_some() {
        " コマンドを編集 "
    } else {
        " コマンドを追加 "
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(Color::Cyan))
        .title(title)
        .title_bottom(" Tab/↑↓:項目移動  Space:切替  Enter:保存  Esc:キャンセル ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::vertical([Constraint::Length(1); 10]).split(inner);
    let label = |i: usize, s: &str| {
        let st = if form.field == i {
            Style::new().fg(Color::Cyan).bold()
        } else {
            Style::new()
        };
        Span::styled(
            format!("{:<1}{s}", if form.field == i { "›" } else { " " }),
            st,
        )
    };

    let text_row = |i: usize, name: &str, input: &Input| {
        Line::from(vec![
            label(i, name),
            Span::raw(input.text.clone()).underlined(),
        ])
    };
    f.render_widget(text_row(0, "名前    : ", &form.name), rows[0]);
    f.render_widget(text_row(1, "コマンド: ", &form.command), rows[1]);

    let check = |i: usize, name: &str, v: bool, note: &str| {
        Line::from(vec![
            label(i, &format!("[{}] ", if v { "x" } else { " " })),
            Span::raw(name.to_string()),
            Span::styled(format!("  {note}"), Style::new().dim()),
        ])
    };
    f.render_widget(
        check(
            2,
            "対話",
            form.interactive,
            "標準入力を受け付ける (確認プロンプト等)",
        ),
        rows[3],
    );
    f.render_widget(
        check(3, "最後に実行", form.run_last, "他のコマンドの後に実行"),
        rows[4],
    );
    f.render_widget(
        check(
            4,
            "root権限が必要",
            form.requires_root,
            "sudo を使うコマンド",
        ),
        rows[5],
    );
    f.render_widget(
        check(5, "WSLで実行", form.wsl, "Windows版のみ有効"),
        rows[6],
    );

    if let Some(err) = &form.error {
        f.render_widget(Line::from(err.clone()).fg(Color::Red), rows[8]);
    }

    let input = match form.field {
        0 => Some((&form.name, rows[0])),
        1 => Some((&form.command, rows[1])),
        _ => None,
    };
    if let Some((input, row)) = input {
        let x = row.x + 11 + input.width_before_cursor();
        f.set_cursor_position(Position::new(x.min(row.right().saturating_sub(1)), row.y));
    }
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let [v] = Layout::vertical([Constraint::Length(h)])
        .flex(Flex::Center)
        .areas(area);
    let [r] = Layout::horizontal([Constraint::Percentage(w)])
        .flex(Flex::Center)
        .areas(v);
    r
}
