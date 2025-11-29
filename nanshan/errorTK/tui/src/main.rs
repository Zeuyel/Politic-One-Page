// 基于 ratatui + crossterm 的简单错题复习 TUI
// 功能：
// - 从 errorTK/backend/data/errors.json 载入题库
// - 默认筛选来源：simulation + real（可通过命令行更改）
// - 列表 + 右侧详情（内容/答案/解析/评论可切换显示）
// - 在界面内将题目标注 new/reviewing/mastered，并回写 JSON

use std::{
    cmp::min,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use anyhow::{Context, Result};
use chrono::Utc;
use clap::{ArgAction, Parser, ValueEnum};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarState,
        Wrap,
    },
    Frame, Terminal,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tui_textarea::{CursorMove, Scrolling, TextArea};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum SourceKind {
    Simulation,
    Real,
    Famous,
}

impl SourceKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Simulation => "simulation",
            Self::Real => "real",
            Self::Famous => "famous",
        }
    }
}

#[derive(Debug, Clone, Parser)]
#[command(name = "errortk-tui", about = "ErrorTK 复习 TUI 工具", version)]
struct Cli {
    /// 数据文件路径，默认读取 errorTK/backend/data/errors.json 或环境变量 ERROR_TK_DATA
    #[arg(long, short = 'f')]
    file: Option<PathBuf>,

    /// 选择来源（可多选），默认 simulation,real
    #[arg(long = "source", short = 's', value_enum, action = ArgAction::Append)]
    sources: Vec<SourceKind>,

    /// 启动时显示评论
    #[arg(long, action = ArgAction::SetTrue)]
    show_comments: bool,

    /// 考试日期（Exam Mode），示例 2025-12-28
    #[arg(long = "exam", value_parser = clap::value_parser!(chrono::NaiveDate))]
    exam_date: Option<chrono::NaiveDate>,

    /// 仅显示到期题目（配合 Exam Mode），默认关闭
    #[arg(long = "due-only", action = ArgAction::SetTrue)]
    due_only: bool,

    /// 每日最大复习题数（0 表示不限制）
    #[arg(long = "daily-limit", default_value_t = 0)]
    daily_limit: usize,

    /// 主题（外观）：dark | light
    #[arg(long = "theme", value_enum, default_value_t = ThemeKind::Dark)]
    theme: ThemeKind,
}

