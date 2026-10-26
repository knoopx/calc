use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::engine::{self as engine, Engine, Kind, LineResult, complete};
use eframe::egui::text::{CCursor, CCursorRange};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame, Key,
    Margin, Modifiers, Order, Pos2, Rect, Sense, TextEdit, Vec2, ViewportCommand, pos2, vec2,
};

use crate::calc::Calculator;
use crate::highlight;
use crate::palette::{self, Palette, token};
use crate::rates::RatesFetcher;

const FONT_SIZE: f32 = 16.0;
const SCROLLBAR_ROOM: f32 = 12.0;
const EDGE: f32 = 24.0;
const COPIED_FOR: Duration = Duration::from_millis(900);
const HINT_IDLE: Duration = Duration::from_millis(600);
const CLOCK_REFRESH: Duration = Duration::from_secs(60);
const NOTICE_FOR: Duration = Duration::from_millis(1_400);

#[derive(Clone)]
struct Popup {
    start: usize,
    end: usize,
    items: Vec<complete::Suggestion>,
    selected: usize,
    anchor: Pos2,
    anchored: bool,
    explicit: bool,
}

#[derive(Clone)]
struct HoverCache {
    text: String,
    cursor: usize,
    hit: Option<engine::Hover>,
}

#[derive(Clone)]
struct RenameSession {
    cursor: usize,
    token_start: usize,
    buffer: String,
    error: Option<String>,
    focus: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shortcut {
    Quit,
    Dismiss,
    CopyAnswers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NameFieldKey {
    Confirm,
    Cancel,
}

pub struct CalcApp {
    engine: Engine,
    calc: Calculator,
    text: String,
    cursor: usize,
    results: Vec<LineResult>,
    results_text: String,
    vars: HashSet<String>,
    popup: Option<Popup>,
    hover: Option<HoverCache>,
    rename: Option<RenameSession>,
    rename_at: Option<usize>,
    selection: Option<(usize, usize)>,
    rename_commit: bool,
    pending_cursor: Option<usize>,
    editor_id: Option<egui::Id>,
    focus_editor: bool,
    copied: Option<(usize, Instant)>,
    notice: Option<(String, Instant)>,
    edited_at: Instant,
    evaluated_at: Instant,
    rates: RatesFetcher,
    rates_version: u64,
    palette: Palette,
}

impl CalcApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_fonts(&cc.egui_ctx);
        palette::apply(&cc.egui_ctx, &Palette::new());
        let rates = RatesFetcher::default();
        let engine = Engine::new(rates.table()).with_clock(engine::monotonic_ms);
        let ctx = cc.egui_ctx.clone();
        let calc = Calculator::new(
            Engine::new(rates.table()).with_clock(engine::monotonic_ms),
            move || ctx.request_repaint(),
        );
        let mut app = Self {
            engine,
            calc,
            text: String::new(),
            cursor: 0,
            results: Vec::new(),
            results_text: String::new(),
            vars: HashSet::new(),
            popup: None,
            hover: None,
            rename: None,
            rename_at: None,
            selection: None,
            rename_commit: false,
            pending_cursor: None,
            editor_id: None,
            focus_editor: true,
            copied: None,
            notice: None,
            edited_at: Instant::now(),
            evaluated_at: Instant::now(),
            rates,
            rates_version: 0,
            palette: Palette::new(),
        };
        app.evaluate();
        app
    }

    fn evaluate(&mut self) {
        self.calc.request(&self.text);
        if self.results_text != self.text {
            self.results = engine::realign(&self.results_text, &self.results, &self.text);
            self.results_text = self.text.clone();
        }
        self.vars = engine::defined_variables(&self.text);
        self.evaluated_at = Instant::now();
    }

    fn take_answers(&mut self) {
        if let Some(done) = self.calc.take() {
            self.results = if done.complete && done.text == self.text {
                done.results
            } else {
                engine::realign_onto(&done.text, &done.results, &self.text, &self.results)
            };
            self.results_text = self.text.clone();
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let chords = ctx.input_mut(|i| take_chords(&mut i.events));
        let mut quit = false;
        let mut dismiss = false;
        for chord in chords.iter().copied() {
            match chord {
                Shortcut::Quit => quit = true,
                Shortcut::Dismiss => dismiss = true,
                Shortcut::CopyAnswers => {}
            }
        }
        if quit {
            ctx.send_viewport_cmd(ViewportCommand::Close);
            return;
        }

        if self.rename.is_some() {
            let key = ctx.input_mut(|i| take_name_field_key(&mut i.events));
            if dismiss || key == Some(NameFieldKey::Cancel) {
                self.rename = None;
                self.focus_editor = true;
            } else if key == Some(NameFieldKey::Confirm) {
                self.rename_commit = true;
            }
            return;
        }

        if self.rename.is_none() && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F2)) {
            self.rename_at = Some(self.cursor);
            self.focus_editor = false;
            self.popup = None;
        }

