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
    Run(Option<String>),
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
    fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c) => self.insert(c),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.text.chars().count()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.text.chars().count(),
            _ => {}
        }
    }
    fn width_before_cursor(&self) -> u16 {
        self.text[..self.byte_idx()].width() as u16
    }
}

/// WSLで実行 はWindows版にのみ存在する
const FIELDS: usize = if cfg!(windows) { 7 } else { 5 };

struct Form {
    index: Option<usize>,
    name: Input,
    command: Input,
    interactive: bool,
    run_last: bool,
    requires_root: bool,
    wsl: bool,
    wsl_shell: Input,
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
            wsl_shell: Input::default(),
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
            wsl_shell: Input::new(e.wsl_shell.as_deref().unwrap_or("")),
            root_touched: true,
            field: 0,
            error: None,
        }
    }
    fn text_input(&mut self) -> Option<&mut Input> {
        match self.field {
            0 => Some(&mut self.name),
            1 => Some(&mut self.command),
            6 => Some(&mut self.wsl_shell),
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
    NewProfile(Input),
}

struct App<'a> {
    cfg: &'a mut Config,
    profile: Option<String>,
    path: &'a Path,
    state: ListState,
    focus: Focus,
    mode: Mode,
    message: Option<String>,
}

pub fn run(cfg: &mut Config, path: &Path, profile: Option<String>) -> Result<Action> {
    let mut terminal = ratatui::init();
    let mut app = App {
        state: ListState::default()
            .with_selected((!cfg.commands(profile.as_deref()).is_empty()).then_some(0)),
        cfg,
        profile,
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
    fn entries(&self) -> &[Entry] {
        self.cfg.commands(self.profile.as_deref())
    }

    fn entries_mut(&mut self) -> &mut Vec<Entry> {
        self.cfg.commands_mut(self.profile.as_deref())
    }

    fn select_profile(&mut self, profile: Option<String>) {
        self.profile = profile;
        self.state = ListState::default().with_selected((!self.entries().is_empty()).then_some(0));
        self.focus = Focus::List;
    }

    fn switch_profile(&mut self, forward: bool) {
        let mut profiles = vec![None];
        profiles.extend(self.cfg.profiles.keys().cloned().map(Some));
        let current = profiles
            .iter()
            .position(|name| name == &self.profile)
            .unwrap_or(0);
        let offset = if forward { 1 } else { profiles.len() - 1 };
        self.select_profile(profiles[(current + offset) % profiles.len()].clone());
    }

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
        self.state.selected().filter(|&i| i < self.entries().len())
    }

    fn handle_key(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        self.message = None;
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Normal => return Ok(self.handle_normal(key)),
            Mode::ConfirmDelete => {
                if matches!(key.code, KeyCode::Char('y') | KeyCode::Char('Y')) {
                    if let Some(i) = self.selected() {
                        self.entries_mut().remove(i);
                        if self.entries().is_empty() {
                            self.state.select(None);
                        } else {
                            self.state.select(Some(i.min(self.entries().len() - 1)));
                        }
                        self.save();
                    }
                }
            }
            Mode::NewProfile(mut input) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => {
                    let name = input.text.trim();
                    if let Err(err) = Config::validate_profile_name(name) {
                        self.message = Some(err.to_string());
                        self.mode = Mode::NewProfile(input);
                    } else if self.cfg.profiles.contains_key(name) {
                        self.message = Some("同名のプロファイルがあります".into());
                        self.mode = Mode::NewProfile(input);
                    } else {
                        self.cfg
                            .profiles
                            .insert(name.to_string(), Default::default());
                        self.select_profile(Some(name.to_string()));
                        self.save();
                    }
                }
                _ => {
                    input.handle_key(key);
                    self.mode = Mode::NewProfile(input);
                }
            },
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
            KeyCode::Char('[') => self.switch_profile(false),
            KeyCode::Char(']') => self.switch_profile(true),
            KeyCode::Char('p') => self.mode = Mode::NewProfile(Input::default()),
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
            KeyCode::Char('r') => return Some(Action::Run(self.profile.clone())),
            KeyCode::Enter => match self.focus {
                Focus::Run => return Some(Action::Run(self.profile.clone())),
                Focus::Cancel => return Some(Action::Quit),
                Focus::List => {
                    if let Some(i) = self.selected() {
                        self.mode = Mode::Edit(Form::edit(i, &self.entries()[i]));
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
                    let entry = &mut self.entries_mut()[i];
                    entry.enabled = !entry.enabled;
                    self.save();
                }
            }
            KeyCode::Char('a') => self.mode = Mode::Edit(Form::new()),
            KeyCode::Char('e') => {
                if let Some(i) = self.selected() {
                    self.mode = Mode::Edit(Form::edit(i, &self.entries()[i]));
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
        if j < 0 || j as usize >= self.entries().len() {
            return;
        }
        self.entries_mut().swap(i, j as usize);
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
                    input.handle_key(key);
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
        e.runner = if cfg!(windows) && form.wsl {
            Runner::Wsl
        } else {
            Runner::Native
        };
        let shell = form.wsl_shell.text.trim();
        e.wsl_shell = (!shell.is_empty()).then(|| shell.to_owned());
        match form.index {
            Some(i) => {
                e.enabled = self.entries()[i].enabled;
                self.entries_mut()[i] = e;
            }
            None => {
                self.entries_mut().push(e);
                self.state.select(Some(self.entries().len() - 1));
            }
        }
        self.save();
        true
    }

    // ---------- 描画 ----------

    fn draw(&mut self, f: &mut Frame) {
        let [title, tabs, list, help, buttons] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(3),
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
                format!("  [{}: 対話なしで実行]", privilege::ROOT_LABEL),
                Style::new().fg(Color::Yellow).bold(),
            ));
        }
        f.render_widget(Line::from(t), title);

        let names = std::iter::once((None, "既定")).chain(
            self.cfg
                .profiles
                .keys()
                .map(|name| (Some(name.as_str()), name.as_str())),
        );
        let mut tabs_line = Vec::new();
        let names: Vec<_> = names.collect();
        let selected = names
            .iter()
            .position(|(name, _)| *name == self.profile.as_deref())
            .unwrap_or(0);
        let before_width: usize = names[..selected]
            .iter()
            .map(|(_, label)| label.width() + 4)
            .sum();
        let start = if before_width + names[selected].1.width() + 4 > usize::from(tabs.width) {
            selected
        } else {
            0
        };
        for (name, label) in &names[start..] {
            let span = Span::raw(format!(" {label} "));
            tabs_line.push(if *name == self.profile.as_deref() {
                span.fg(Color::Black).bg(Color::Cyan).bold()
            } else {
                span.dim()
            });
            tabs_line.push(Span::raw("  "));
        }
        f.render_widget(Line::from(tabs_line), tabs);
        self.draw_list(f, list);

        let help_text = match (&self.mode, &self.message) {
            (_, Some(m)) => Line::from(m.clone()).fg(Color::Red),
            (Mode::ConfirmDelete, _) => Line::from("選択中のコマンドを削除しますか? (y/N)").fg(Color::Yellow),
            _ => Line::from(
                "[/]:プロファイル切替  p:プロファイル追加  Tab:フォーカス移動  ↑↓:選択  Space:有効/無効  a:追加  e/Enter:編集  d:削除  J/K:並べ替え  r:実行  q:終了",
            )
            .dim(),
        };
        f.render_widget(
            Paragraph::new(help_text).wrap(ratatui::widgets::Wrap { trim: true }),
            help,
        );

        self.draw_buttons(f, buttons);

        match &self.mode {
            Mode::Edit(form) => draw_form(f, form),
            Mode::NewProfile(input) => {
                let area = centered(f.area(), 70, 3);
                f.render_widget(Clear, area);
                let block =
                    Block::bordered().title(" プロファイル名 (Enter:追加 / Esc:キャンセル) ");
                let inner = block.inner(area);
                f.render_widget(Paragraph::new(input.text.as_str()).block(block), area);
                if inner.width > 0 && inner.height > 0 {
                    f.set_cursor_position(Position::new(
                        inner.x + input.width_before_cursor().min(inner.width - 1),
                        inner.y,
                    ));
                }
            }
            _ => {}
        }
    }

    fn draw_list(&mut self, f: &mut Frame, area: Rect) {
        let name_w = self
            .entries()
            .iter()
            .map(|e| e.name.width())
            .max()
            .unwrap_or(0)
            .min(20);
        let items: Vec<ListItem> = self
            .entries()
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
            if cfg!(windows) {
                "管理者権限が必要"
            } else {
                "root権限が必要"
            },
            form.requires_root,
            if cfg!(windows) {
                "未昇格なら UAC で昇格して実行"
            } else {
                "sudo を使うコマンド"
            },
        ),
        rows[5],
    );
    if cfg!(windows) {
        f.render_widget(
            check(5, "WSLで実行", form.wsl, "WSL のシェルで実行"),
            rows[6],
        );
        f.render_widget(text_row(6, "WSLシェル: ", &form.wsl_shell), rows[7]);
    }

    if let Some(err) = &form.error {
        f.render_widget(Line::from(err.clone()).fg(Color::Red), rows[8]);
    }

    let input = match form.field {
        0 => Some((&form.name, rows[0], "名前    : ")),
        1 => Some((&form.command, rows[1], "コマンド: ")),
        6 => Some((&form.wsl_shell, rows[7], "WSLシェル: ")),
        _ => None,
    };
    if let Some((input, row, label)) = input {
        let x = row.x + 1 + label.width() as u16 + input.width_before_cursor();
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