// ---------------- 数据结构 ----------------
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct OptionItem {
    label: String,
    content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
struct Question {
    id: i64,
    origin_name: String,
    sub_name: String,
    #[serde(default)]
    r#type: i32,
    content: String,
    #[serde(default)]
    options: Vec<OptionItem>,
    #[serde(default)]
    answer: Vec<String>,
    #[serde(default)]
    analysis: String,
    #[serde(default)]
    comments: Vec<String>,
    #[serde(default = "default_status")]
    user_status: String,
    #[serde(default)]
    last_reviewed: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    exam: Option<ExamState>,
    #[serde(default)]
    exam_by_cloze: HashMap<String, ExamState>,
}

fn default_status() -> String {
    "new".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Meta {
    last_sync: Option<String>,
    version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ErrorData {
    #[serde(default)]
    meta: Meta,
    #[serde(default)]
    simulation: Vec<Question>,
    #[serde(default)]
    real: Vec<Question>,
    #[serde(default)]
    famous: Vec<Question>,
}

#[derive(Debug, Clone)]
struct RowRef {
    src: SourceKind,
    idx: usize,
}

#[derive(Debug)]
struct App {
    data: ErrorData,
    rows: Vec<RowRef>,
    list_state: ListState,
    show_answer: bool,               // 全局：是否显示答案/解析
    show_comments: bool,             // 全局：是否显示评论
    show_answer_ids: HashSet<i64>,   // 局部：针对单题显示答案
    show_comments_ids: HashSet<i64>, // 局部：针对单题显示评论
    filter_sources: Vec<SourceKind>,
    exam_date: Option<chrono::NaiveDate>,
    due_only: bool,
    daily_limit: Option<usize>,
    theme: Theme,
    keymap: HashMap<char, KeyAction>,
    // Visual 模式与笔记
    focus: Focus,
    mode: Mode,
    cursor_line: usize,
    cursor_col: usize,
    sel_start: Option<(usize, usize)>,
    flat_lines: Vec<String>,
    editor: Option<Editor>,
    notes: NotesStore,
    visual_kind: VisualKind,
    left_panel: LeftPanel,
    list_state_notes: ListState,
    left_width: u16,
    right_scroll: usize,
    right_viewport: usize,
    content_offset: usize,
    textarea: TextArea<'static>,
    // Notes 搜索
    note_search_query: Option<String>,
    note_search_active: bool,
    filtered_note_indices: Vec<usize>,
    note_indent_levels: Vec<usize>,
    note_fold_mode: NotesFoldMode,
    question_search_query: Option<String>,
    question_search_active: bool,
    question_filtered_indices: Vec<usize>,
    // flashcards
    flash_mode: bool,
    flash_cards: Vec<FlashCardSource>,
    flash_pos: usize,
    flash_revealed: bool,
}

impl App {
    fn new(
        data: ErrorData,
        filter_sources: Vec<SourceKind>,
        show_comments: bool,
        exam_date: Option<chrono::NaiveDate>,
        due_only: bool,
        daily_limit: Option<usize>,
        theme: Theme,
        keymap: HashMap<char, KeyAction>,
        notes: NotesStore,
    ) -> Self {
        let mut app = Self {
            data,
            rows: vec![],
            list_state: ListState::default(),
            show_answer: false,
            show_comments,
            show_answer_ids: HashSet::new(),
            show_comments_ids: HashSet::new(),
            filter_sources,
            exam_date,
            due_only,
            daily_limit,
            theme,
            keymap,
            focus: Focus::List,
            mode: Mode::Normal,
            cursor_line: 0,
            cursor_col: 0,
            sel_start: None,
            flat_lines: vec![],
            editor: None,
            notes,
            visual_kind: VisualKind::Char,
            left_panel: LeftPanel::Questions,
            list_state_notes: ListState::default(),
            left_width: 45,
            right_scroll: 0,
            right_viewport: 0,
            content_offset: 0,
            textarea: TextArea::default(),
            note_search_query: None,
            note_search_active: false,
            filtered_note_indices: Vec::new(),
            note_indent_levels: Vec::new(),
            note_fold_mode: NotesFoldMode::Full,
            question_search_query: None,
            question_search_active: false,
            question_filtered_indices: Vec::new(),
            flash_mode: false,
            flash_cards: Vec::new(),
            flash_pos: 0,
            flash_revealed: false,
        };
        app.rebuild_rows();
        app.list_state.select(Some(0));
        rebuild_note_view(&mut app);
        app
    }

    fn rebuild_rows(&mut self) {
        self.rows.clear();
        let include = |k: SourceKind, v: &Vec<Question>| -> bool {
            !v.is_empty() && self.filter_sources.contains(&k)
        };
        let mut tmp: Vec<RowRef> = vec![];
        if include(SourceKind::Simulation, &self.data.simulation) {
            for i in 0..self.data.simulation.len() {
                tmp.push(RowRef {
                    src: SourceKind::Simulation,
                    idx: i,
                });
            }
        }
        if include(SourceKind::Real, &self.data.real) {
            for i in 0..self.data.real.len() {
                tmp.push(RowRef {
                    src: SourceKind::Real,
                    idx: i,
                });
            }
        }
        if include(SourceKind::Famous, &self.data.famous) {
            for i in 0..self.data.famous.len() {
                tmp.push(RowRef {
                    src: SourceKind::Famous,
                    idx: i,
                });
            }
        }
        // Exam Mode: 仅显示到期 + 排序 + 限流
        if self.due_only {
            let now = chrono::Utc::now();
            tmp.retain(|rr| {
                let q = self.get_question(rr);
                if let Some(ex) = &q.exam {
                    if let Some(due) = &ex.due {
                        return parse_rfc3339(due).map(|d| d <= now).unwrap_or(false);
                    }
                }
                false
            });
        }
        // 排序：按 due（无 due 置后）+ priority（默认 1）
        tmp.sort_by(|a, b| {
            let qa = self.get_question(a);
            let qb = self.get_question(b);
            let da = qa
                .exam
                .as_ref()
                .and_then(|e| e.due.as_ref())
                .and_then(|s| parse_rfc3339(s));
            let db = qb
                .exam
                .as_ref()
                .and_then(|e| e.due.as_ref())
                .and_then(|s| parse_rfc3339(s));
            match (da, db) {
                (Some(x), Some(y)) => x.cmp(&y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
        });
        if let Some(limit) = self.daily_limit {
            if limit > 0 && tmp.len() > limit {
                tmp.truncate(limit);
            }
        }
        self.rows = tmp;
        if self.rows.is_empty() {
            self.list_state.select(None);
        } else if self.list_state.selected().is_none() {
            self.list_state.select(Some(0));
        }
        refresh_question_filter(self);
    }

    fn get_question_mut(&mut self, r: &RowRef) -> &mut Question {
        match r.src {
            SourceKind::Simulation => &mut self.data.simulation[r.idx],
            SourceKind::Real => &mut self.data.real[r.idx],
            SourceKind::Famous => &mut self.data.famous[r.idx],
        }
    }

    fn get_question(&self, r: &RowRef) -> &Question {
        match r.src {
            SourceKind::Simulation => &self.data.simulation[r.idx],
            SourceKind::Real => &self.data.real[r.idx],
            SourceKind::Famous => &self.data.famous[r.idx],
        }
    }

    fn selected_ref(&self) -> Option<&RowRef> {
        let selected = self.list_state.selected()?;
        let idx = if self.question_search_active {
            self.question_filtered_indices.get(selected).copied()
        } else {
            Some(selected)
        }?;
        self.rows.get(idx)
    }

    fn status_counts(&self) -> (usize, usize, usize) {
        let mut n = 0;
        let mut r = 0;
        let mut m = 0;
        for rr in &self.rows {
            let q = self.get_question(rr);
            match q.user_status.as_str() {
                "new" => n += 1,
                "reviewing" => r += 1,
                "mastered" => m += 1,
                _ => n += 1,
            }
        }
        (n, r, m)
    }
}

fn default_data_path(cli: &Cli) -> PathBuf {
    if let Some(p) = &cli.file {
        return p.clone();
    }
    if let Ok(envp) = std::env::var("ERROR_TK_DATA") {
        return PathBuf::from(envp);
    }

    // 自动探测：从当前目录向上查找常见路径
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        for anc in cwd.ancestors() {
            candidates.push(anc.join("errorTK/backend/data/errors.json"));
            candidates.push(anc.join("backend/data/errors.json"));
        }
    }
    // 常见相对路径兜底
    candidates.push(PathBuf::from("errorTK/backend/data/errors.json"));
    candidates.push(PathBuf::from("../backend/data/errors.json"));

    for c in candidates {
        if c.exists() {
            return c;
        }
    }
    // 最后返回默认路径（可能不存在，load 时会给出清晰错误）
    PathBuf::from("errorTK/backend/data/errors.json")
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ReviewEvent {
    ts: String,
    grade: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ExamState {
    stage: u8,
    again_streak: u8,
    priority: u8,
    due: Option<String>,
    history: Vec<ReviewEvent>,
}

fn default_exam_state() -> ExamState {
    ExamState {
        stage: 0,
        again_streak: 0,
        priority: 1,
        due: None,
        history: vec![],
    }
}

fn apply_exam_grade(ex: &mut ExamState, grade: &str, exam_date: Option<chrono::NaiveDate>) {
    let now = Utc::now();
    let again_seq: [f64; 3] = [10.0 / 1440.0, 4.0 / 24.0, 1.0];
    let hard_seq: [f64; 5] = [1.0, 3.0, 7.0, 14.0, 28.0];
    let good_seq: [f64; 4] = [2.0, 5.0, 12.0, 25.0];
    let easy_seq: [f64; 3] = [4.0, 10.0, 24.0];

    let mut next_days = match grade {
        "again" => {
            ex.again_streak = (ex.again_streak.saturating_add(1)).min(3);
            ex.stage = ex.stage.saturating_sub(1);
            again_seq[(ex.again_streak as usize - 1).min(again_seq.len() - 1)]
        }
        "hard" => {
            ex.again_streak = 0;
            let i = (ex.stage as usize).min(hard_seq.len() - 1);
            ex.stage = ex.stage.saturating_add(1);
            hard_seq[i]
        }
        "good" => {
            ex.again_streak = 0;
            let i = (ex.stage as usize).min(good_seq.len() - 1);
            ex.stage = ex.stage.saturating_add(1);
            good_seq[i]
        }
        "easy" => {
            ex.again_streak = 0;
            let i = (ex.stage as usize).min(easy_seq.len() - 1);
            ex.stage = ex.stage.saturating_add(1);
            easy_seq[i]
        }
        _ => 2.0,
    };

    if let Some(ed) = exam_date {
        let rest_days = (ed
            .and_hms_opt(7, 0, 0)
            .unwrap_or_else(|| ed.and_hms_milli_opt(0, 0, 0, 0).unwrap())
            .and_utc()
            - now)
            .num_seconds() as f64
            / 86400.0;
        if rest_days > 0.0 {
            next_days = next_days.min((rest_days - 2.0).max(again_seq[0]));
        } else {
            next_days = again_seq[0];
        }
    }

    let due_dt = now + days_to_duration(next_days);
    ex.due = Some(to_rfc3339(due_dt));
    ex.history.push(ReviewEvent {
        ts: to_rfc3339(now),
        grade: grade.to_string(),
    });
}

fn load_data(path: &PathBuf) -> Result<ErrorData> {
    if !path.exists() {
        let tip = format!(
            "读取数据文件失败: {}\n提示: 使用 --file ../backend/data/errors.json 或设置环境变量 ERROR_TK_DATA 指向正确路径。",
            path.display()
        );
        return Err(anyhow::anyhow!(tip));
    }
    let s = fs::read_to_string(path)
        .with_context(|| format!("读取数据文件失败: {}", path.display()))?;
    let mut d: ErrorData = serde_json::from_str(&s).context("解析 JSON 失败")?;
    // 兼容：补齐来源字段，便于过滤
    for q in &mut d.simulation {
        if q.source.is_none() {
            q.source = Some("simulation".into());
        }
    }
    for q in &mut d.real {
        if q.source.is_none() {
            q.source = Some("real".into());
        }
    }
    for q in &mut d.famous {
        if q.source.is_none() {
            q.source = Some("famous".into());
        }
    }
    // 兼容：补齐 exam 字段
    for q in d
        .simulation
        .iter_mut()
        .chain(d.real.iter_mut())
        .chain(d.famous.iter_mut())
    {
        if q.exam.is_none() {
            q.exam = Some(default_exam_state());
        }
    }
    Ok(d)
}

fn save_data(path: &PathBuf, d: &ErrorData) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let s = serde_json::to_string_pretty(d)?;
    fs::write(path, s).with_context(|| format!("写入数据文件失败: {}", path.display()))?;
    Ok(())
}

fn parse_rfc3339(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn to_rfc3339(dt: chrono::DateTime<Utc>) -> String {
    dt.to_rfc3339()
}

fn days_to_duration(days: f64) -> chrono::Duration {
    let secs = (days * 86400.0).max(0.0);
    chrono::Duration::seconds(secs as i64)
}

fn grade_and_schedule(app: &mut App, data_path: &PathBuf, grade: &str) -> Result<()> {
    if let Some(idx) = app.list_state.selected() {
        let rr = app.rows[idx].clone();
        let now = Utc::now();
        let exam_date = app.exam_date;
        let q = app.get_question_mut(&rr);
        let mut ex = q.exam.clone().unwrap_or_else(default_exam_state);
        apply_exam_grade(&mut ex, grade, exam_date);
        q.exam = Some(ex);

        // 联动状态：多次 Good/Easy 推进到 mastered；Again 退到 reviewing/new
        match grade {
            "again" => {
                q.user_status = if q.user_status == "new" {
                    "new".into()
                } else {
                    "reviewing".into()
                };
            }
            "hard" => {
                if q.user_status == "new" {
                    q.user_status = "reviewing".into();
                }
            }
            "good" | "easy" => {
                if q.user_status != "mastered" {
                    q.user_status = "reviewing".into();
                }
            }
            _ => {}
        }
        q.last_reviewed = Some(to_rfc3339(now));
        save_data(data_path, &app.data)?;
        // 评分后若仅看到期，需要重建列表以便下一题顶上来
        if app.due_only {
            app.rebuild_rows();
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let data_path = default_data_path(&cli);
    let sources = if cli.sources.is_empty() {
        vec![SourceKind::Simulation, SourceKind::Real]
    } else {
        cli.sources.clone()
    };
    let data = load_data(&data_path)?;
    let keymap = load_keymap().unwrap_or_else(|_| default_keymap());
    let notes_path = data_path
        .parent()
        .map(|p| p.join("notes.json"))
        .unwrap_or_else(|| PathBuf::from("notes.json"));
    let notes = NotesStore::open(notes_path)?;

    // TUI 初始化
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(
        data,
        sources,
        cli.show_comments,
        cli.exam_date,
        cli.due_only,
        if cli.daily_limit > 0 {
            Some(cli.daily_limit)
        } else {
            None
        },
        theme_of(cli.theme),
        keymap,
        notes,
    );
    let res = run_app(&mut terminal, &mut app, &data_path);

    // 退出还原
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    res
}

fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    data_path: &PathBuf,
) -> Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;
        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(k) => {
                    // 编辑器模式下，直接交给编辑器处理
                    if let Some(ed) = app.editor.as_mut() {
                        if handle_editor_key(ed, &k) {
                            // true 表示已保存/退出
                            let saved = ed.saved;
                            let content = ed.buffer.clone();
                            if saved {
                                if let Some(idx) = ed.target_note_index {
                                    if let Some(n) = app.notes.data.notes.get_mut(idx) {
                                        n.content = content;
                                        n.updated_at = Utc::now().to_rfc3339();
                                    }
                                    app.notes.save()?;
                                    rebuild_note_view(app);
                                } else if let (Some(qid), Some(excerpt)) =
                                    (ed.new_note_qid, ed.new_note_excerpt.clone())
                                {
                                    app.notes.add_note(qid, excerpt, content)?;
                                    rebuild_note_view(app);
                                } // 否则忽略
                            }
                            app.editor = None;
                        }
                        continue;
                    }
                    if handle_key(app, k, data_path)? {
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn handle_key(app: &mut App, key: KeyEvent, data_path: &PathBuf) -> Result<bool> {
    let KeyEvent { code, .. } = key;
    match code {
        KeyCode::Char('q') => {
            if app.flash_mode {
                app.flash_mode = false;
                return Ok(false);
            }
            if app.focus == Focus::Text {
                exit_text_focus(app);
            } else {
                return Ok(true);
            }
        }
        KeyCode::Down => match app.left_panel {
            LeftPanel::Questions => {
                let n = question_visible_count(app);
                if n > 0 {
                    if let Some(sel) = app.list_state.selected() {
                        app.list_state.select(Some(min(sel + 1, n - 1)));
                    } else {
                        app.list_state.select(Some(0));
                    }
                }
            }
            LeftPanel::Notes => move_note_selection(app, 1),
        },
        KeyCode::Up => match app.left_panel {
            LeftPanel::Questions => {
                if let Some(sel) = app.list_state.selected() {
                    if sel > 0 {
                        app.list_state.select(Some(sel - 1));
                    }
                }
            }
            LeftPanel::Notes => move_note_selection(app, -1),
        },
        KeyCode::Enter => {
            if app.note_search_active && matches!(app.left_panel, LeftPanel::Notes) {
                app.note_search_active = false;
                rebuild_note_view(app);
            } else if app.question_search_active && matches!(app.left_panel, LeftPanel::Questions) {
                app.question_search_active = false;
                app.question_search_query = None;
                refresh_question_filter(app);
            } else {
                match app.left_panel {
                    LeftPanel::Questions => apply_action(app, data_path, KeyAction::EnterText)?,
                    LeftPanel::Notes => apply_action(app, data_path, KeyAction::NoteOpen)?,
                }
            }
        }
        KeyCode::Esc => {
            if app.note_search_active && matches!(app.left_panel, LeftPanel::Notes) {
                app.note_search_active = false;
                app.note_search_query = None;
                rebuild_note_view(app);
            } else if app.question_search_active && matches!(app.left_panel, LeftPanel::Questions) {
                app.question_search_active = false;
                app.question_search_query = None;
                refresh_question_filter(app);
            } else {
                apply_action(app, data_path, KeyAction::ExitText)?;
            }
        }
        KeyCode::Tab => {
            apply_action(app, data_path, KeyAction::SwitchLeftPanel)?;
        }
        KeyCode::Char('<') => {
            apply_action(app, data_path, KeyAction::ResizeLeftShrink)?;
        }
        KeyCode::Char('>') => {
            apply_action(app, data_path, KeyAction::ResizeLeftExpand)?;
        }
        KeyCode::Char('/') => {
            if matches!(app.left_panel, LeftPanel::Notes) {
                app.note_search_active = true;
                app.note_search_query = Some(String::new());
                rebuild_note_view(app);
            } else if matches!(app.left_panel, LeftPanel::Questions) {
                app.question_search_active = true;
                app.question_search_query = Some(String::new());
                refresh_question_filter(app);
            }
        }
        KeyCode::Char('j') => {
            if app.focus == Focus::Text {
                app.textarea.move_cursor(CursorMove::Down);
                let n = app.flat_lines.len();
                if n > 0 {
                    app.cursor_line = (app.cursor_line + 1).min(n - 1);
                    let len = app
                        .flat_lines
                        .get(app.cursor_line)
                        .map(|s| s.chars().count())
                        .unwrap_or(0);
                    if app.cursor_col > len {
                        app.cursor_col = len;
                    }
                }
            } else if matches!(app.left_panel, LeftPanel::Questions) {
                let n = question_visible_count(app);
                if let Some(sel) = app.list_state.selected() {
                    if n > 0 {
                        app.list_state.select(Some(min(sel + 1, n - 1)));
                    }
                } else if n > 0 {
                    app.list_state.select(Some(0));
                }
            } else if matches!(app.left_panel, LeftPanel::Notes) {
                move_note_selection(app, 1);
            }
        }
        KeyCode::Char('k') => {
            if app.focus == Focus::Text {
                app.textarea.move_cursor(CursorMove::Up);
                if app.cursor_line > 0 {
                    app.cursor_line -= 1;
                    let len = app
                        .flat_lines
                        .get(app.cursor_line)
                        .map(|s| s.chars().count())
                        .unwrap_or(0);
                    if app.cursor_col > len {
                        app.cursor_col = len;
                    }
                }
            } else if matches!(app.left_panel, LeftPanel::Questions) {
                let n = question_visible_count(app);
                if let Some(sel) = app.list_state.selected() {
                    if sel > 0 {
                        app.list_state.select(Some(sel - 1));
                    }
                } else if n > 0 {
                    app.list_state.select(Some(0));
                }
            } else if matches!(app.left_panel, LeftPanel::Notes) {
                move_note_selection(app, -1);
            }
        }
        KeyCode::Char('h') => {
            if app.focus == Focus::Text {
                app.textarea.move_cursor(CursorMove::Back);
                if app.cursor_col > 0 {
                    app.cursor_col -= 1;
                }
            }
        }
        KeyCode::Char('l') => {
            if app.focus == Focus::Text {
                app.textarea.move_cursor(CursorMove::Forward);
                let len = app
                    .flat_lines
                    .get(app.cursor_line)
                    .map(|s| s.chars().count())
                    .unwrap_or(0);
                if app.cursor_col < len {
                    app.cursor_col += 1;
                }
            }
        }
        // handled above in unconditional 'j'/'k'
        KeyCode::Char('v') if app.flash_mode => {
            flash_grade(app, data_path, "easy")?;
        }
        KeyCode::Char('V') => {
            if app.focus == Focus::Text {
                app.mode = Mode::Visual;
                app.visual_kind = VisualKind::Line;
                app.sel_start = Some((app.cursor_line, 0));
                app.textarea.start_selection();
            }
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.textarea.scroll(Scrolling::HalfPageDown);
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.textarea.scroll(Scrolling::HalfPageUp);
        }
        KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if app.focus == Focus::Text {
                app.textarea.move_cursor(CursorMove::Down);
            }
        }
        KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if app.focus == Focus::Text {
                app.textarea.move_cursor(CursorMove::Up);
            }
        }
        KeyCode::Char('F') => {
            flash_toggle(app);
        }
        KeyCode::Char(' ') if app.flash_mode => {
            flash_reveal(app);
        }
        KeyCode::Char('n') if app.flash_mode => {
            flash_next(app);
        }
        KeyCode::Char('p') if app.flash_mode => {
            flash_prev(app);
        }
        KeyCode::Char('z') if app.flash_mode => {
            flash_grade(app, data_path, "again")?;
        }
        KeyCode::Char('x') if app.flash_mode => {
            flash_grade(app, data_path, "hard")?;
        }
        KeyCode::Char('g') if app.flash_mode => {
            flash_grade(app, data_path, "good")?;
        }
        KeyCode::Char('v') if app.flash_mode => {
            flash_grade(app, data_path, "easy")?;
        }
        KeyCode::Char('v') => {
            if app.focus == Focus::Text {
                app.mode = Mode::Visual;
                app.visual_kind = VisualKind::Char;
                app.sel_start = Some((app.cursor_line, app.cursor_col));
                app.textarea.start_selection();
            }
        }
        KeyCode::Char(ch) => {
            if app.note_search_active && matches!(app.left_panel, LeftPanel::Notes) {
                let s = app.note_search_query.get_or_insert(String::new());
                s.push(ch);
                rebuild_note_view(app);
                return Ok(false);
            } else if app.question_search_active && matches!(app.left_panel, LeftPanel::Questions) {
                let s = app.question_search_query.get_or_insert(String::new());
                s.push(ch);
                refresh_question_filter(app);
                return Ok(false);
            }
            if let Some(action) = app.keymap.get(&ch).cloned() {
                apply_action(app, data_path, action)?;
            }
        }
        KeyCode::Backspace => {
            if app.note_search_active && matches!(app.left_panel, LeftPanel::Notes) {
                if let Some(s) = app.note_search_query.as_mut() {
                    s.pop();
                }
                rebuild_note_view(app);
            } else if app.question_search_active && matches!(app.left_panel, LeftPanel::Questions) {
                if let Some(s) = app.question_search_query.as_mut() {
                    s.pop();
                }
                refresh_question_filter(app);
            }
        }
        // Flashcards 快捷键
        _ => {}
    }
    Ok(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyAction {
    ToggleAnswerCurrent,
    ToggleAnswerGlobal,
    ToggleCommentsCurrent,
    ToggleCommentsGlobal,
    ToggleSourceSim,
    ToggleSourceReal,
    ToggleSourceFamous,
    MarkNew,
    MarkReviewing,
    MarkMastered,
    GradeAgain,
    GradeHard,
    GradeGood,
    GradeEasy,
    ToggleDueOnly,
    Reload,
    // Visual/Notes
    VisualToggle,
    VisualLineToggle,
    EnterText,
    ExitText,
    MoveLeft,
    MoveRight,
    MoveUpDetail,
    MoveDownDetail,
    YankToNote,
    // Panes / Notes
    SwitchLeftPanel,
    ResizeLeftShrink,
    ResizeLeftExpand,
    ToggleNotesFold,
    RunScraper,
    NoteOpen,
    NoteEdit,
    NoteDelete,
    ScrollPageDown,
    ScrollPageUp,
    ScrollLineDown,
    ScrollLineUp,
    // Flashcards
    FlashStart,
    FlashReveal,
    FlashNext,
    FlashPrev,
}

fn apply_action(app: &mut App, data_path: &PathBuf, action: KeyAction) -> Result<()> {
    match action {
        KeyAction::ToggleAnswerCurrent => {
            if let Some(rr) = app.selected_ref() {
                let id = app.get_question(rr).id;
                if !app.show_answer_ids.insert(id) {
                    app.show_answer_ids.remove(&id);
                }
            }
        }
        KeyAction::ToggleAnswerGlobal => {
            app.show_answer = !app.show_answer;
        }
        KeyAction::ToggleCommentsCurrent => {
            if let Some(rr) = app.selected_ref() {
                let id = app.get_question(rr).id;
                if !app.show_comments_ids.insert(id) {
                    app.show_comments_ids.remove(&id);
                }
            }
        }
        KeyAction::ToggleCommentsGlobal => {
            app.show_comments = !app.show_comments;
        }
        KeyAction::ToggleSourceSim => toggle_source(app, SourceKind::Simulation),
        KeyAction::ToggleSourceReal => toggle_source(app, SourceKind::Real),
        KeyAction::ToggleSourceFamous => toggle_source(app, SourceKind::Famous),
        KeyAction::MarkNew => set_status_and_save(app, data_path, "new")?,
        KeyAction::MarkReviewing => set_status_and_save(app, data_path, "reviewing")?,
        KeyAction::MarkMastered => set_status_and_save(app, data_path, "mastered")?,
        KeyAction::GradeAgain => {
            if matches!(app.left_panel, LeftPanel::Notes) {
                grade_note(app, "again")?;
            } else {
                grade_and_schedule(app, data_path, "again")?;
            }
        }
        KeyAction::GradeHard => {
            if matches!(app.left_panel, LeftPanel::Notes) {
                grade_note(app, "hard")?;
            } else {
                grade_and_schedule(app, data_path, "hard")?;
            }
        }
        KeyAction::GradeGood => {
            if matches!(app.left_panel, LeftPanel::Notes) {
                grade_note(app, "good")?;
            } else {
                grade_and_schedule(app, data_path, "good")?;
            }
        }
        KeyAction::GradeEasy => {
            if matches!(app.left_panel, LeftPanel::Notes) {
                grade_note(app, "easy")?;
            } else {
                grade_and_schedule(app, data_path, "easy")?;
            }
        }
        KeyAction::ToggleDueOnly => {
            app.due_only = !app.due_only;
            app.rebuild_rows();
        }
        KeyAction::Reload => {
            let d = load_data(data_path)?;
            app.data = d;
            app.rebuild_rows();
        }
        KeyAction::VisualToggle => toggle_visual_char(app),
        KeyAction::VisualLineToggle => toggle_visual_line(app),
        KeyAction::EnterText => enter_text_focus(app),
        KeyAction::ExitText => exit_text_focus(app),
        KeyAction::MoveLeft => move_cursor(app, 0, -1),
        KeyAction::MoveRight => move_cursor(app, 0, 1),
        KeyAction::MoveUpDetail => move_cursor(app, -1, 0),
        KeyAction::MoveDownDetail => move_cursor(app, 1, 0),
        KeyAction::YankToNote => yank_to_note(app)?,
        KeyAction::SwitchLeftPanel => switch_left_panel(app),
        KeyAction::ResizeLeftShrink => resize_left(app, -5),
        KeyAction::ResizeLeftExpand => resize_left(app, 5),
        KeyAction::ToggleNotesFold => toggle_notes_fold(app),
        KeyAction::RunScraper => run_scraper(app, data_path)?,
        KeyAction::NoteOpen => note_open_right(app),
        KeyAction::NoteEdit => note_edit(app),
        KeyAction::NoteDelete => note_delete(app)?,
        KeyAction::ScrollPageDown => {
            scroll_right(app, app.right_viewport.saturating_div(2).max(1) as isize)
        }
        KeyAction::ScrollPageUp => {
            scroll_right(app, -(app.right_viewport.saturating_div(2).max(1) as isize))
        }
        KeyAction::ScrollLineDown => scroll_right(app, 1),
        KeyAction::ScrollLineUp => scroll_right(app, -1),
        KeyAction::FlashStart => flash_start(app),
        KeyAction::FlashReveal => flash_reveal(app),
        KeyAction::FlashNext => flash_next(app),
        KeyAction::FlashPrev => flash_prev(app),
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    Visual,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    List,
    Text,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeftPanel {
    Questions,
    Notes,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VisualKind {
    Char,
    Line,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NotesFoldMode {
    Full,
    CurrentParent,
}

fn toggle_visual_char(app: &mut App) {
    if app.focus != Focus::Text {
        enter_text_focus(app);
    }
    match app.mode {
        Mode::Normal => {
            app.mode = Mode::Visual;
            app.visual_kind = VisualKind::Char;
            app.sel_start = Some((app.cursor_line, app.cursor_col));
        }
        Mode::Visual => {
            app.mode = Mode::Normal;
            app.sel_start = None;
        }
    }
}

fn toggle_visual_line(app: &mut App) {
    if app.focus != Focus::Text {
        enter_text_focus(app);
    }
    match app.mode {
        Mode::Normal => {
            app.mode = Mode::Visual;
            app.visual_kind = VisualKind::Line;
            app.sel_start = Some((app.cursor_line, 0));
            app.cursor_col = app
                .flat_lines
                .get(app.cursor_line)
                .map(|s| s.chars().count())
                .unwrap_or(0);
        }
        Mode::Visual => {
            app.mode = Mode::Normal;
            app.sel_start = None;
        }
    }
}

fn rebuild_flat_lines(app: &mut App) {
    let mut lines = Vec::new();
    if let Some(rr) = app.selected_ref() {
        let q = app.get_question(rr);
        // 将题干/选项/答案/解析/评论统一为“行缓冲”，便于像 Vim 一样移动
        lines.extend(q.content.split('\n').map(|s| s.to_string()));
        if !q.options.is_empty() {
            for o in &q.options {
                lines.push(format!("{}. {}", o.label, o.content));
            }
        }
        if !q.answer.is_empty() {
            lines.push(format!("答案: {}", q.answer.join(", ")));
        }
        if !q.analysis.is_empty() {
            lines.extend(q.analysis.split('\n').map(|s| s.to_string()));
        }
        if !q.comments.is_empty() {
            lines.push("评论:".into());
            for c in &q.comments {
                lines.extend(c.split('\n').map(|s| format!("- {}", s)));
            }
        }
    }
    if lines.is_empty() {
        lines.push(String::from("(无内容)"));
    }
    app.flat_lines = lines;
    app.cursor_line = 0;
    app.cursor_col = 0;
}

fn enter_text_focus(app: &mut App) {
    app.focus = Focus::Text;
    app.mode = Mode::Normal;
    rebuild_flat_lines(app);
    // 初始化 TextArea 内容（标题 + 来源 + 空行 + 内容）
    if let Some(rr) = app.selected_ref() {
        let q = app.get_question(rr);
        let mut text_lines: Vec<String> = Vec::new();
        text_lines.push(format!(
            "ID:{}  来源:{}  状态:{}",
            q.id,
            q.source.clone().unwrap_or_else(|| rr.src.as_str().into()),
            q.user_status
        ));
        text_lines.push(String::new());
        text_lines.push(format!("{} - {}", q.origin_name, q.sub_name));
        text_lines.push(String::new());
        text_lines.extend(app.flat_lines.clone());
        app.textarea = TextArea::from(text_lines);
        app.content_offset = 4;
    } else {
        app.textarea = TextArea::from(vec!["(无内容)".to_string()]);
        app.content_offset = 0;
    }
    // 基本样式
    app.textarea
        .set_block(ratatui::widgets::block::Block::default());
    app.textarea.set_cursor_line_style(Style::default());
    app.textarea
        .set_cursor_style(Style::default().bg(app.theme.accent).fg(Color::Black));
    app.textarea
        .set_selection_style(Style::default().bg(app.theme.selection_bg));
    // 将光标移动到 TextArea 对应位置（头部四行偏移）
    let row: u16 = (4 + app.cursor_line).try_into().unwrap_or(u16::MAX);
    let col: u16 = (app.cursor_col).try_into().unwrap_or(u16::MAX);
    app.textarea.move_cursor(CursorMove::Jump(row, col));
}

fn exit_text_focus(app: &mut App) {
    app.focus = Focus::List;
    app.mode = Mode::Normal;
    app.sel_start = None;
    app.cursor_line = 0;
    app.cursor_col = 0;
    app.content_offset = 0;
    app.right_scroll = 0;
}

fn move_cursor(app: &mut App, dline: isize, dcol: isize) {
    if app.focus != Focus::Text {
        return;
    }
    let nlines = app.flat_lines.len();
    if nlines == 0 {
        return;
    }
    let mut line = app.cursor_line as isize + dline;
    line = line.clamp(0, (nlines as isize - 1).max(0));
    app.cursor_line = line as usize;
    let max_col = app.flat_lines[app.cursor_line].chars().count();
    let mut col = app.cursor_col as isize + dcol;
    col = col.clamp(0, (max_col as isize).max(0));
    app.cursor_col = col as usize;
    // 自然滚动：光标越界时调整右侧滚动位置（按显示行：content_offset + cursor_line）
    let vp = app.right_viewport.max(1);
    let anchor = app.content_offset.saturating_add(app.cursor_line);
    let total_lines = app.content_offset.saturating_add(app.flat_lines.len());
    let max_top = total_lines.saturating_sub(vp);
    let mut new_top = app.right_scroll;
    if anchor < app.right_scroll {
        new_top = anchor;
    } else if anchor > app.right_scroll.saturating_add(vp).saturating_sub(1) {
        new_top = anchor.saturating_sub(vp.saturating_sub(1));
    }
    if new_top > max_top {
        new_top = max_top;
    }
    app.right_scroll = new_top;
}

fn yank_to_note(app: &mut App) -> Result<()> {
    if app.mode != Mode::Visual {
        return Ok(());
    }
    let (sline, scol, eline, ecol) = if let Some((sl, sc)) = app.sel_start {
        let el = app.cursor_line;
        let ec = app.cursor_col;
        if (el, ec) >= (sl, sc) {
            (sl, sc, el, ec)
        } else {
            (el, ec, sl, sc)
        }
    } else {
        return Ok(());
    };
    // 提取选中文本
    let mut out = String::new();
    if matches!(app.visual_kind, VisualKind::Line) {
        for i in sline..=eline {
            out.push_str(app.flat_lines.get(i).map(|s| s.as_str()).unwrap_or(""));
            if i != eline {
                out.push('\n');
            }
        }
    } else {
        for i in sline..=eline {
            let line = app.flat_lines.get(i).cloned().unwrap_or_default();
            let chars: Vec<char> = line.chars().collect();
            let (start, end) = if i == sline && i == eline {
                (scol.min(chars.len()), ecol.min(chars.len()))
            } else if i == sline {
                (scol.min(chars.len()), chars.len())
            } else if i == eline {
                (0, ecol.min(chars.len()))
            } else {
                (0, chars.len())
            };
            if start < end {
                out.push_str(&chars[start..end].iter().collect::<String>());
            }
            if i != eline {
                out.push('\n');
            }
        }
    }
    // 打开编辑器（预填为选中文本）
    if let Some(rr) = app.selected_ref() {
        let qid = app.get_question(rr).id;
        app.editor = Some(Editor::new_new(qid, out.clone()));
    } else {
        app.editor = Some(Editor::new_edit(out.clone(), 0));
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct Editor {
    buffer: String,
    // initial: String, // 不再使用
    saved: bool,
    cursor: usize,
    // 目标：新建或编辑
    target_note_index: Option<usize>,
    new_note_qid: Option<i64>,
    new_note_excerpt: Option<String>,
}
impl Editor {
    fn new_new(qid: i64, excerpt: String) -> Self {
        let cur = excerpt.chars().count();
        Self {
            buffer: excerpt.clone(),
            saved: false,
            cursor: cur,
            target_note_index: None,
            new_note_qid: Some(qid),
            new_note_excerpt: Some(excerpt),
        }
    }
    fn new_edit(content: String, idx: usize) -> Self {
        let cur = content.chars().count();
        Self {
            buffer: content.clone(),
            saved: false,
            cursor: cur,
            target_note_index: Some(idx),
            new_note_qid: None,
            new_note_excerpt: None,
        }
    }
}

fn handle_editor_key(ed: &mut Editor, k: &KeyEvent) -> bool {
    match (k.code, k.modifiers) {
        (KeyCode::Esc, _) => {
            ed.saved = false;
            return true;
        }
        (KeyCode::Char('s'), KeyModifiers::CONTROL) => {
            ed.saved = true;
            return true;
        }
        (KeyCode::Enter, _) => {
            insert_char(ed, '\n');
        }
        (KeyCode::Backspace, _) => {
            backspace(ed);
        }
        (KeyCode::Left, _) => {
            if ed.cursor > 0 {
                ed.cursor -= 1;
            }
        }
        (KeyCode::Right, _) => {
            if ed.cursor < ed.buffer.chars().count() {
                ed.cursor += 1;
            }
        }
        (KeyCode::Char(ch), _) => {
            insert_char(ed, ch);
        }
        _ => {}
    }
    false
}

fn insert_char(ed: &mut Editor, ch: char) {
    let mut v: Vec<char> = ed.buffer.chars().collect();
    let pos = ed.cursor.min(v.len());
    v.insert(pos, ch);
    ed.cursor += 1;
    ed.buffer = v.into_iter().collect();
}

fn backspace(ed: &mut Editor) {
    if ed.cursor == 0 {
        return;
    }
    let mut v: Vec<char> = ed.buffer.chars().collect();
    let pos = ed.cursor - 1;
    v.remove(pos);
    ed.cursor -= 1;
    ed.buffer = v.into_iter().collect();
}

fn toggle_source(app: &mut App, k: SourceKind) {
    if let Some(pos) = app.filter_sources.iter().position(|x| *x == k) {
        app.filter_sources.remove(pos);
    } else {
        app.filter_sources.push(k);
    }
    if app.filter_sources.is_empty() {
        app.filter_sources = vec![SourceKind::Simulation, SourceKind::Real];
    }
    app.rebuild_rows();
}

fn switch_left_panel(app: &mut App) {
    app.left_panel = match app.left_panel {
        LeftPanel::Questions => LeftPanel::Notes,
        LeftPanel::Notes => LeftPanel::Questions,
    };
    match app.left_panel {
        LeftPanel::Notes => {
            if app.list_state_notes.selected().is_none() && note_visible_count(app) > 0 {
                app.list_state_notes.select(Some(0));
            }
            rebuild_note_view(app);
        }
        LeftPanel::Questions => {
            if app.list_state.selected().is_none() && !app.rows.is_empty() {
                app.list_state.select(Some(0));
            }
            refresh_question_filter(app);
        }
    }
}

fn resize_left(app: &mut App, delta: i16) {
    let w = app.left_width as i16 + delta;
    app.left_width = w.clamp(20, 80) as u16;
}

fn toggle_notes_fold(app: &mut App) {
    app.note_fold_mode = match app.note_fold_mode {
        NotesFoldMode::Full => NotesFoldMode::CurrentParent,
        NotesFoldMode::CurrentParent => NotesFoldMode::Full,
    };
    rebuild_note_view(app);
}

fn note_open_right(app: &mut App) {
    if let Some(note) = current_note(app) {
        let mut target_index: Option<usize> = None;
        for (i, rr) in app.rows.iter().enumerate() {
            let q = app.get_question(rr);
            if q.id == note.qid {
                target_index = Some(i);
                break;
            }
        }
        if let Some(i) = target_index {
            app.list_state.select(Some(i));
            app.left_panel = LeftPanel::Questions;
            enter_text_focus(app);
        }
    }
}

fn note_edit(app: &mut App) {
    if let Some(idx) = current_note_index(app) {
        if let Some(n) = app.notes.data.notes.get(idx) {
            app.editor = Some(Editor::new_edit(n.content.clone(), idx));
        }
    }
}

fn note_delete(app: &mut App) -> Result<()> {
    if let Some(idx) = current_note_index(app) {
        if idx < app.notes.data.notes.len() {
            app.notes.data.notes.remove(idx);
            app.notes.save()?;
            rebuild_note_view(app);
        }
    }
    Ok(())
}

fn scroll_right(app: &mut App, delta: isize) {
    let max_lines: isize = if matches!(app.left_panel, LeftPanel::Notes) {
        current_note(app)
            .map(|n| n.content.lines().count() as isize)
            .unwrap_or(0)
    } else {
        app.flat_lines.len() as isize
    };
    if max_lines <= 0 {
        return;
    }
    let viewport = app.right_viewport as isize;
    let mut new = app.right_scroll as isize + delta;
    let max_top = (max_lines - viewport).max(0);
    if new < 0 {
        new = 0;
    }
    if new > max_top {
        new = max_top;
    }
    app.right_scroll = new as usize;
}

fn grade_note(app: &mut App, grade: &str) -> Result<()> {
    if let Some(note) = current_note_mut(app) {
        let mut ex = note.exam.clone().unwrap_or_else(default_exam_state);
        apply_exam_grade(&mut ex, grade, None);
        note.exam = Some(ex);
        note.updated_at = Utc::now().to_rfc3339();
        app.notes.save()?;
    }
    Ok(())
}

// ------------- Flashcards -------------
fn flash_start(app: &mut App) {
    match app.left_panel {
        LeftPanel::Notes => flash_start_notes(app),
        LeftPanel::Questions => flash_start_question(app),
    }
}

fn flash_start_notes(app: &mut App) {
    if let Some(idx) = current_note_index(app) {
        if let Some(n) = app.notes.data.notes.get(idx) {
            let clozes = parse_clozes(&n.content);
            if clozes.is_empty() {
                return;
            }
            let mut cards = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for c in clozes {
                if seen.insert(c.idx.clone()) {
                    cards.push(FlashCardSource::Note {
                        note_idx: idx,
                        cloze: c.idx,
                    });
                }
            }
            app.flash_cards = cards;
            app.flash_pos = 0;
            app.flash_revealed = false;
            app.flash_mode = true;
        }
    }
}

fn flash_start_question(app: &mut App) {
    if let Some(rr) = app.selected_ref() {
        let q = app.get_question(rr);
        if q.answer.is_empty() {
            return;
        }
        let mut cards = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let answers: Vec<String> = q
            .answer
            .iter()
            .filter_map(|ans| {
                let trimmed = ans.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(ans.clone())
                }
            })
            .collect();
        if answers.is_empty() {
            return;
        }
        if answers.len() > 1 {
            let cloze = "multi".to_string();
            if seen.insert(cloze.clone()) {
                cards.push(FlashCardSource::Question {
                    row: rr.clone(),
                    cloze,
                    answers: answers.clone(),
                    is_multi: true,
                });
            }
        } else {
            let cloze = "a1".to_string();
            if seen.insert(cloze.clone()) {
                cards.push(FlashCardSource::Question {
                    row: rr.clone(),
                    cloze,
                    answers: answers.clone(),
                    is_multi: false,
                });
            }
        }
        if cards.is_empty() {
            return;
        }
        app.flash_cards = cards;
        app.flash_pos = 0;
        app.flash_revealed = false;
        app.flash_mode = true;
    }
}

fn flash_reveal(app: &mut App) {
    if app.flash_mode {
        app.flash_revealed = true;
    }
}
fn flash_next(app: &mut App) {
    if app.flash_mode {
        if app.flash_pos + 1 < app.flash_cards.len() {
            app.flash_pos += 1;
            app.flash_revealed = false;
        }
    }
}
fn flash_prev(app: &mut App) {
    if app.flash_mode {
        if app.flash_pos > 0 {
            app.flash_pos -= 1;
            app.flash_revealed = false;
        }
    }
}

#[derive(Debug, Clone)]
enum FlashCardSource {
    Note {
        note_idx: usize,
        cloze: String,
    },
    Question {
        row: RowRef,
        cloze: String,
        answers: Vec<String>,
        is_multi: bool,
    },
}

fn flash_toggle(app: &mut App) {
    if app.flash_mode {
        app.flash_mode = false;
        app.flash_revealed = false;
    } else {
        flash_start(app);
    }
}

fn flash_grade(app: &mut App, data_path: &PathBuf, grade: &str) -> Result<()> {
    if !app.flash_mode || app.flash_cards.is_empty() {
        return Ok(());
    }
    let card = app.flash_cards[app.flash_pos].clone();
    match card {
        FlashCardSource::Note { note_idx, cloze } => {
            if let Some(note) = app.notes.data.notes.get_mut(note_idx) {
                let entry = note
                    .exam_by_cloze
                    .entry(cloze.clone())
                    .or_insert_with(default_exam_state);
                apply_exam_grade(entry, grade, None);
                note.updated_at = Utc::now().to_rfc3339();
                app.notes.save()?;
            }
        }
        FlashCardSource::Question { ref row, cloze, .. } => {
            grade_and_schedule(app, data_path, grade)?;
            let exam_date = app.exam_date;
            let q = app.get_question_mut(row);
            let entry = q
                .exam_by_cloze
                .entry(cloze.clone())
                .or_insert_with(default_exam_state);
            apply_exam_grade(entry, grade, exam_date);
        }
    }
    if !app.flash_cards.is_empty() {
        app.flash_pos = (app.flash_pos + 1) % app.flash_cards.len();
    }
    app.flash_revealed = false;
    Ok(())
}

fn set_status_and_save(app: &mut App, data_path: &PathBuf, status: &str) -> Result<()> {
    if let Some(idx) = app.list_state.selected() {
        let rr = app.rows[idx].clone();
        let q = app.get_question_mut(&rr);
        q.user_status = status.into();
        q.last_reviewed = Some(Utc::now().to_rfc3339());
        save_data(data_path, &app.data)?;
    }
    Ok(())
}

fn run_scraper(app: &mut App, data_path: &PathBuf) -> Result<()> {
    let scraper = Path::new("../backend/scraper.py");
    let status = Command::new("python3")
        .arg(scraper)
        .status()
        .with_context(|| format!("执行 scraper 失败: {}", scraper.display()))?;
    if !status.success() {
        return Err(anyhow::anyhow!("scraper 返回非 0 退出码"));
    }
    let d = load_data(data_path)?;
    app.data = d;
    app.rebuild_rows();
    Ok(())
}

fn ui(f: &mut Frame, app: &mut App) {
    if app.flash_mode {
        draw_flashcard_fullscreen(f, app);
        return;
    }
    // 顶栏 + 主区 + 底栏
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(1),
        ])
        .split(f.area());

    // 主区再水平分栏
    let h = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(app.left_width),
            Constraint::Percentage(100 - app.left_width),
        ])
        .split(v[1]);

    draw_header(f, v[0], app);
    draw_left_panel(f, h[0], app);
    draw_detail(f, h[1], app);
    draw_footer(f, v[2], app);
    // 编辑器弹窗
    if let Some(ed) = app.editor.as_ref() {
        let area = centered_rect(70, 60, f.area());
        f.render_widget(Clear, area);
        let block = Block::default()
            .title(Span::styled(
                " 新建笔记  [Ctrl+S 保存 / Esc 取消 | ←/→ 光标] ",
                Style::default().fg(app.theme.accent),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(app.theme.muted));
        // 画出编辑器光标
        let chars: Vec<char> = ed.buffer.chars().collect();
        let a = ed.cursor.min(chars.len());
        let left: String = chars[0..a].iter().collect();
        let right: String = chars[a..].iter().collect();
        let composed = vec![Line::from(vec![
            Span::raw(left),
            Span::styled("▏", Style::default().fg(app.theme.accent)),
            Span::raw(right),
        ])];
        let para = Paragraph::new(composed)
            .block(block)
            .wrap(Wrap { trim: false });
        f.render_widget(para, area);
    }
}

fn draw_flashcard_fullscreen(f: &mut Frame, app: &mut App) {
    let th = app.theme;
    let area = f.area();
    let block = Block::default()
        .title(Span::styled(" Flashcards ", Style::default().fg(th.accent)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(th.muted));
    f.render_widget(block, area);
    if app.flash_cards.is_empty() {
        return;
    }
    let card = &app.flash_cards[app.flash_pos];
    let inner = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };
    let (notes, single, multi) = flashcard_counts(app);
    let stats_line = Line::from(vec![
        Span::styled(format!("[New:{}] ", notes), Style::default().fg(th.info)),
        Span::styled(
            format!("[Learning:{}] ", single),
            Style::default().fg(th.good),
        ),
        Span::styled(format!("[Review:{}]", multi), Style::default().fg(th.warn)),
    ]);
    let body_lines = match card {
        FlashCardSource::Note { note_idx, cloze } => {
            if let Some(n) = app.notes.data.notes.get(*note_idx) {
                let masked = mask_cloze(&n.content, cloze, app.flash_revealed);
                let header = format!(
                    "{} · {} ({}/{})",
                    note_display_title(n),
                    cloze,
                    app.flash_pos + 1,
                    app.flash_cards.len(),
                );
                vec![
                    Line::from(Span::styled(header, Style::default().fg(th.fg))),
                    Line::from(Span::raw(" ")),
                    Line::from(Span::raw(masked)),
                ]
            } else {
                vec![Line::from(Span::styled(
                    format!(
                        "笔记已失效 ({}/{})",
                        app.flash_pos + 1,
                        app.flash_cards.len()
                    ),
                    Style::default().fg(th.muted),
                ))]
            }
        }
        FlashCardSource::Question {
            row,
            cloze,
            answers,
            is_multi,
        } => {
            let q = app.get_question(row);
            let prompt = if app.flash_revealed {
                format!("{}\n\n答案: {}", q.content, answers.join(" | "))
            } else {
                format!("{}\n\n答案: [···]", q.content)
            };
            let label = if *is_multi {
                "【多选题】".to_string()
            } else {
                format!("{}", cloze)
            };
            let options = format_question_options(q);
            let schedule = format_question_schedule(q);
            let mut lines = vec![
                Line::from(Span::styled(
                    format!(
                        "qid:{} {} · {}/{}",
                        q.id,
                        label,
                        answers.len(),
                        answers.len().max(1)
                    ),
                    Style::default().fg(th.fg),
                )),
                Line::from(Span::styled(schedule, Style::default().fg(th.muted))),
            ];
            if !options.is_empty() {
                lines.push(Line::from(Span::raw(options)));
            }
            lines.push(Line::from(Span::raw(prompt)));
            lines
        }
    };
    let mut all_lines = vec![stats_line];
    all_lines.extend(body_lines);
    let para = Paragraph::new(all_lines)
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(th.fg));
    f.render_widget(para, inner);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    let horiz = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vert[1]);
    horiz[1]
}

fn draw_list(f: &mut Frame, area: Rect, app: &mut App) {
    let th = app.theme;
    let visible_rows: Vec<&RowRef> = app
        .question_filtered_indices
        .iter()
        .filter_map(|&idx| app.rows.get(idx))
        .collect();

    let items: Vec<ListItem> = visible_rows
        .into_iter()
        .map(|rr| {
            let q = app.get_question(rr);
            let id = q.id;
            let src = q.source.clone().unwrap_or_else(|| rr.src.as_str().into());
            let origin = q.origin_name.clone();
            let sub = q.sub_name.clone();
            let status = q.user_status.clone();
            let mut spans = Vec::new();
            let icon = match status.as_str() {
                "mastered" => "✅",
                "reviewing" => "🔄",
                _ => "🆕",
            };
            let src_color = match src.as_str() {
                "simulation" => Color::LightBlue,
                "real" => Color::Magenta,
                _ => Color::Yellow,
            };
            let status_color = match status.as_str() {
                "mastered" => th.good,
                "reviewing" => th.warn,
                _ => th.muted,
            };
            spans.push(Span::styled("› ", Style::default().fg(th.accent)));
            spans.push(Span::raw(icon));
            spans.push(Span::styled(
                format!(" {:>6}  ", id),
                Style::default().fg(th.muted),
            ));
            spans.push(Span::styled(
                format!(" {} ", src),
                Style::default().fg(src_color),
            ));
            spans.push(Span::styled(" | ", Style::default().fg(th.muted)));
            spans.push(Span::styled(origin, Style::default().fg(th.fg)));
            spans.push(Span::raw(" - "));
            spans.push(Span::styled(sub, Style::default().fg(th.muted)));
            spans.push(Span::styled("  ", Style::default()));
            spans.push(Span::styled(status, Style::default().fg(status_color)));
            if q.answer.len() > 1 {
                spans.push(Span::styled("  【多选题】", Style::default().fg(th.warn)));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(Span::styled(
                    " 题目列表 (1/2/3切换来源) ",
                    Style::default().fg(th.accent),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(th.muted)),
        )
        .highlight_style(
            Style::default()
                .bg(th.selection_bg)
                .fg(th.fg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");
    f.render_stateful_widget(list, area, &mut app.list_state);
}

fn draw_left_panel(f: &mut Frame, area: Rect, app: &mut App) {
    match app.left_panel {
        LeftPanel::Questions => draw_list(f, area, app),
        LeftPanel::Notes => draw_notes_list(f, area, app),
    }
}

fn draw_notes_list(f: &mut Frame, area: Rect, app: &mut App) {
    let th = app.theme;
    let mut items: Vec<ListItem> = Vec::new();
    for (pos, &idx) in app.filtered_note_indices.iter().enumerate() {
        if let Some(n) = app.notes.data.notes.get(idx) {
            let depth = app.note_indent_levels.get(pos).copied().unwrap_or(0);
            let indent = "  ".repeat(depth);
            let mut spans = Vec::new();
            let date_label = n.created_at.chars().take(10).collect::<String>();
            spans.push(Span::styled(
                format!("{} ", date_label),
                Style::default().fg(th.muted),
            ));
            spans.push(Span::styled(
                format!("#{} ", n.qid),
                Style::default().fg(th.info),
            ));
            spans.push(Span::raw(indent));
            spans.push(Span::styled(
                note_display_title(n),
                Style::default().fg(th.fg),
            ));
            let excerpt = note_excerpt_head(n);
            if !excerpt.is_empty() {
                spans.push(Span::styled(" · ", Style::default().fg(th.muted)));
                spans.push(Span::styled(excerpt, Style::default().fg(th.muted)));
            }
            items.push(ListItem::new(Line::from(spans)));
        }
    }
    let fold_label = match app.note_fold_mode {
        NotesFoldMode::Full => "全量",
        NotesFoldMode::CurrentParent => "父子聚焦",
    };
    let block = Block::default()
        .title(Span::styled(
            format!(" 笔记列表 ({}) ", fold_label),
            Style::default().fg(th.accent),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(th.muted));
    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(th.selection_bg)
                .fg(th.fg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");
    f.render_stateful_widget(list, area, &mut app.list_state_notes);
}

fn draw_detail(f: &mut Frame, area: Rect, app: &mut App) {
    let th = app.theme;
    let mut lines: Vec<Line> = vec![];
    if matches!(app.left_panel, LeftPanel::Notes) {
        if let Some(n) = current_note(app) {
            lines.push(Line::from(Span::styled(
                format!("{}  ·  qid:{}  ·  {}", n.id, n.qid, note_display_title(n)),
                Style::default().fg(th.accent).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(" "));
            for l in n.content.lines() {
                lines.push(Line::from(Span::raw(l.to_string())));
            }
        } else {
            lines.push(Line::from(Span::styled(
                "无笔记",
                Style::default().fg(th.muted),
            )));
        }
    } else if let Some(rr) = app.selected_ref() {
        let q = app.get_question(rr);
        if !matches!(app.focus, Focus::Text) {
            lines.push(Line::from(Span::styled(
                "题干:",
                Style::default().add_modifier(Modifier::BOLD).fg(th.info),
            )));
            if q.answer.len() > 1 {
                lines.push(Line::from(Span::styled(
                    "【多选题】",
                    Style::default().fg(th.warn),
                )));
            }
            lines.push(Line::from(Span::raw(q.content.clone())));
            lines.push(Line::from(" "));
            if !q.options.is_empty() {
                lines.push(Line::from(Span::styled(
                    "选项:",
                    Style::default().add_modifier(Modifier::BOLD).fg(th.info),
                )));
                for o in &q.options {
                    lines.push(Line::from(Span::raw(format!("{}. {}", o.label, o.content))));
                }
                lines.push(Line::from(" "));
            }
            let show_answer = app.show_answer || app.show_answer_ids.contains(&q.id);
            if show_answer {
                if !q.answer.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "答案:",
                        Style::default().add_modifier(Modifier::BOLD).fg(th.good),
                    )));
                    lines.push(Line::from(Span::raw(format!("{}", q.answer.join(", ")))));
                    lines.push(Line::from(" "));
                }
                if !q.analysis.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "解析:",
                        Style::default().add_modifier(Modifier::BOLD).fg(th.info),
                    )));
                    lines.push(Line::from(Span::raw(q.analysis.clone())));
                    lines.push(Line::from(" "));
                }
            }
            let show_comments = app.show_comments || app.show_comments_ids.contains(&q.id);
            if show_comments && !q.comments.is_empty() {
                lines.push(Line::from(Span::styled(
                    "评论:",
                    Style::default().add_modifier(Modifier::BOLD).fg(th.info),
                )));
                for c in &q.comments {
                    lines.push(Line::from(Span::raw(format!("- {}", c))));
                }
            }
        }
    } else {
        lines.push(Line::from(Span::styled(
            "无结果，请检查筛选条件 (1/2/3)。",
            Style::default().fg(app.theme.muted),
        )));
    }

    // 计算并应用滚动（根据焦点/光标自动调整）
    let viewport = area.height.saturating_sub(2) as usize;
    if viewport != 0 {
        app.right_viewport = viewport;
    }
    if matches!(app.focus, Focus::Text) {
        let inner_width = area.width.saturating_sub(2) as usize;
        let (wrapped_lines, row_counts) = wrap_flat_lines(&app.flat_lines, inner_width);
        app.textarea = TextArea::from(wrapped_lines);
        app.textarea.set_block(
            ratatui::widgets::block::Block::default()
                .title(Span::styled(
                    " 详情（Text Focus）",
                    Style::default().fg(th.accent),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(th.muted)),
        );
        app.textarea.set_cursor_line_style(Style::default());
        app.textarea
            .set_cursor_style(Style::default().bg(app.theme.accent).fg(Color::Black));
        app.textarea
            .set_selection_style(Style::default().bg(app.theme.selection_bg));
        let content_len = apply_textarea_scroll(app, &row_counts, inner_width);
        f.render_widget(&app.textarea, area);
        draw_scrollbar(f, area, app.right_scroll, content_len);
        return;
    } else if matches!(app.left_panel, LeftPanel::Notes) {
        let vp = app.right_viewport.max(1);
        let max_top = lines.len().saturating_sub(vp);
        if app.right_scroll > max_top {
            app.right_scroll = max_top;
        }
    }
    let para = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .title(Span::styled(
                    " 详情 [a]答案 [c]评论 [n/r/m]状态 ",
                    Style::default().fg(th.accent),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(th.muted)),
        )
        .scroll((app.right_scroll as u16, 0));
    f.render_widget(para, area);
    // 绘制滚动条（非 Text Focus 情况）
    if !matches!(app.focus, Focus::Text) {
        let content_len = app.right_scroll + app.right_viewport + 1; // 近似
        draw_scrollbar(f, area, app.right_scroll, content_len);
    }
}

fn apply_textarea_scroll(app: &mut App, row_counts: &[usize], maxw: usize) -> usize {
    let width = maxw.max(1);
    let vp = app.right_viewport.max(1);
    let total_display: usize = row_counts.iter().sum();
    let cursor_line = app.cursor_line.min(row_counts.len().saturating_sub(1));
    let cursor_display_base: usize = row_counts.iter().take(cursor_line).sum();
    let cur_text = app
        .flat_lines
        .get(app.cursor_line)
        .map(|s| s.as_str())
        .unwrap_or("");
    let take_cols = app.cursor_col.min(cur_text.chars().count());
    let mut tmp = String::new();
    tmp.extend(cur_text.chars().take(take_cols));
    let cur_col_w = UnicodeWidthStr::width(tmp.as_str());
    let intra = cur_col_w / width;
    let anchor = app.content_offset + cursor_display_base + intra;
    let mut max_top = app.content_offset + total_display;
    max_top = max_top.saturating_sub(vp);
    let mut new_top = app.right_scroll;
    if anchor < app.right_scroll {
        new_top = anchor;
    } else if anchor > app.right_scroll.saturating_add(vp).saturating_sub(1) {
        new_top = anchor.saturating_sub(vp.saturating_sub(1));
    }
    if new_top > max_top {
        new_top = max_top;
    }
    app.right_scroll = new_top;
    app.content_offset + total_display
}

fn draw_scrollbar(f: &mut Frame, area: Rect, position: usize, content_len: usize) {
    if area.height <= 2 {
        return;
    }
    let total = content_len.max(position + 1).max(1);
    let mut state = ScrollbarState::new(total).position(position);
    let sb = Scrollbar::default();
    let sb_area = Rect {
        x: area.x + area.width.saturating_sub(1),
        y: area.y + 1,
        width: 1,
        height: area.height.saturating_sub(2),
    };
    f.render_stateful_widget(sb, sb_area, &mut state);
}

fn flashcard_counts(app: &App) -> (usize, usize, usize) {
    let mut new = 0usize;
    let mut learning = 0usize;
    let mut review = 0usize;
    for card in &app.flash_cards {
        match card {
            FlashCardSource::Note { note_idx, cloze } => {
                if let Some(note) = app.notes.data.notes.get(*note_idx) {
                    match card_phase(note.exam_by_cloze.get(cloze)) {
                        FlashCardPhase::New => new += 1,
                        FlashCardPhase::Learning => learning += 1,
                        FlashCardPhase::Review => review += 1,
                    }
                } else {
                    new += 1;
                }
            }
            FlashCardSource::Question { row, cloze, .. } => {
                let q = app.get_question(row);
                match card_phase(q.exam_by_cloze.get(cloze)) {
                    FlashCardPhase::New => new += 1,
                    FlashCardPhase::Learning => learning += 1,
                    FlashCardPhase::Review => review += 1,
                }
            }
        }
    }
    (new, learning, review)
}

#[derive(Debug, Clone, Copy)]
enum FlashCardPhase {
    New,
    Learning,
    Review,
}

fn card_phase(exam: Option<&ExamState>) -> FlashCardPhase {
    match exam {
        None => FlashCardPhase::New,
        Some(ex) => {
            if ex.stage == 0 {
                FlashCardPhase::Learning
            } else {
                FlashCardPhase::Review
            }
        }
    }
}

fn format_question_options(q: &Question) -> String {
    if q.options.is_empty() {
        String::new()
    } else {
        q.options
            .iter()
            .map(|o| format!("{}. {}", o.label, o.content))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn format_question_schedule(q: &Question) -> String {
    if let Some(ex) = &q.exam {
        let due = ex.due.as_deref().unwrap_or("-");
        format!("stage:{} priority:{} due:{}", ex.stage, ex.priority, due)
    } else {
        "stage:? priority:? due:?".into()
    }
}

fn wrap_flat_lines(lines: &[String], maxw: usize) -> (Vec<String>, Vec<usize>) {
    let width = maxw.max(1);
    let mut wrapped = Vec::new();
    let mut counts = Vec::with_capacity(lines.len());
    for line in lines {
        let mut rows = 0;
        let mut chunk = String::new();
        let mut chunk_width = 0;
        for ch in line.chars() {
            let w = ch.width().unwrap_or(0);
            if chunk_width + w > width && !chunk.is_empty() {
                wrapped.push(chunk);
                rows += 1;
                chunk = String::new();
                chunk_width = 0;
            }
            chunk.push(ch);
            chunk_width += w;
        }
        if !chunk.is_empty() {
            wrapped.push(chunk);
            rows += 1;
        } else if rows == 0 {
            wrapped.push(String::new());
            rows = 1;
        }
        counts.push(rows);
    }
    (wrapped, counts)
}

fn render_flat_text(lines: &mut Vec<Line>, app: &App) {
    let th = app.theme;
    let n = app.flat_lines.len();
    let sel = match (app.mode, app.sel_start) {
        (Mode::Visual, Some((sl, sc))) => {
            let (el, ec) = (app.cursor_line, app.cursor_col);
            let (sl, sc, el, ec) = if (el, ec) >= (sl, sc) {
                (sl, sc, el, ec)
            } else {
                (el, ec, sl, sc)
            };
            Some((sl, sc, el, ec))
        }
        _ => None,
    };
    for i in 0..n {
        let s = &app.flat_lines[i];
        // 统一在这里渲染：先按选择高亮，再在光标处覆盖纯色块
        let chars: Vec<char> = s.chars().collect();
        let len = chars.len();
        let mut spans: Vec<Span> = Vec::new();
        // 计算当前行的选择范围
        let (sel_start, sel_end) = if let Some((sl, sc, el, ec)) = sel {
            if matches!(app.visual_kind, VisualKind::Line) {
                if i >= sl && i <= el {
                    (Some(0usize), None)
                } else {
                    (None, None)
                }
            } else {
                if sl == el && i == sl {
                    (Some(sc.min(len)), Some(ec.min(len)))
                } else if i == sl && i < el {
                    (Some(sc.min(len)), None)
                } else if i == el && i > sl {
                    (Some(0usize), Some(ec.min(len)))
                } else if i > sl && i < el {
                    (Some(0usize), None)
                } else {
                    (None, None)
                }
            }
        } else {
            (None, None)
        };

        // 基础：未选中全部普通渲染
        let mut idx = 0usize;
        // 未选部分（左）
        if let Some(ss) = sel_start {
            if ss > 0 {
                spans.push(Span::raw(chars[0..ss].iter().collect::<String>()));
            }
            idx = ss;
        }
        // 选中部分
        if let Some(ss) = sel_start {
            let ee = sel_end.unwrap_or(len);
            if ee > ss {
                spans.push(Span::styled(
                    chars[ss..ee].iter().collect::<String>(),
                    Style::default().bg(th.selection_bg),
                ));
                idx = ee;
            }
        }
        // 未选部分（右）
        if idx < len {
            spans.push(Span::raw(chars[idx..].iter().collect::<String>()));
        }

        // 覆盖光标样式
        if i == app.cursor_line {
            if matches!(app.mode, Mode::Visual) {
                let c = app.cursor_col.min(len);
                // 保留选区高亮，同时在光标处插入纯色块
                let mut new_line: Vec<Span> = Vec::new();
                let ss = sel_start;
                let ee = sel_end;
                let build_range = |from: usize, to: usize| -> Vec<Span> {
                    let mut out: Vec<Span> = Vec::new();
                    if from >= to {
                        return out;
                    }
                    if let Some(s) = ss {
                        let e_use = ee.unwrap_or(len);
                        if from < s {
                            out.push(Span::raw(chars[from..s.min(to)].iter().collect::<String>()));
                        }
                        let sel_from = s.max(from);
                        let sel_to = e_use.min(to);
                        if sel_to > sel_from {
                            out.push(Span::styled(
                                chars[sel_from..sel_to].iter().collect::<String>(),
                                Style::default().bg(th.selection_bg),
                            ));
                        }
                        if to > e_use {
                            out.push(Span::raw(
                                chars[e_use.max(from)..to].iter().collect::<String>(),
                            ));
                        }
                    } else {
                        out.push(Span::raw(chars[from..to].iter().collect::<String>()));
                    }
                    out
                };
                // 左侧范围
                new_line.extend(build_range(0, c));
                // 光标块
                new_line.push(Span::styled(
                    "█",
                    Style::default().fg(th.accent).bg(th.accent),
                ));
                // 右侧范围
                new_line.extend(build_range(c, len));
                lines.push(Line::from(new_line));
            } else {
                // Normal 模式：细竖线
                let a = app.cursor_col.min(len);
                let left: String = chars[0..a].iter().collect();
                let right: String = chars[a..].iter().collect();
                lines.push(Line::from(vec![
                    Span::raw(left),
                    Span::styled("▏", Style::default().fg(th.accent)),
                    Span::raw(right),
                ]));
            }
        } else {
            lines.push(Line::from(spans));
        }
    }
}

fn push_split_line(buf: &mut Vec<Line>, s: &str, a: Option<usize>, b: Option<usize>, th: Theme) {
    if let (Some(aa), Some(bb)) = (a, b) {
        let chars: Vec<char> = s.chars().collect();
        let a = aa.min(chars.len());
        let b = bb.min(chars.len());
        let left: String = chars[0..a].iter().collect();
        let mid: String = chars[a..b].iter().collect();
        let right: String = chars[b..].iter().collect();
        buf.push(Line::from(vec![
            Span::raw(left),
            Span::styled(mid, Style::default().bg(th.selection_bg)),
            Span::raw(right),
        ]));
    } else if let (Some(aa), None) = (a, b) {
        let chars: Vec<char> = s.chars().collect();
        let a = aa.min(chars.len());
        let left: String = chars[0..a].iter().collect();
        let right: String = chars[a..].iter().collect();
        buf.push(Line::from(vec![
            Span::raw(left),
            Span::styled(right, Style::default().bg(th.selection_bg)),
        ]));
    } else {
        buf.push(Line::from(Span::raw(s.to_string())));
    }
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let th = app.theme;
    // 背景色条
    let bg = Block::default()
        .borders(Borders::NONE)
        .style(Style::default().bg(th.bar_bg));
    f.render_widget(bg, area);
    // 内容
    let (n, r, m) = app.status_counts();
    let sources = app
        .filter_sources
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let left_label = match app.left_panel {
        LeftPanel::Questions => "Questions",
        LeftPanel::Notes => "Notes",
    };
    let mut segs = vec![
        Span::styled(
            " ErrorTK · Review ",
            Style::default().fg(th.accent).add_modifier(Modifier::BOLD),
        ),
        if matches!(app.mode, Mode::Visual) {
            Span::styled(
                " [VISUAL] ",
                Style::default().fg(th.warn).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::raw("")
        },
        Span::styled(" | pane:", Style::default().fg(th.muted)),
        Span::styled(left_label, Style::default().fg(th.fg)),
        Span::styled(" | src:", Style::default().fg(th.muted)),
        Span::styled(format!("{}", sources), Style::default().fg(th.fg)),
        Span::styled(" | due-only:", Style::default().fg(th.muted)),
        Span::styled(
            format!("{}", if app.due_only { "ON" } else { "OFF" }),
            Style::default().fg(if app.due_only { th.good } else { th.muted }),
        ),
        Span::styled(" | stats:", Style::default().fg(th.muted)),
        Span::styled(
            format!(" new:{} reviewing:{} mastered:{}", n, r, m),
            Style::default().fg(th.fg),
        ),
    ];
    if app.note_search_active {
        let q = app
            .note_search_query
            .as_ref()
            .map(|s| s.as_str())
            .unwrap_or("");
        segs.push(Span::styled("  /", Style::default().fg(th.muted)));
        segs.push(Span::styled(q, Style::default().fg(th.fg)));
        segs.push(Span::styled("_", Style::default().fg(th.accent)));
    }
    if app.question_search_active {
        let q = app
            .question_search_query
            .as_ref()
            .map(|s| s.as_str())
            .unwrap_or("");
        segs.push(Span::styled("  /Q", Style::default().fg(th.muted)));
        segs.push(Span::styled(q, Style::default().fg(th.fg)));
        segs.push(Span::styled("_", Style::default().fg(th.accent)));
    }
    let text = Line::from(segs);
    let para = Paragraph::new(text).style(Style::default().bg(th.bar_bg).fg(th.fg));
    f.render_widget(para, area);
}

fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
    let th = app.theme;
    let bg = Block::default()
        .borders(Borders::NONE)
        .style(Style::default().bg(th.bar_bg));
    f.render_widget(bg, area);
    let mut tips = String::from(" [q]退出  [j/k]上下  [1/2/3]来源  [a/A]答案  [c/C]评论  [z/x/g/v]Again/Hard/Good/Easy  [D]仅到期  [R]重载 ");
    tips.push_str(" | Text: [v/V]Visual/Line  [y]复制  [Ctrl+S]保存笔记 ");
    tips.push_str(" | Questions/Notes: [/]搜索 [o]折叠 [Tab]切换  [S]Scraper ");
    tips.push_str(" | Flash: [F]进入/退出  [Space]揭示  [n/p]切换  [z/x/g/v]评分 ");
    let help = Paragraph::new(Line::from(vec![Span::styled(
        tips,
        Style::default().fg(th.muted),
    )]))
    .style(Style::default().bg(th.bar_bg));
    f.render_widget(help, area);
}

fn render_selectable(lines: &mut Vec<Line>, text: &str, app: &App, block_idx: usize) {
    let th = app.theme;
    // 选择区间（仅在 Visual 模式有效）
    let selected = if let (Mode::Visual, Some((sl, sc))) = (app.mode, app.sel_start) {
        let (el, ec) = (app.cursor_line, app.cursor_col);
        let (sl, sc, el, ec) = if (el, ec) >= (sl, sc) {
            (sl, sc, el, ec)
        } else {
            (el, ec, sl, sc)
        };
        Some((sl, sc, el, ec))
    } else {
        None
    };
    // 简化：每个 block 作为一行（content=0，analysis=1）
    let line_idx = block_idx;
    let push_split = |buf: &mut Vec<Line>, s: &str, a: Option<usize>, b: Option<usize>| {
        if let (Some(aa), Some(bb)) = (a, b) {
            let chars: Vec<char> = s.chars().collect();
            let a = aa.min(chars.len());
            let b = bb.min(chars.len());
            let left: String = chars[0..a].iter().collect();
            let mid: String = chars[a..b].iter().collect();
            let right: String = chars[b..].iter().collect();
            buf.push(Line::from(vec![
                Span::raw(left),
                Span::styled(mid, Style::default().bg(th.selection_bg)),
                Span::raw(right),
            ]));
        } else {
            buf.push(Line::from(Span::raw(s.to_string())));
        }
    };
    if let Some((sl, sc, el, ec)) = selected {
        if sl == el && sl == line_idx {
            if sc == ec {
                // 空选择：显示光标（细竖线）
                let chars: Vec<char> = text.chars().collect();
                let a = sc.min(chars.len());
                let left: String = chars[0..a].iter().collect();
                let right: String = chars[a..].iter().collect();
                lines.push(Line::from(vec![
                    Span::raw(left),
                    Span::styled("▏", Style::default().fg(th.accent)),
                    Span::raw(right),
                ]));
            } else {
                push_split(lines, text, Some(sc), Some(ec));
            }
        } else if sl == line_idx && line_idx < el {
            push_split(lines, text, Some(sc), None);
        } else if el == line_idx && line_idx > sl {
            push_split(lines, text, Some(0), Some(ec));
        } else if line_idx > sl && line_idx < el {
            push_split(lines, text, Some(0), None);
        } else {
            push_split(lines, text, None, None);
        }
    } else {
        push_split(lines, text, None, None);
    }
}

// ---------------- Keymap ----------------
#[derive(Deserialize)]
struct KeyMapToml {
    keys: HashMap<String, String>,
}

fn load_keymap() -> Result<HashMap<char, KeyAction>> {
    // 探测 keymap.toml：当前目录及向上
    let mut paths = vec![PathBuf::from("keymap.toml")];
    if let Ok(cwd) = std::env::current_dir() {
        for anc in cwd.ancestors() {
            paths.push(anc.join("errorTK/tui/keymap.toml"));
        }
    }
    for p in paths {
        if p.exists() {
            let content = fs::read_to_string(&p)
                .with_context(|| format!("读取 keymap 失败: {}", p.display()))?;
            let km: KeyMapToml = toml::from_str(&content).context("解析 keymap.toml 失败")?;
            return Ok(parse_keymap(km.keys));
        }
    }
    Err(anyhow::anyhow!("未找到 keymap.toml"))
}

fn parse_keymap(map: HashMap<String, String>) -> HashMap<char, KeyAction> {
    let mut out = HashMap::new();
    for (k, v) in map {
        if let Some(ch) = k.chars().next() {
            if k.chars().count() == 1 {
                if let Some(act) = action_from_str(&v) {
                    out.insert(ch, act);
                }
            }
        }
    }
    if out.is_empty() {
        out = default_keymap();
    }
    out
}

fn action_from_str(s: &str) -> Option<KeyAction> {
    use KeyAction::*;
    Some(match s {
        "toggle_answer_current" => ToggleAnswerCurrent,
        "toggle_answer_global" => ToggleAnswerGlobal,
        "toggle_comments_current" => ToggleCommentsCurrent,
        "toggle_comments_global" => ToggleCommentsGlobal,
        "toggle_source_sim" => ToggleSourceSim,
        "toggle_source_real" => ToggleSourceReal,
        "toggle_source_famous" => ToggleSourceFamous,
        "mark_new" => MarkNew,
        "mark_reviewing" => MarkReviewing,
        "mark_mastered" => MarkMastered,
        "grade_again" => GradeAgain,
        "grade_hard" => GradeHard,
        "grade_good" => GradeGood,
        "grade_easy" => GradeEasy,
        "toggle_due_only" => ToggleDueOnly,
        "reload" => Reload,
        "visual_toggle" => VisualToggle,
        "visual_line_toggle" => VisualLineToggle,
        "enter_text" => EnterText,
        "exit_text" => ExitText,
        "left" => MoveLeft,
        "right" => MoveRight,
        "up_detail" => MoveUpDetail,
        "down_detail" => MoveDownDetail,
        "yank_to_note" => YankToNote,
        "toggle_notes_fold" => ToggleNotesFold,
        "run_scraper" => RunScraper,
        _ => return None,
    })
}

fn default_keymap() -> HashMap<char, KeyAction> {
    use KeyAction::*;
    let mut m = HashMap::new();
    m.insert('a', ToggleAnswerCurrent);
    m.insert('A', ToggleAnswerGlobal);
    m.insert('c', ToggleCommentsCurrent);
    m.insert('C', ToggleCommentsGlobal);
    m.insert('1', ToggleSourceSim);
    m.insert('2', ToggleSourceReal);
    m.insert('3', ToggleSourceFamous);
    m.insert('n', MarkNew);
    m.insert('r', MarkReviewing);
    m.insert('m', MarkMastered);
    m.insert('z', GradeAgain);
    m.insert('x', GradeHard);
    m.insert('g', GradeGood);
    m.insert('v', GradeEasy);
    m.insert('S', RunScraper); // 大写 S
    m.insert('D', ToggleDueOnly); // 大写 D
    m.insert('R', Reload); // 大写 R
                           // Visual 默认
    m.insert('v', VisualToggle);
    m.insert('h', MoveLeft);
    m.insert('l', MoveRight);
    m.insert('j', MoveDownDetail);
    m.insert('k', MoveUpDetail);
    m.insert('y', YankToNote);
    m.insert('o', ToggleNotesFold);
    m
}
// ---------------- 主题与样式 ----------------
#[derive(Debug, Clone, Copy, ValueEnum)]
enum ThemeKind {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy)]
struct Theme {
    // bg: Color, // 未使用，避免编译警告
    fg: Color,
    muted: Color,
    accent: Color,
    bar_bg: Color,
    selection_bg: Color,
    good: Color,
    warn: Color,
    info: Color,
}

fn theme_of(kind: ThemeKind) -> Theme {
    match kind {
        ThemeKind::Dark => Theme {
            // bg: Color::Rgb(20, 22, 26),
            fg: Color::Rgb(220, 220, 220),
            muted: Color::Rgb(140, 140, 140),
            accent: Color::Rgb(95, 175, 255), // 蓝色系，参考 yazi 风格
            bar_bg: Color::Rgb(35, 40, 46),
            selection_bg: Color::Rgb(60, 65, 72),
            good: Color::Rgb(130, 200, 120),
            warn: Color::Rgb(255, 200, 110),
            info: Color::Rgb(120, 170, 255),
        },
        ThemeKind::Light => Theme {
            // bg: Color::Rgb(250, 250, 250),
            fg: Color::Rgb(30, 30, 30),
            muted: Color::Rgb(120, 120, 120),
            accent: Color::Rgb(0, 122, 255),
            bar_bg: Color::Rgb(235, 240, 245),
            selection_bg: Color::Rgb(210, 220, 235),
            good: Color::Rgb(38, 166, 91),
            warn: Color::Rgb(255, 160, 0),
            info: Color::Rgb(0, 122, 255),
        },
    }
}
// ---------------- 笔记存储 ----------------
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Note {
    id: String,
    qid: i64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    parent_id: Option<String>,
    excerpt: String,
    content: String,
    tags: Vec<String>,
    created_at: String,
    updated_at: String,
    #[serde(default)]
    exam: Option<ExamState>,
    #[serde(default)]
    exam_by_cloze: HashMap<String, ExamState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct NotesFile {
    notes: Vec<Note>,
}

#[derive(Debug)]
struct NotesStore {
    path: PathBuf,
    data: NotesFile,
}

impl NotesStore {
    fn open(path: PathBuf) -> Result<Self> {
        let data = if path.exists() {
            let s = fs::read_to_string(&path)
                .with_context(|| format!("读取笔记失败: {}", path.display()))?;
            serde_json::from_str(&s).unwrap_or_default()
        } else {
            NotesFile::default()
        };
        Ok(Self { path, data })
    }
    fn save(&self) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let s = serde_json::to_string_pretty(&self.data)?;
        fs::write(&self.path, s)
            .with_context(|| format!("写入笔记失败: {}", self.path.display()))?;
        Ok(())
    }
    fn add_note(&mut self, qid: i64, excerpt: String, content: String) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let id = format!("n-{}-{}", qid, Utc::now().timestamp_millis());
        let title = derive_note_title(&excerpt, qid);
        let note = Note {
            id,
            qid,
            title,
            parent_id: None,
            excerpt,
            content,
            tags: vec![],
            created_at: now.clone(),
            updated_at: now,
            exam: None,
            exam_by_cloze: HashMap::new(),
        };
        self.data.notes.push(note);
        self.save()
    }
}

fn derive_note_title(source: &str, qid: i64) -> String {
    source
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .unwrap_or_else(|| format!("Note {}", qid))
}

fn note_display_title(note: &Note) -> String {
    if note.title.trim().is_empty() {
        derive_note_title(&note.excerpt, note.qid)
    } else {
        note.title.trim().to_string()
    }
}

fn note_excerpt_head(note: &Note) -> String {
    note.excerpt
        .lines()
        .next()
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn note_matches_query(note: &Note, query: &str) -> bool {
    let mut haystack = String::new();
    haystack.push_str(&note_display_title(note));
    haystack.push('\n');
    haystack.push_str(&note.excerpt);
    haystack.push('\n');
    haystack.push_str(&note.content);
    haystack.to_lowercase().contains(query)
}

fn refresh_question_filter(app: &mut App) {
    let mut indices: Vec<usize> = (0..app.rows.len()).collect();
    if app.question_search_active {
        let query = app
            .question_search_query
            .as_ref()
            .map(|s| s.to_lowercase())
            .unwrap_or_default();
        if !query.is_empty() {
            indices = app
                .rows
                .iter()
                .enumerate()
                .filter(|(_, rr)| question_matches(app, rr, &query))
                .map(|(i, _)| i)
                .collect();
        }
    }
    if indices.is_empty() {
        app.list_state.select(None);
    } else {
        let sel = app
            .list_state
            .selected()
            .unwrap_or(0)
            .min(indices.len() - 1);
        app.list_state.select(Some(sel));
    }
    app.question_filtered_indices = indices;
}

fn question_matches(app: &App, rr: &RowRef, query: &str) -> bool {
    let q = app.get_question(rr);
    let mut hay = String::new();
    hay.push_str(&q.content);
    hay.push('\n');
    hay.push_str(&q.analysis);
    hay.push('\n');
    hay.push_str(&q.answer.join(" "));
    hay.push('\n');
    for comment in &q.comments {
        hay.push_str(comment);
        hay.push('\n');
    }
    hay.to_lowercase().contains(query)
}

fn question_visible_count(app: &App) -> usize {
    app.question_filtered_indices.len()
}

fn rebuild_note_view(app: &mut App) {
    let prev_indices = app.filtered_note_indices.clone();
    let prev_selected = app
        .list_state_notes
        .selected()
        .and_then(|pos| prev_indices.get(pos).copied());

    let has_query = app
        .note_search_query
        .as_ref()
        .map(|s| !s.is_empty())
        .unwrap_or(false);

    if has_query {
        let query = app
            .note_search_query
            .as_ref()
            .map(|s| s.to_lowercase())
            .unwrap_or_default();
        let mut indices = Vec::new();
        for (idx, note) in app.notes.data.notes.iter().enumerate() {
            if note_matches_query(note, &query) {
                indices.push(idx);
            }
        }
        app.filtered_note_indices = indices;
        app.note_indent_levels = vec![0; app.filtered_note_indices.len()];
    } else {
        let anchor_id = if matches!(app.note_fold_mode, NotesFoldMode::CurrentParent) {
            prev_selected
                .and_then(|idx| app.notes.data.notes.get(idx))
                .map(|note| note.parent_id.clone().unwrap_or_else(|| note.id.clone()))
        } else {
            None
        };
        let (order, indents) = build_note_order(&app.notes.data.notes, anchor_id.as_deref());
        app.filtered_note_indices = order;
        app.note_indent_levels = indents;
    }

    let new_selection = prev_selected.and_then(|idx| {
        app.filtered_note_indices
            .iter()
            .position(|&candidate| candidate == idx)
    });

    if app.filtered_note_indices.is_empty() {
        app.list_state_notes.select(None);
    } else {
        app.list_state_notes
            .select(Some(new_selection.unwrap_or(0)));
    }
}

fn build_note_order(notes: &[Note], anchor: Option<&str>) -> (Vec<usize>, Vec<usize>) {
    let mut id_to_index: HashMap<String, usize> = HashMap::new();
    for (idx, note) in notes.iter().enumerate() {
        id_to_index.insert(note.id.clone(), idx);
    }

    let mut children: HashMap<Option<String>, Vec<usize>> = HashMap::new();
    for (idx, note) in notes.iter().enumerate() {
        let parent = note
            .parent_id
            .as_ref()
            .filter(|pid| id_to_index.contains_key(pid.as_str()))
            .cloned();
        children.entry(parent).or_default().push(idx);
    }

    for vec in children.values_mut() {
        vec.sort_by(|a, b| {
            let a_key = note_display_title(&notes[*a]).to_lowercase();
            let b_key = note_display_title(&notes[*b]).to_lowercase();
            a_key
                .cmp(&b_key)
                .then_with(|| notes[*a].created_at.cmp(&notes[*b].created_at))
        });
    }

    let expand_all = anchor.is_none();
    let expanded_chain = anchor.map(|target| {
        let mut chain = HashSet::new();
        let mut cursor = Some(target.to_string());
        while let Some(id) = cursor.clone() {
            if !chain.insert(id.clone()) {
                break;
            }
            cursor = id_to_index
                .get(&id)
                .and_then(|idx| notes[*idx].parent_id.clone());
        }
        chain
    });

    let mut order = Vec::new();
    let mut depths = Vec::new();
    let mut visited = HashSet::new();
    dfs_notes(
        None,
        0,
        &children,
        notes,
        &mut order,
        &mut depths,
        expand_all,
        expanded_chain.as_ref(),
        &mut visited,
    );
    for idx in 0..notes.len() {
        if visited.contains(&idx) {
            continue;
        }
        visited.insert(idx);
        order.push(idx);
        depths.push(0);
        let id = notes[idx].id.clone();
        let should_expand = expand_all
            || expanded_chain
                .as_ref()
                .map(|set| set.contains(&id))
                .unwrap_or(false);
        if should_expand {
            dfs_notes(
                Some(id),
                1,
                &children,
                notes,
                &mut order,
                &mut depths,
                expand_all,
                expanded_chain.as_ref(),
                &mut visited,
            );
        }
    }
    (order, depths)
}

fn dfs_notes(
    parent: Option<String>,
    depth: usize,
    children: &HashMap<Option<String>, Vec<usize>>,
    notes: &[Note],
    order: &mut Vec<usize>,
    depths: &mut Vec<usize>,
    expand_all: bool,
    expanded_chain: Option<&HashSet<String>>,
    visited: &mut HashSet<usize>,
) {
    if let Some(list) = children.get(&parent) {
        for &idx in list {
            if !visited.insert(idx) {
                continue;
            }
            order.push(idx);
            depths.push(depth);
            let id = notes[idx].id.clone();
            let should_expand =
                expand_all || expanded_chain.map(|set| set.contains(&id)).unwrap_or(false);
            if should_expand {
                dfs_notes(
                    Some(id),
                    depth + 1,
                    children,
                    notes,
                    order,
                    depths,
                    expand_all,
                    expanded_chain,
                    visited,
                );
            }
        }
    }
}

fn current_note_index(app: &App) -> Option<usize> {
    app.list_state_notes
        .selected()
        .and_then(|pos| app.filtered_note_indices.get(pos).copied())
}

fn current_note(app: &App) -> Option<&Note> {
    current_note_index(app).and_then(|idx| app.notes.data.notes.get(idx))
}

fn current_note_mut(app: &mut App) -> Option<&mut Note> {
    let idx = current_note_index(app)?;
    app.notes.data.notes.get_mut(idx)
}

fn note_visible_count(app: &App) -> usize {
    app.filtered_note_indices.len()
}

fn move_note_selection(app: &mut App, delta: isize) {
    let total = note_visible_count(app);
    if total == 0 {
        app.list_state_notes.select(None);
        return;
    }
    let base = app
        .list_state_notes
        .selected()
        .map(|v| v as isize)
        .unwrap_or(-1);
    let mut idx = base + delta;
    if idx < 0 {
        idx = 0;
    }
    if idx >= total as isize {
        idx = total as isize - 1;
    }
    app.list_state_notes.select(Some(idx as usize));
    rebuild_note_view(app);
}

// ------------ Cloze 解析与遮罩 ------------
#[derive(Debug, Clone)]
struct Cloze {
    idx: String,  // 如 "c1"
    text: String, // 被挖空的原文
    hint: Option<String>,
}

fn parse_clozes(content: &str) -> Vec<Cloze> {
    // 兼容 {{c1::text}} 与 {{c1::text::hint}}
    let re = Regex::new(r"\{\{(c\d+)::(.*?)(?:::(.*?))?\}\}").unwrap();
    let mut res = Vec::new();
    for caps in re.captures_iter(content) {
        let idx = caps
            .get(1)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        let txt = caps
            .get(2)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        let hint = caps.get(3).map(|m| m.as_str().to_string());
        res.push(Cloze {
            idx,
            text: txt,
            hint,
        });
    }
    res
}

fn mask_cloze(content: &str, target_idx: &str, revealed: bool) -> String {
    let re = Regex::new(r"\{\{(c\d+)::(.*?)(?:::(.*?))?\}\}").unwrap();
    re.replace_all(content, |caps: &regex::Captures| {
        let idx = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let txt = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        if idx == target_idx {
            if revealed {
                txt.to_string()
            } else {
                "[···]".to_string()
            }
        } else {
            txt.to_string()
        }
    })
    .to_string()
}