        if ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, Key::Space)) {
            self.open_completion(true);
        }

        if let Some(popup) = &mut self.popup {
            let keys = ctx.input_mut(take_popup_keys);
            let n = popup.items.len();
            if keys.previous {
                popup.selected = (popup.selected + n - 1) % n;
            }
            if keys.next {
                popup.selected = (popup.selected + 1) % n;
            }
            if dismiss {
                self.popup = None;
            } else if keys.accept {
                let pick = popup.selected;
                self.accept_completion(pick);
            }
        } else if dismiss {
            self.hover = None;
        }

        if took_chord(&chords, Shortcut::CopyAnswers) {
            self.copy_sheet(ctx);
        }
    }

    fn copy_sheet(&mut self, ctx: &egui::Context) {
        let selected = self.selection.is_some_and(|(from, to)| from != to);
        let (first, last) = self.copy_range();
        let lines: Vec<&str> = self.text.split('\n').collect();
        let last = last.min(lines.len());
        if first >= last {
            return;
        }
        let text = lines[first..last].join("\n");
        let answers = &self.results[first.min(self.results.len())..last.min(self.results.len())];
        let out = engine::with_answers(&text, answers);
        if out.trim().is_empty() {
            return;
        }
        ctx.copy_text(out);
        let label = if selected {
            "selection copied"
        } else {
            "sheet copied"
        };
        self.notice = Some((label.to_string(), Instant::now()));
        ctx.request_repaint_after(NOTICE_FOR);
    }

    fn copy_range(&self) -> (usize, usize) {
        match self.selection.filter(|(from, to)| from != to) {
            Some((from, to)) => touched_lines(&self.text, from, to),
            None => (0, self.text.split('\n').count()),
        }
    }

    fn open_completion(&mut self, explicit: bool) {
        let cursor = self.cursor;
        let text = self.text.clone();
        let Some((start, word)) = complete::word_before(&text, cursor) else {
            return;
        };
        if !want_completion(explicit, &word) {
            return;
        }
        let items = complete::suggest(&word, &self.vars);
        if items.is_empty() {
            return;
        }
        let (anchor, anchored, selected) = match &self.popup {
            Some(popup) if popup.end == cursor => (popup.anchor, popup.anchored, popup.selected),
            _ => (Pos2::ZERO, false, 0),
        };
        self.popup = Some(Popup {
            start,
            end: cursor,
            selected: selected.min(items.len() - 1),
            items,
            anchor,
            anchored,
            explicit,
        });
    }

    fn accept_completion(&mut self, pick: usize) {
        let Some(popup) = self.popup.take() else {
            return;
        };
        let Some(item) = popup.items.get(pick).cloned() else {
            return;
        };

        let inserted = if item.is_function() {
            format!("{}(", item.text)
        } else {
            item.text.clone()
        };
        let chars: Vec<char> = self.text.chars().collect();
        let mut text: String = chars[..popup.start].iter().collect();
        text.push_str(&inserted);
        text.extend(&chars[popup.end.min(chars.len())..]);
        self.text = text;
        self.pending_cursor = Some(popup.start + inserted.chars().count());
        self.edited_at = Instant::now();
        self.focus_editor = true;
        self.evaluate();
    }

    fn naming(&self) -> bool {
        self.rename.is_some() || self.rename_at.is_some()
    }

    fn editor(&mut self, ui: &mut egui::Ui) {
        let theme = self.palette.clone();
        let font = code_font(FONT_SIZE);
        let idle = self.edited_at.elapsed();
        let cursor_line = line_at(&self.text, self.cursor);
        let (row_height, answers) = ui.ctx().fonts_mut(|f| {
            let answers: Vec<Option<f32>> = self
                .results
                .iter()
                .enumerate()
                .map(|(line, r)| {
                    if r.hint && !r.stale && r.note.is_some() {
                        let editing = cursor_line == line;
                        return name_hint_visible(editing, idle).then_some(16.0);
                    }
                    let text = match (&r.kind, r.note.as_deref()) {
                        (Kind::Value, _) if !r.display.is_empty() => r.display.clone(),
                        (Kind::Error, Some(note)) if !r.stale && !r.hint => note.to_string(),
                        _ => return None,
                    };
                    let swatch = if r.color.is_some() {
                        palette::swatch_room(&font)
                    } else {
                        0.0
                    };
                    Some(
                        f.layout_no_wrap(text, font.clone(), Color32::WHITE)
                            .size()
                            .x
                            + swatch,
                    )
                })
                .collect();
            (f.row_height(&font), answers)
        });

        if self.rename_at.take().is_some() {
            self.open_rename(self.cursor);
        }

        let editor_width = (ui.available_width() - SCROLLBAR_ROOM).max(120.0);
        let min_rows = (ui.available_height() / row_height).floor().max(1.0) as usize;

        let vars = &self.vars;
        let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
            let mut job = highlight::layout(buf.as_str(), vars, &theme, font.clone());
            job.wrap.max_width = wrap_width;
            let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
            make_room_for_answers(galley, &answers, wrap_width, row_height)
        };

        if self.naming() {
            self.focus_editor = false;
            if let Some(id) = self.editor_id {
                ui.ctx().memory_mut(|mem| mem.surrender_focus(id));
            }
        }

        let output = TextEdit::multiline(&mut self.text)
            .id_salt("editor")
            .font(font.clone())
            .frame(Frame::NONE)
            .desired_width(editor_width)
            .desired_rows(min_rows)
            .event_filter(sheet_event_filter())
            .layouter(&mut layouter)
            .show(ui);
        let response = &output.response.response;
        let changed = response.changed();

        self.editor_id = Some(response.id);
        if self.rename.is_some() {
            self.focus_editor = false;
        } else if std::mem::take(&mut self.focus_editor) {
            response.request_focus();
        }
        if let Some(pos) = self.pending_cursor.take() {
            let mut state = output.state.clone();
            state
                .cursor
                .set_char_range(Some(CCursorRange::one(CCursor::new(pos))));
            state.store(ui.ctx(), response.id);
            response.scroll_to_me(None);
            ui.ctx().request_repaint();
        }

        if changed {
            self.edited_at = Instant::now();
            self.evaluate();
        }

        if let Some(c) = output.cursor_range.map(|r| r.primary.index.0) {
            self.cursor = c;
        }

        self.selection = output.cursor_range.map(|range| {
            let (a, b) = (range.primary.index.0, range.secondary.index.0);
            (a.min(b), a.max(b))
        });

        let cursor = output
            .cursor_range
            .filter(|r| r.primary == r.secondary)
            .map(|r| r.primary.index.0);
        match cursor {
            Some(c) if changed && response.has_focus() => {
                let explicit = self.popup.as_ref().is_some_and(|p| p.explicit);
                let text = self.text.clone();
                let at = output.galley.pos_from_cursor(CCursor::new(c));
                let anchor = output.galley_pos + at.left_bottom().to_vec2() + vec2(0.0, 2.0);
                self.popup = complete::word_before(&text, c).and_then(|(start, word)| {
                    let items = complete::suggest(&word, &self.vars);
                    (!items.is_empty()).then_some(Popup {
                        start,
                        end: c,
                        items,
                        selected: 0,
                        anchor,
                        anchored: true,
                        explicit,
                    })
                });
            }
            Some(c) if self.popup.as_ref().is_some_and(|p| p.end == c) => {}
            _ => self.popup = None,
        }
        if let Some(popup) = &mut self.popup
            && !popup.anchored
            && let Some(c) = cursor
        {
            let at = output.galley.pos_from_cursor(CCursor::new(c));
            popup.anchor = output.galley_pos + at.left_bottom().to_vec2() + vec2(0.0, 2.0);
            popup.anchored = true;
        }

        if response.hovered() {
            self.show_hover(ui, &output.galley, output.galley_pos);
        }
        self.show_rename_field(ui, &output.galley, output.galley_pos);
        if self.rename_commit {
            self.rename_commit = false;
            self.commit_rename(ui.ctx());
        }

        let column_right = ui.max_rect().right() - SCROLLBAR_ROOM;
        let origin = output.galley_pos;
        let mut line = 0;
        let rows = &output.galley.rows;
        for (i, row) in rows.iter().enumerate() {
            let last_of_line = row.ends_with_newline || i + 1 == rows.len();
            if !last_of_line {
                continue;
            }
            if let Some(Some(width)) = answers.get(line) {
                let dropped = answer_drops(row.rect().max.x, *width, editor_width);
                let content_h = row.rect().height().max(row_height);
                let (y, h) = if dropped {
                    (origin.y + row.pos.y + content_h, row_height)
                } else {
                    (origin.y + row.pos.y, content_h)
                };
                self.paint_result(ui, line, column_right, y, h, &font);
            }
            line += 1;
        }
    }

    fn show_notice(&self, ctx: &egui::Context) {
        let Some((label, at)) = self.notice.clone() else {
            return;
        };
        let left = NOTICE_FOR.saturating_sub(at.elapsed());
        if left.is_zero() {
            return;
        }
        ctx.request_repaint_after(left);
        let theme = self.palette.clone();
        egui::Area::new(egui::Id::unique("notice"))
            .order(Order::Foreground)
            .anchor(Align2::RIGHT_BOTTOM, vec2(-EDGE, -EDGE))
            .show(ctx, |ui| {
                palette::notice(&theme).show(ui, |ui| {
                    ui.label(egui::RichText::new(label).size(12.5).color(theme.dim));
                });
            });
    }

    fn paint_result(
        &mut self,
        ui: &egui::Ui,
        line: usize,
        right: f32,
        y: f32,
        h: f32,
        font: &FontId,
    ) {
        let Some(result) = self.results.get(line) else {
            return;
        };
        let theme = &self.palette;
        if let (Kind::Error, Some(note), false) =
            (&result.kind, result.note.as_deref(), result.stale)
        {
            if result.hint {
                let editing = line_at(&self.text, self.cursor) == line;
                if !name_hint_visible(editing, self.edited_at.elapsed()) {
                    if let Some(left) = HINT_IDLE.checked_sub(self.edited_at.elapsed()) {
                        ui.ctx().request_repaint_after(left);
                    }
                    return;
                }
                let center = pos2(right - 8.0, y + h / 2.0);
                ui.painter().circle_filled(center, 3.0, theme.comment);
                let rect = Rect::from_center_size(center, vec2(16.0, h));
                let resp = ui.interact(
                    rect,
                    ui.scope_id().with(("name-hint", line)),
                    Sense::hover(),
                );
                resp.on_hover_text(note);
                return;
            }
            let galley = ui
                .ctx()
                .fonts_mut(|f| f.layout_no_wrap(note.to_string(), font.clone(), theme.comment));
            let size = galley.size();
            let x = right - size.x;
            ui.painter()
                .galley(pos2(x, y + (h - size.y) / 2.0), galley, theme.comment);
            return;
        }
        if result.kind != Kind::Value || result.display.is_empty() {
            return;
        }
        let copied = self
            .copied
            .is_some_and(|(l, t)| l == line && t.elapsed() < COPIED_FOR);
        let text = if copied {
            "copied".to_string()
        } else {
            result.display.clone()
        };
        let color = if copied || result.stale {
            theme.dim
        } else {
            theme.accent
        };
        let galley = ui
            .ctx()
            .fonts_mut(|f| f.layout_no_wrap(text, font.clone(), color));
        let size = galley.size();
        let swatch = result.color.filter(|_| !copied);
        let extra = if swatch.is_some() {
            palette::swatch_room(font)
        } else {
            0.0
        };
        let content_left = right - size.x - extra;
        let rect = Rect::from_min_size(pos2(content_left, y), vec2(size.x + extra, h))
            .expand2(vec2(6.0, 1.0));
        let response = ui.interact(rect, ui.scope_id().with(("result", line)), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(
                rect,
                CornerRadius::same(palette::token::RADIUS_SM),
                theme.hover,
            );
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if response.clicked() {
            let display = result.display.clone();
            ui.ctx().copy_text(display.clone());
            self.copied = Some((line, Instant::now()));
            ui.ctx().request_repaint_after(COPIED_FOR);
        }
        if let Some([r, g, b]) = swatch {
            let side = font.size * 0.8;
            let square =
                Rect::from_min_size(pos2(content_left, y + (h - side) / 2.0), vec2(side, side));
            ui.painter()
                .rect_filled(square, CornerRadius::same(3), Color32::from_rgb(r, g, b));
            ui.painter().rect_stroke(
                square,
                CornerRadius::same(3),
                egui::Stroke::new(1.0, theme.border()),
                egui::StrokeKind::Inside,
            );
        }
        ui.painter().galley(
            pos2(content_left + extra, y + (h - size.y) / 2.0),
            galley,
            color,
        );
    }

    fn show_popup(&mut self, ctx: &egui::Context) {
        let Some(popup) = &self.popup else { return };
        let theme = &self.palette;
        let font = code_font(FONT_SIZE - 2.0);
        let mut clicked = None;
        egui::Area::new(egui::Id::unique("completions"))
            .order(Order::Foreground)
            .fixed_pos(popup.anchor)
            .show(ctx, |ui| {
                palette::popup(theme).show(ui, |ui| {
                    let meta: Vec<String> = popup.items.iter().map(|item| item.meta()).collect();
                    let width = popup
                        .items
                        .iter()
                        .zip(&meta)
                        .map(|(item, label)| item.text.chars().count() + label.chars().count() + 12)
                        .max()
                        .unwrap_or(8) as f32
                        * (FONT_SIZE - 2.0)
                        * 0.62
                        + 16.0;
                    let width = width.clamp(220.0, 520.0);
                    for (i, item) in popup.items.iter().enumerate() {
                        let (rect, resp) =
                            ui.allocate_exact_size(vec2(width, token::HIT), Sense::click());
                        let selected = i == popup.selected;
                        if selected {
                            ui.painter()
                                .rect_filled(rect, palette::radius_row(), theme.selection);
                        } else if resp.hovered() {
                            ui.painter()
                                .rect_filled(rect, palette::radius_row(), theme.hover);
                        }
                        let color = if selected {
                            theme.accent
                        } else {
                            theme.foreground
                        };
                        ui.painter().text(
                            pos2(rect.left() + 8.0, rect.center().y),
                            Align2::LEFT_CENTER,
                            &item.text,
                            font.clone(),
                            color,
                        );
                        ui.painter().text(
                            pos2(rect.right() - 8.0, rect.center().y),
                            Align2::RIGHT_CENTER,
                            &meta[i],
                            ui_font(FONT_SIZE - 4.0),
                            if selected {
                                theme.foreground
                            } else {
                                theme.dim
                            },
                        );
                        if resp.clicked() {
                            clicked = Some(i);
                        }
                    }
                });
            });
        if let Some(i) = clicked {
            self.accept_completion(i);
        }
    }

    fn show_hover(&mut self, ui: &egui::Ui, galley: &egui::Galley, origin: Pos2) {
        if self.rename.is_some() || self.popup.is_some() {
            return;
        }
        if self.edited_at.elapsed() < HINT_IDLE {
            return;
        }
        let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) else {
            return;
        };
        let cc = galley.cursor_from_pos(pointer - origin);
        let Some(hit) = self.lookup_hover(cc.index.0) else {
            return;
        };
        let start = galley.pos_from_cursor(CCursor::new(hit.start));
        let end = galley.pos_from_cursor(CCursor::new(hit.end));
        let rect = Rect::from_min_max(
            origin + start.left_top().to_vec2(),
            origin + end.right_bottom().to_vec2(),
        )
        .expand(3.0);
        if !rect.contains(pointer) {
            return;
        }
        let note = (hit.used_by > 0).then(|| format!("used by {}", hit.used_by));
        paint_hover_tip(ui.ctx(), &self.palette, rect, &hit.value, note.as_deref());
    }

    fn lookup_hover(&mut self, cursor: usize) -> Option<engine::Hover> {
        if let Some(cache) = &self.hover
            && cache.text == self.text
        {
            if let Some(hit) = &cache.hit
                && cursor >= hit.start
                && cursor <= hit.end
            {
                return Some(hit.clone());
            }
            if cache.hit.is_none() && cache.cursor == cursor {
                return None;
            }
        }
        let hit = self.engine.hover_at(&self.text, cursor);
        self.hover = Some(HoverCache {
            text: self.text.clone(),
            cursor,
            hit: hit.clone(),
        });
        hit
    }

    fn open_rename(&mut self, cursor: usize) {
        let Some(binding) = engine::rename::binding_at(&self.text, cursor) else {
            self.rename = None;
            self.focus_editor = true;
            return;
        };
        self.popup = None;
        self.focus_editor = false;
        self.rename = Some(RenameSession {
            cursor,
            token_start: binding.token_start,
            buffer: binding.name,
            error: None,
            focus: true,
        });
    }

    fn show_rename_field(&mut self, ui: &egui::Ui, galley: &egui::Galley, origin: Pos2) {
        let Some(session) = &self.rename else {
            return;
        };
        let start = galley.pos_from_cursor(CCursor::new(session.token_start));
        let pos = origin + start.left_bottom().to_vec2() + vec2(0.0, 4.0);
        let theme = self.palette.clone();
        let mut buffer = session.buffer.clone();
        let error = session.error.clone();
        let focus = session.focus;
        let mut written = None;
        egui::Area::new(egui::Id::unique("rename"))
            .order(Order::Foreground)
            .fixed_pos(pos)
            .interactable(true)
            .show(ui.ctx(), |ui| {
                palette::field(&theme).show(ui, |ui| {
                    let width = (buffer.chars().count().max(8) as f32) * 10.0 + 12.0;
                    let chars = buffer.chars().count();
                    let out = TextEdit::singleline(&mut buffer)
                        .id(egui::Id::unique("rename-field"))
                        .frame(Frame::NONE)
                        .font(code_font(FONT_SIZE))
                        .desired_width(width.clamp(96.0, 280.0))
                        .hint_text("New name")
                        .show(ui);
                    place_name_caret(ui.ctx(), &out, chars, focus);
                    if let Some(err) = &error {
                        ui.label(egui::RichText::new(err).size(12.5).color(theme.danger()));
                    }
                });
                written = Some(buffer);
            });
        if let Some(session) = &mut self.rename {
            if let Some(buffer) = written {
                session.buffer = buffer;
            }
            session.focus = false;
        }
    }

    fn commit_rename(&mut self, ctx: &egui::Context) {
        let Some(session) = self.rename.clone() else {
            return;
        };
        let before = self.text.clone();
        match engine::rename::rename_variable(&self.text, session.cursor, &session.buffer) {
            Ok(done) => {
                self.push_editor_undo(ctx, &before);
                self.text = done.text;
                self.pending_cursor = Some(done.cursor);
                self.rename = None;
                self.focus_editor = true;
                self.edited_at = Instant::now();
                self.evaluate();
            }
            Err(engine::rename::RenameFail::NotAVariable) => {
                self.rename = None;
                self.focus_editor = true;
            }
            Err(err) => {
                if let Some(session) = &mut self.rename {
                    session.error = Some(err.message());
                    session.focus = true;
                }
            }
        }
    }

    fn push_editor_undo(&self, ctx: &egui::Context, old_text: &str) {
        let Some(id) = self.editor_id else {
            return;
        };
        let cursor = self.cursor;
        let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
        let mut undoer = state.undoer();
        undoer.add_undo(&(
            CCursorRange::one(CCursor::new(cursor)),
            old_text.to_string(),
        ));
        state.set_undoer(undoer);
        state.store(ctx, id);
    }
}

impl eframe::App for CalcApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.take_answers();
        self.handle_shortcuts(&ctx);
        let repainted = ctx.clone();
        self.rates
            .refresh_if_stale(move || repainted.request_repaint());
        let version = self.rates.version();
        if version != self.rates_version {
            self.rates_version = version;
            self.evaluate();
        }
        if self.evaluated_at.elapsed() >= CLOCK_REFRESH {
            self.evaluate();
            ctx.request_repaint_after(CLOCK_REFRESH);
        }

        let frame = Frame::NONE
            .fill(self.palette.background)
            .inner_margin(Margin {
                left: EDGE as i8,
                right: (EDGE - SCROLLBAR_ROOM) as i8,
                top: EDGE as i8,
                bottom: EDGE as i8,
            });
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| self.editor(ui));
        });
        self.show_popup(&ctx);
        self.show_notice(&ctx);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.palette.background.to_normalized_gamma_f32()
    }
}

fn sheet_event_filter() -> egui::EventFilter {
    egui::EventFilter {
        tab: true,
        horizontal_arrows: true,
        vertical_arrows: true,

        escape: true,
    }
}

fn took_chord(chords: &[Shortcut], want: Shortcut) -> bool {
    chords.contains(&want)
}

struct PopupKeys {
    accept: bool,
    previous: bool,
    next: bool,
}

fn take_popup_keys(input: &mut egui::InputState) -> PopupKeys {
    let tab = input.consume_key(Modifiers::NONE, Key::Tab);
    let enter = input.consume_key(Modifiers::NONE, Key::Enter);
    PopupKeys {
        accept: tab || enter,
        previous: input.consume_key(Modifiers::NONE, Key::ArrowUp),
        next: input.consume_key(Modifiers::NONE, Key::ArrowDown),
    }
}

fn want_completion(explicit: bool, word: &str) -> bool {
    explicit || word.chars().count() >= 2
}

fn shortcut_for(ctrl: bool, alt: bool, shift: bool, key: Key, repeat: bool) -> Option<Shortcut> {
    if alt {
        return None;
    }
    match (ctrl, shift, key, repeat) {
        (false, false, Key::Escape, false) => Some(Shortcut::Dismiss),
        (true, false, Key::Q | Key::W, false) => Some(Shortcut::Quit),
        (true, true, Key::C, false) => Some(Shortcut::CopyAnswers),
        _ => None,
    }
}

fn owns_key(ctrl: bool, alt: bool, shift: bool, key: Key) -> bool {
    if alt {
        return false;
    }
    match (ctrl, shift, key) {
        (false, false, Key::Escape) => true,
        (true, false, Key::Q | Key::W) => true,

        (true, true, Key::C) => true,
        _ => false,
    }
}

fn take_chords(events: &mut Vec<egui::Event>) -> Vec<Shortcut> {
    let mut swallow_text = false;
    let mut actions = Vec::new();
    for event in events.iter() {
        if let egui::Event::Key {
            key,
            modifiers,
            pressed: true,
            repeat,
            ..
        } = event
            && owns_key(modifiers.ctrl, modifiers.alt, modifiers.shift, *key)
        {
            swallow_text = true;
            if let Some(shortcut) = shortcut_for(
                modifiers.ctrl,
                modifiers.alt,
                modifiers.shift,
                *key,
                *repeat,
            ) {
                actions.push(shortcut);
            }
        }
    }
    events.retain(|event| match event {
        egui::Event::Key {
            key,
            modifiers,
            pressed: true,
            ..
        } if owns_key(modifiers.ctrl, modifiers.alt, modifiers.shift, *key) => false,
        egui::Event::Text(_) if swallow_text => false,
        _ => true,
    });
    actions
}

fn take_name_field_key(events: &mut Vec<egui::Event>) -> Option<NameFieldKey> {
    let mut found = None;
    events.retain(|event| match event {
        egui::Event::Key {
            key: Key::Enter,
            pressed: true,
            modifiers,
            repeat,
            ..
        } if !modifiers.command && !modifiers.alt && !modifiers.shift => {
            if !repeat {
                found = Some(NameFieldKey::Confirm);
            }
            false
        }
        egui::Event::Key {
            key: Key::Escape,
            pressed: true,
            modifiers,
            repeat,
            ..
        } if modifiers.is_none() => {
            if !repeat {
                found = Some(NameFieldKey::Cancel);
            }
            false
        }
        _ => true,
    });
    found
}

fn place_name_caret(
    ctx: &egui::Context,
    out: &egui::text_edit::TextEditOutput,
    chars: usize,
    select_all: bool,
) {
    if select_all || !out.response.has_focus() {
        out.response.request_focus();
    }
    if !select_all {
        return;
    }
    let mut state = out.state.clone();
    state.cursor.set_char_range(Some(CCursorRange::two(
        CCursor::new(0),
        CCursor::new(chars),
    )));
    state.store(ctx, out.response.id);
}

fn name_hint_visible(editing_line: bool, idle_for: Duration) -> bool {
    !editing_line || idle_for >= HINT_IDLE
}

fn answer_drops(row_end: f32, width: f32, wrap_width: f32) -> bool {
    row_end + palette::GAP + width > wrap_width
}

fn make_room_for_answers(
    galley: Arc<egui::Galley>,
    answers: &[Option<f32>],
    wrap_width: f32,
    row_height: f32,
) -> Arc<egui::Galley> {
    let mut drops = Vec::new();
    let mut line = 0;
    for (i, row) in galley.rows.iter().enumerate() {
        if row.ends_with_newline || i + 1 == galley.rows.len() {
            if let Some(Some(width)) = answers.get(line)
                && answer_drops(row.rect().max.x, *width, wrap_width)
            {
                drops.push(i);
            }
            line += 1;
        }
    }
    if drops.is_empty() {
        return galley;
    }
    let mut galley = Arc::unwrap_or_clone(galley);
    let mut shift = 0.0;
    let mut next = drops.iter().peekable();
    for (i, row) in galley.rows.iter_mut().enumerate() {
        row.pos.y += shift;
        if next.peek() == Some(&&i) {
            next.next();
            shift += row_height;
        }
    }
    galley.rect.max.y += shift;
    galley.mesh_bounds.max.y += shift;
    Arc::new(galley)
}

fn hover_tip_rect(token: Rect, text_size: Vec2, bounds: Rect) -> Rect {
    let pad = vec2(12.0, 8.0);
    let size = text_size + pad * 2.0;
    let gap = 6.0;
    let mut y = token.top() - size.y - gap;
    if y < bounds.top() {
        y = token.bottom() + gap;
    }
    if y + size.y > bounds.bottom() {
        y = (bounds.bottom() - size.y).max(bounds.top());
    }
    let mut x = token.left();
    if x + size.x > bounds.right() {
        x = bounds.right() - size.x;
    }
    if x < bounds.left() {
        x = bounds.left();
    }
    Rect::from_min_size(pos2(x, y), size)
}

fn paint_hover_tip(
    ctx: &egui::Context,
    theme: &Palette,
    token: Rect,
    value: &str,
    note: Option<&str>,
) {
    let font = code_font(14.0);
    let small = ui_font(12.5);
    let (galley, note) = ctx.fonts_mut(|fonts| {
        (
            fonts.layout_no_wrap(value.to_string(), font, theme.accent),
            note.map(|note| fonts.layout_no_wrap(note.to_string(), small, theme.dim)),
        )
    });
    let value_size = galley.size();
    let note_size = note.as_ref().map_or(Vec2::ZERO, |g| g.size());
    let size = vec2(
        value_size.x.max(note_size.x),
        value_size.y
            + if note.is_some() {
                4.0 + note_size.y
            } else {
                0.0
            },
    );
    let bounds = ctx.content_rect().shrink(8.0);
    let tip = hover_tip_rect(token, size, bounds);
    let painter = ctx.layer_painter(egui::LayerId::new(
        Order::Tooltip,
        egui::Id::unique("hover"),
    ));
    painter.rect_filled(
        tip.translate(vec2(0.0, 2.0)),
        CornerRadius::same(8),
        Color32::from_black_alpha(90),
    );
    painter.rect_filled(tip, CornerRadius::same(8), theme.surface());
    painter.rect_stroke(
        tip,
        CornerRadius::same(8),
        egui::Stroke::new(1.0, theme.border()),
        egui::StrokeKind::Inside,
    );
    let at = tip.min + vec2(12.0, 8.0);
    painter.galley(at, galley, theme.accent);
    if let Some(note) = note {
        painter.galley(at + vec2(0.0, value_size.y + 4.0), note, theme.dim);
    }
}

fn line_at(text: &str, cursor: usize) -> usize {
    text.chars().take(cursor).filter(|c| *c == '\n').count()
}

fn touched_lines(text: &str, from: usize, to: usize) -> (usize, usize) {
    let mut first = None;
    let mut last = 0;
    let mut at = 0;
    for (line, part) in text.split_inclusive('\n').enumerate() {
        let end = at + part.chars().count();
        if to > at && from < end {
            first = first.or(Some(line));
            last = line + 1;
        }
        at = end;
    }
    (first.unwrap_or(0), last)
}

macro_rules! font {
    ($name:literal) => {
        include_bytes!(concat!(env!("OUT_DIR"), "/", $name, ".deflate"))
    };
}

static UI_FONT: &[u8] = font!("Inter-Regular.ttf");
static CODE_FONT: &[u8] = font!("JetBrainsMonoNerdFont-Regular.ttf");
static CODE_BOLD_FONT: &[u8] = font!("JetBrainsMonoNerdFont-Bold.ttf");
static CURRENCY_FONT: &[u8] = font!("NotoSans-Currency.ttf");
static EMOJI_FONT: &[u8] = font!("NotoEmoji-Regular.ttf");

fn inflate(packed: &[u8]) -> FontData {
    FontData::from_owned(miniz_oxide::inflate::decompress_to_vec(packed).expect("bundled font"))
}

#[cfg(target_os = "linux")]
fn system_font() -> Option<Vec<u8>> {
    let out = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", "monospace"])
        .output()
        .ok()?;
    let path = String::from_utf8(out.stdout).ok()?;
    let path = path.trim();
    if !(path.ends_with(".ttf") || path.ends_with(".otf")) {
        return None;
    }
    std::fs::read(path).ok()
}

#[cfg(not(target_os = "linux"))]
fn system_font() -> Option<Vec<u8>> {
    None
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::empty();
    let mut put = |name: &str, data: FontData| {
        fonts.font_data.insert(name.into(), Arc::new(data));
    };
    put("inter", inflate(UI_FONT));
    put("code", inflate(CODE_FONT));
    put("code-bold", inflate(CODE_BOLD_FONT));
    put("currency", inflate(CURRENCY_FONT));
    put("emoji", inflate(EMOJI_FONT));
    let mut fallback = vec!["currency", "emoji"];
    if let Some(bytes) = system_font() {
        put("system", FontData::from_owned(bytes));
        fallback.push("system");
    }

    let family = |head: &str| {
        let mut names = vec![head.to_string()];
        names.extend(fallback.iter().map(|name| name.to_string()));
        names
    };
    fonts.families.insert(FontFamily::Monospace, family("code"));
    fonts
        .families
        .insert(FontFamily::Proportional, family("inter"));
    fonts
        .families
        .insert(FontFamily::Name("bold".into()), family("code-bold"));
    ctx.set_fonts(fonts);
}

fn code_font(size: f32) -> FontId {
    FontId::monospace(size)
}

fn ui_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn run() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("calc")
            .with_title("Calc")
            .with_inner_size(Vec2::new(980.0, 600.0))
            .with_min_inner_size(Vec2::new(420.0, 220.0)),
        #[cfg(windows)]
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "Calc",
        options,
        Box::new(|cc| Ok(Box::new(CalcApp::new(cc)))),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        Shortcut, code_font, install_fonts, owns_key, sheet_event_filter, shortcut_for,
        take_popup_keys, touched_lines, ui_font, want_completion,
    };
    use eframe::egui;
    use egui::{Event, FontFamily, InputState, Key, Modifiers};

    fn press(key: Key) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            modifiers: Modifiers::NONE,
            repeat: false,
        }
    }

    #[test]
    fn the_sheet_is_jetbrains_mono_nerd_and_the_interface_is_inter() {
        assert_eq!(code_font(16.0).family, FontFamily::Monospace);
        assert_eq!(ui_font(16.0).family, FontFamily::Proportional);

        let ctx = egui::Context::default();
        install_fonts(&ctx);
        let mut loaded = (false, false);
        let mut out = ctx.run_pass(egui::RawInput::default(), |ctx| {
            loaded = ctx.fonts_mut(|fonts| {
                (
                    fonts.has_glyph(&code_font(16.0), 'm'),
                    fonts.has_glyph(&ui_font(16.0), 'm'),
                )
            });
        });
        out.textures_delta.clear();
        let (mono, proportional) = loaded;
        assert!(mono, "the code face did not load");
        assert!(proportional, "the interface face did not load");

        let patched = ctx.fonts_mut(|fonts| fonts.has_glyph(&code_font(16.0), '\u{e0a1}'));
        assert!(patched, "the code face is not the nerd font build");
    }

    #[test]
    fn typing_needs_two_letters_but_ctrl_space_does_not() {
        assert!(!want_completion(false, "m"));
        assert!(want_completion(false, "mi"));
        assert!(want_completion(true, "m"));
    }

    #[test]
    fn both_tab_and_enter_insert_the_selection() {
        for key in [Key::Tab, Key::Enter] {
            let mut input = InputState::default();
            input.events.push(press(key));
            let keys = take_popup_keys(&mut input);
            assert!(keys.accept, "{key:?} should insert the suggestion");
            assert!(
                input.events.is_empty(),
                "{key:?} must not also reach the text"
            );
        }
    }

    #[test]
    fn arrows_move_the_selection_and_nothing_else() {
        let mut input = InputState::default();
        input.events.push(press(Key::ArrowDown));
        let keys = take_popup_keys(&mut input);
        assert!(keys.next);
        assert!(!keys.previous);
        assert!(!keys.accept);
    }

    #[test]
    fn a_bare_ctrl_c_copies_the_selection_while_shift_adds_the_answers() {
        assert_eq!(
            shortcut_for(true, false, true, Key::C, false),
            Some(Shortcut::CopyAnswers)
        );
        assert!(owns_key(true, false, true, Key::C));
        assert!(
            !owns_key(true, false, false, Key::C),
            "Ctrl+C without shift must stay with the text edit"
        );
    }

    #[test]
    fn a_selection_copies_whole_lines_only() {
        assert_eq!(touched_lines("1 m = 100 cm\n2 + 2\n", 13, 14), (1, 2));

        assert_eq!(touched_lines("1 m = 100 cm\n2 + 2\n", 12, 14), (0, 2));
        assert_eq!(touched_lines("a\nbb\nccc", 2, 4), (1, 2));
        assert_eq!(touched_lines("a\nbb\nccc", 2, 6), (1, 3));

        assert_eq!(touched_lines("a\nbb\nccc", 2, 5), (1, 2));
        assert_eq!(touched_lines("a\nb\n", 0, 4), (0, 2));
    }

    #[test]
    fn escape_stays_with_the_sheet_instead_of_losing_focus() {
        assert!(
            sheet_event_filter().matches(&press(Key::Escape)),
            "escape is the sheet's to handle, not focus navigation's"
        );
    }
}
