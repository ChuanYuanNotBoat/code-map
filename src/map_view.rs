//! The map widget: draws the tree as nested rectangles and lets you fly
//! around it. Scroll to zoom, drag to pan, click to inspect, double click
//! to fly into something. Click a striped (ignored) box to read it.
//!
//! Everything on screen is a quad drawn by one tiny pixel shader. Makepad
//! batches all quads of the same shader into one GPU draw call, which is why
//! drawing a hundred thousand of them per frame is fine.

use crate::model::{code_inner, frame, Kind, Tree, CHAR_W, COL_CHARS, R};
use crate::scan::{self, Entry};
use makepad_widgets::*;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::mpsc::{channel, Receiver, Sender},
    time::Instant,
};

/// Below this many screen pixels per line, a file is just a tinted box.
const STRIPS_FROM_PX: f64 = 0.6;
/// From this many pixels per line, draw the real text instead of strips.
const TEXT_FROM_PX: f64 = 9.0;
/// Hard cap on quads per frame, a safety net for huge projects.
const QUAD_BUDGET: usize = 400_000;
const LABEL_BUDGET: usize = 500;

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    // The shader for every box, line strip and label pill. `self.pos` is the
    // position inside the quad (0..1) and `self.rect_size` its size in pixels,
    // just like UV coordinates and a resolution uniform in a Three.js shader.
    set_type_default() do #(DrawBlock::script_shader(vm)){
        ..mod.draw.DrawQuad
        color: #x202630
        edge: #x000000
        border: 0.0
        hatch: 0.0
        pixel: fn() {
            let p = self.pos * self.rect_size
            let d = min(min(p.x, p.y), min(self.rect_size.x - p.x, self.rect_size.y - p.y))
            // diagonal stripes mark ignored things
            let stripe = step(0.5, fract((p.x + p.y) / 10.0)) * self.hatch
            let base = vec4(self.color.rgb * (1.0 + 0.6 * stripe), self.color.w)
            let cov = clamp(self.border - d + 0.5, 0.0, 1.0) * step(0.01, self.border)
            let c = mix(base, self.edge, cov)
            return vec4(c.rgb * c.w, c.w)
        }
    }

    mod.widgets.CodeMapBase = #(CodeMap::register_widget(vm))
    mod.widgets.CodeMap = set_type_default() do mod.widgets.CodeMapBase{
        width: Fill
        height: Fill
        draw_bg +: {color: #x07090c}
        draw_block +: {}
        draw_label +: {
            color: #xe6ebf0
            text_style: theme.font_regular{font_size: 8.0}
        }
        draw_code +: {
            color: #xc9d1d9
            text_style: theme.font_code{font_size: 6.0}
        }
    }
}

#[derive(Script, ScriptHook)]
#[repr(C)]
pub struct DrawBlock {
    #[deref]
    draw_super: DrawQuad,
    #[live]
    color: Vec4f,
    #[live]
    edge: Vec4f,
    #[live]
    border: f32,
    #[live]
    hatch: f32,
}

/// What the app shows in the inspector panel.
#[derive(Clone, Debug, Default)]
pub struct NodeInfo {
    pub title: String,
    pub path: String,
    pub details: String,
}

#[derive(Clone, Debug, Default)]
pub enum CodeMapAction {
    #[default]
    None,
    Selected(NodeInfo),
    Status(String),
}

enum ScanMsg {
    Project(Result<scan::ProjectScan, String>, f64),
    Expanded(String, Vec<Entry>),
}

struct Label {
    rect: Rect,
    text: String,
    dim: bool,
}

#[derive(Clone, Copy)]
struct Drag {
    start: DVec2,
    start_off: DVec2,
    moved: bool,
    taps: u32,
}

#[derive(Script, ScriptHook, Widget)]
pub struct CodeMap {
    #[uid]
    uid: WidgetUid,
    #[walk]
    walk: Walk,
    #[layout]
    layout: Layout,
    #[redraw]
    #[area]
    area: Area,
    #[live]
    draw_bg: DrawColor,
    #[live]
    draw_block: DrawBlock,
    #[live]
    draw_label: DrawText,
    #[live]
    draw_code: DrawText,

    #[rust]
    root: PathBuf,
    #[rust]
    tree: Tree,
    #[rust(true)]
    show_ignored: bool,
    #[rust]
    needs_layout: bool,
    #[rust]
    needs_fit: bool,
    /// The map's rectangle on screen, from the last draw.
    #[rust]
    view: Rect,
    /// World size of the whole map.
    #[rust]
    world: R,
    /// Camera: screen = view.pos + (world - cam_off) * cam_scale
    #[rust]
    cam_scale: f64,
    #[rust]
    cam_off: DVec2,
    /// Where a double click is flying the camera to: (world center, scale).
    #[rust]
    flight: Option<(DVec2, f64)>,
    #[rust]
    drag: Option<Drag>,
    #[rust]
    hover: Option<usize>,
    #[rust]
    selected: Option<usize>,
    #[rust]
    tx: Option<Sender<ScanMsg>>,
    #[rust]
    rx: Option<Receiver<ScanMsg>>,
    #[rust]
    pending: usize,
    #[rust]
    frame: NextFrame,
    /// File contents for the fully zoomed-in view, loaded on demand.
    #[rust]
    text_cache: HashMap<usize, Vec<String>>,
    #[rust]
    message: String,
    #[rust]
    labels: Vec<Label>,
}

impl CodeMap {
    pub fn open(&mut self, cx: &mut Cx, root: PathBuf) {
        let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| root.display().to_string());
        self.tree = Tree::new(&name);
        self.root = root.clone();
        self.selected = None;
        self.hover = None;
        self.text_cache.clear();
        let (tx, rx) = channel();
        let worker_tx = tx.clone();
        self.tx = Some(tx);
        self.rx = Some(rx);
        self.pending = 1;
        self.message = format!("Scanning {} ...", root.display());
        cx.widget_action(self.uid, CodeMapAction::Status(self.message.clone()));
        // Scanning runs on a background thread so the window stays responsive.
        // `move` hands ownership of `root` and the sender to that thread.
        std::thread::spawn(move || {
            let started = Instant::now();
            let result = scan::scan_project(&root);
            let _ = worker_tx.send(ScanMsg::Project(result, started.elapsed().as_secs_f64()));
        });
        self.frame = cx.new_next_frame();
        self.redraw(cx);
    }

    pub fn set_show_ignored(&mut self, cx: &mut Cx, show: bool) {
        self.show_ignored = show;
        self.needs_layout = true;
        self.redraw(cx);
    }

    pub fn fit(&mut self, cx: &mut Cx) {
        let world = self.world;
        self.fly_to(cx, world, 1.0);
    }

    fn fly_to(&mut self, cx: &mut Cx, r: R, fill: f64) {
        if self.view.size.x <= 0.0 || r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        let scale = (self.view.size.x / r.w).min(self.view.size.y / r.h) * fill;
        self.flight = Some((dvec2(r.x + r.w * 0.5, r.y + r.h * 0.5), scale));
        self.frame = cx.new_next_frame();
    }

    fn expand(&mut self, cx: &mut Cx, index: usize) {
        let Kind::Ghost { dir, loading: false } = self.tree.nodes[index].kind else { return };
        let Some(tx) = self.tx.clone() else { return };
        self.tree.nodes[index].kind = Kind::Ghost { dir, loading: true };
        let root = self.root.clone();
        let path = self.tree.nodes[index].path.clone();
        self.pending += 1;
        std::thread::spawn(move || {
            let entries = scan::scan_ignored(&root, &path);
            let _ = tx.send(ScanMsg::Expanded(path, entries));
        });
        self.frame = cx.new_next_frame();
        self.redraw(cx);
    }

    /// Pick up finished background scans.
    fn drain(&mut self, cx: &mut Cx) {
        let Some(rx) = &self.rx else { return };
        let msgs: Vec<ScanMsg> = rx.try_iter().collect();
        for msg in msgs {
            self.pending = self.pending.saturating_sub(1);
            match msg {
                ScanMsg::Project(Ok(project), secs) => {
                    let used_git = project.used_git;
                    for entry in project.entries {
                        self.tree.insert(entry);
                    }
                    self.tree.update_weights(true);
                    let root = &self.tree.nodes[0];
                    self.message = format!(
                        "{} files, {} lines, scanned in {:.2}s ({})",
                        fmt_num(root.total_files),
                        fmt_num(root.total_lines),
                        secs,
                        if used_git { "ignore rules from git" } else { "no git: read .gitignore files" }
                    );
                    self.needs_layout = true;
                    self.needs_fit = true;
                }
                ScanMsg::Project(Err(err), _) => {
                    self.message = format!("Could not scan: {err}");
                }
                ScanMsg::Expanded(path, entries) => {
                    if let Some(index) = self.tree.find(&path) {
                        let count = entries.len();
                        self.tree.graft(index, entries);
                        self.needs_layout = true;
                        self.selected = Some(index);
                        self.message = format!("Read ignored {path}: {} files", fmt_num(count as u64));
                    }
                }
            }
            cx.widget_action(self.uid, CodeMapAction::Status(self.message.clone()));
            self.redraw(cx);
        }
    }

    fn relayout(&mut self) {
        self.tree.update_weights(self.show_ignored);
        if self.world.w <= 0.0 {
            // the world is the size of the first view, so zoom 1.0 is "fit"
            self.world = R { x: 0.0, y: 0.0, w: self.view.size.x.max(100.0), h: self.view.size.y.max(100.0) };
        }
        self.tree.layout(self.world);
        self.needs_layout = false;
    }

    fn to_screen(&self, r: R) -> Rect {
        Rect {
            pos: self.view.pos + (dvec2(r.x, r.y) - self.cam_off) * self.cam_scale,
            size: dvec2(r.w, r.h) * self.cam_scale,
        }
    }

    fn to_world(&self, abs: DVec2) -> DVec2 {
        self.cam_off + (abs - self.view.pos) / self.cam_scale
    }

    fn hit(&self, abs: DVec2) -> Option<usize> {
        if self.tree.is_empty() || self.cam_scale <= 0.0 {
            return None;
        }
        let p = self.to_world(abs);
        self.tree.hit(p.x, p.y, 3.0 / self.cam_scale)
    }

    fn step_flight(&mut self, cx: &mut Cx) {
        let Some((center, scale)) = self.flight else { return };
        let half = self.view.size * 0.5;
        let now_center = self.cam_off + half / self.cam_scale;
        let k = 0.2;
        // interpolate zoom in log space so zooming in and out feel the same
        let new_scale = (self.cam_scale.ln() + (scale.ln() - self.cam_scale.ln()) * k).exp();
        let new_center = now_center + (center - now_center) * k;
        self.cam_scale = new_scale;
        self.cam_off = new_center - half / new_scale;
        if (new_scale / scale - 1.0).abs() < 0.002 && (new_center - center).length() * new_scale < 0.5 {
            self.cam_scale = scale;
            self.cam_off = center - half / scale;
            self.flight = None;
        } else {
            self.frame = cx.new_next_frame();
        }
        self.redraw(cx);
    }

    fn info(&self, index: usize) -> NodeInfo {
        let n = &self.tree.nodes[index];
        let title = n.name.clone();
        let mut details = match n.kind {
            Kind::Dir => format!(
                "Folder\n{} files\n{} lines\n{}\n{} direct children",
                fmt_num(n.total_files),
                fmt_num(n.total_lines),
                fmt_bytes(n.bytes),
                n.children.len()
            ),
            Kind::Text => {
                let comments = n.lines.iter().filter(|l| l.comment).count();
                format!(
                    "Text file\n{} lines ({} comment lines)\n{}",
                    fmt_num(n.lines.len() as u64),
                    fmt_num(comments as u64),
                    fmt_bytes(n.bytes)
                )
            }
            Kind::Binary => format!("Binary or very large file\n{}", fmt_bytes(n.bytes)),
            Kind::Ghost { dir, loading } => format!(
                "Ignored {}\n{}",
                if dir { "folder" } else { "file" },
                if loading { "Reading it now ..." } else { "Not read yet. Click it to read it." }
            ),
        };
        if n.ignored && !matches!(n.kind, Kind::Ghost { .. }) {
            details.push_str("\n\nMatched by .gitignore");
        }
        NodeInfo { title, path: if n.path.is_empty() { self.root.display().to_string() } else { n.path.clone() }, details }
    }

    fn draw_map(&mut self, cx: &mut Cx2d) {
        let view = self.view;
        let mut quads = 0usize;
        let mut stack = vec![0usize];
        while let Some(index) = stack.pop() {
            let node = &self.tree.nodes[index];
            if node.weight <= 0.0 {
                continue;
            }
            let r = self.to_screen(node.rect);
            if r.pos.x > view.pos.x + view.size.x
                || r.pos.y > view.pos.y + view.size.y
                || r.pos.x + r.size.x < view.pos.x
                || r.pos.y + r.size.y < view.pos.y
                || (r.size.x < 0.5 && r.size.y < 0.5)
            {
                continue;
            }
            if quads > QUAD_BUDGET {
                break;
            }
            let small = r.size.x.min(r.size.y);
            let border = if small > 8.0 { 1.0 } else { 0.0 };
            match node.kind {
                Kind::Dir => {
                    let shade = 0.07 + 0.03 * (node.depth.min(5) as f32);
                    let color = if node.ignored { vec4(shade * 1.3, shade, shade * 0.8, 1.0) } else { vec4(shade * 0.85, shade, shade * 1.25, 1.0) };
                    block(&mut self.draw_block, cx, r, color, vec4(0.0, 0.0, 0.0, 0.6), border, if node.ignored { 0.4 } else { 0.0 });
                    quads += 1;
                    if small > 4.0 {
                        stack.extend(node.children.iter().rev());
                        let (_, head) = frame(node.rect);
                        if head * self.cam_scale >= 13.0 && r.size.x > 50.0 {
                            self.labels.push(Label { rect: r, text: node.name.clone(), dim: node.ignored });
                        }
                    }
                }
                Kind::Text => {
                    let hue = ext_hue(&node.name);
                    let bg = hsv(hue, 0.35, if node.ignored { 0.10 } else { 0.13 });
                    block(&mut self.draw_block, cx, r, bg, hsv(hue, 0.4, 0.28), border, if node.ignored { 0.3 } else { 0.0 });
                    quads += 1;
                    quads += self.draw_code_lines(cx, index, hue);
                }
                Kind::Binary => {
                    block(&mut self.draw_block, cx, r, vec4(0.16, 0.16, 0.17, 1.0), vec4(0.3, 0.3, 0.3, 1.0), border, 0.5);
                    quads += 1;
                    if r.size.x > 60.0 && r.size.y > 18.0 {
                        self.labels.push(Label { rect: r, text: node.name.clone(), dim: true });
                    }
                }
                Kind::Ghost { loading, .. } => {
                    let color = if loading { vec4(0.20, 0.17, 0.08, 1.0) } else { vec4(0.10, 0.09, 0.08, 1.0) };
                    block(&mut self.draw_block, cx, r, color, vec4(0.45, 0.38, 0.25, 1.0), border, 1.0);
                    quads += 1;
                    if r.size.x > 60.0 && r.size.y > 18.0 {
                        let text = if loading { format!("{} (reading...)", node.name) } else { format!("{} (ignored)", node.name) };
                        self.labels.push(Label { rect: r, text, dim: true });
                    }
                }
            }
        }
    }

    /// Returns how many quads were drawn.
    fn draw_code_lines(&mut self, cx: &mut Cx2d, index: usize, hue: f32) -> usize {
        let node = &self.tree.nodes[index];
        let line_px = node.line_h * self.cam_scale;
        let r_screen = self.to_screen(node.rect);
        if line_px < STRIPS_FROM_PX || node.lines.is_empty() {
            if r_screen.size.x > 60.0 && r_screen.size.y > 18.0 && self.labels.len() < LABEL_BUDGET {
                self.labels.push(Label { rect: r_screen, text: node.name.clone(), dim: false });
            }
            return 0;
        }
        let inner = self.to_screen(code_inner(node.rect));
        let cols = node.cols.max(1) as usize;
        let rows = node.lines.len().div_ceil(cols);
        let char_px = line_px * CHAR_W;
        let col_px = COL_CHARS * char_px;
        let view = self.view;
        let text_mode = line_px >= TEXT_FROM_PX;
        if text_mode && !self.text_cache.contains_key(&index) {
            if self.text_cache.len() > 300 {
                self.text_cache.clear();
            }
            let content = std::fs::read(self.root.join(&node.path)).unwrap_or_default();
            let text = String::from_utf8_lossy(&content).into_owned();
            self.text_cache.insert(index, text.lines().map(|l| l.replace('\t', "    ")).collect());
        }
        let node = &self.tree.nodes[index];
        let mut drawn = 0;
        // only visit the columns and rows that are actually on screen
        let first_col = (((view.pos.x - inner.pos.x) / col_px).floor().max(0.0)) as usize;
        let last_col = ((((view.pos.x + view.size.x) - inner.pos.x) / col_px).ceil().max(0.0) as usize).min(cols);
        let first_row = (((view.pos.y - inner.pos.y) / line_px).floor().max(0.0)) as usize;
        let last_row = ((((view.pos.y + view.size.y) - inner.pos.y) / line_px).ceil().max(0.0) as usize).min(rows);
        if text_mode {
            self.draw_code.text_style.font_size = (line_px * 0.6) as f32;
        }
        for col in first_col..last_col {
            for row in first_row..last_row {
                let k = col * rows + row;
                let Some(line) = node.lines.get(k) else { break };
                let x = inner.pos.x + col as f64 * col_px;
                let y = inner.pos.y + row as f64 * line_px;
                if text_mode {
                    if let Some(text) = self.text_cache.get(&index).and_then(|t| t.get(k)) {
                        let clipped: String = text.chars().take(COL_CHARS as usize).collect();
                        self.draw_code.color = if line.comment { vec4(0.45, 0.55, 0.45, 1.0) } else { hsv(hue, 0.2, 0.85) };
                        self.draw_code.draw_abs(cx, dvec2(x, y), &clipped);
                    }
                } else if line.len > 0 {
                    let color = if line.comment { vec4(0.30, 0.38, 0.30, 1.0) } else { hsv(hue, 0.45, 0.62) };
                    let w = (line.len as f64 * char_px).min(col_px - line.indent as f64 * char_px).max(0.5);
                    let rect = Rect {
                        pos: dvec2(x + line.indent as f64 * char_px, y + line_px * 0.18),
                        size: dvec2(w, (line_px * 0.64).max(0.5)),
                    };
                    block(&mut self.draw_block, cx, rect, color, vec4(0.0, 0.0, 0.0, 0.0), 0.0, 0.0);
                    drawn += 1;
                }
            }
        }
        if r_screen.size.x > 60.0 && r_screen.size.y > 18.0 && self.labels.len() < LABEL_BUDGET {
            self.labels.push(Label { rect: r_screen, text: node.name.clone(), dim: false });
        }
        drawn
    }

    fn outline(&mut self, cx: &mut Cx2d, index: usize, color: Vec4f, width: f32) {
        let r = self.to_screen(self.tree.nodes[index].rect);
        block(&mut self.draw_block, cx, r, vec4(0.0, 0.0, 0.0, 0.0), color, width, 0.0);
    }

    fn draw_labels(&mut self, cx: &mut Cx2d) {
        let labels = std::mem::take(&mut self.labels);
        for label in labels.iter().take(LABEL_BUDGET) {
            let pos = dvec2(label.rect.pos.x.max(self.view.pos.x) + 4.0, label.rect.pos.y.max(self.view.pos.y) + 3.0);
            let room = label.rect.pos.x + label.rect.size.x - pos.x - 8.0;
            if room < 20.0 {
                continue;
            }
            let max_chars = (room / 6.0) as usize;
            let text: String = if label.text.chars().count() > max_chars {
                label.text.chars().take(max_chars.saturating_sub(1)).chain(['…']).collect()
            } else {
                label.text.clone()
            };
            let w = (text.chars().count() as f64 * 5.6 + 8.0).min(room + 4.0);
            block(&mut self.draw_block, cx, Rect { pos: pos - dvec2(3.0, 1.0), size: dvec2(w, 14.0) }, vec4(0.0, 0.0, 0.0, 0.72), vec4(0.0, 0.0, 0.0, 0.0), 0.0, 0.0);
            self.draw_label.color = if label.dim { vec4(0.75, 0.68, 0.55, 1.0) } else { vec4(0.92, 0.94, 0.96, 1.0) };
            self.draw_label.draw_abs(cx, pos, &text);
        }
        self.labels = labels;
        self.labels.clear();
    }
}

impl Widget for CodeMap {
    fn draw_walk(&mut self, cx: &mut Cx2d, _scope: &mut Scope, walk: Walk) -> DrawStep {
        // begin_turtle clips everything we draw to the widget's own rectangle
        cx.begin_turtle(walk, self.layout);
        let rect = cx.turtle().rect();
        self.draw_bg.draw_abs(cx, rect);
        self.view = rect;

        if self.tree.is_empty() {
            self.draw_label.color = vec4(0.7, 0.7, 0.7, 1.0);
            let message = if self.message.is_empty() { "No folder opened".to_string() } else { self.message.clone() };
            self.draw_label.draw_abs(cx, rect.pos + dvec2(16.0, 16.0), &message);
            cx.end_turtle_with_area(&mut self.area);
            return DrawStep::done();
        }
        if self.needs_layout {
            self.relayout();
        }
        if self.needs_fit && rect.size.x > 0.0 {
            self.cam_scale = (rect.size.x / self.world.w).min(rect.size.y / self.world.h);
            let half = rect.size * 0.5;
            self.cam_off = dvec2(self.world.w * 0.5, self.world.h * 0.5) - half / self.cam_scale;
            self.needs_fit = false;
        }

        self.labels.clear();
        self.draw_map(cx);
        if let Some(h) = self.hover {
            if h < self.tree.nodes.len() {
                self.outline(cx, h, vec4(1.0, 1.0, 1.0, 0.5), 1.5);
            }
        }
        if let Some(s) = self.selected {
            if s < self.tree.nodes.len() {
                self.outline(cx, s, vec4(0.35, 0.85, 1.0, 1.0), 2.5);
            }
        }
        self.draw_labels(cx);
        cx.end_turtle_with_area(&mut self.area);
        DrawStep::done()
    }

    fn handle_event(&mut self, cx: &mut Cx, event: &Event, _scope: &mut Scope) {
        if self.frame.is_event(event).is_some() {
            self.drain(cx);
            self.step_flight(cx);
            if self.pending > 0 {
                self.frame = cx.new_next_frame();
            }
        }
        match event.hits(cx, self.area) {
            Hit::FingerHoverIn(e) | Hit::FingerHoverOver(e) => {
                let hover = self.hit(e.abs);
                if hover != self.hover {
                    self.hover = hover;
                    self.redraw(cx);
                }
            }
            Hit::FingerHoverOut(_) => {
                if self.hover.take().is_some() {
                    self.redraw(cx);
                }
            }
            Hit::FingerDown(e) => {
                self.flight = None;
                self.drag = Some(Drag { start: e.abs, start_off: self.cam_off, moved: false, taps: e.tap_count });
            }
            Hit::FingerMove(e) => {
                if let Some(mut drag) = self.drag {
                    let delta = e.abs - drag.start;
                    if delta.length() > 4.0 {
                        drag.moved = true;
                    }
                    if drag.moved {
                        cx.set_cursor(MouseCursor::Grabbing);
                        self.cam_off = drag.start_off - delta / self.cam_scale;
                        self.redraw(cx);
                    }
                    self.drag = Some(drag);
                }
            }
            Hit::FingerUp(e) => {
                cx.set_cursor(MouseCursor::Default);
                if let Some(drag) = self.drag.take() {
                    if !drag.moved {
                        if let Some(index) = self.hit(e.abs) {
                            self.selected = Some(index);
                            if matches!(self.tree.nodes[index].kind, Kind::Ghost { .. }) {
                                self.expand(cx, index);
                            } else if drag.taps >= 2 {
                                let r = self.tree.nodes[index].rect;
                                self.fly_to(cx, r, 0.95);
                            }
                            cx.widget_action(self.uid, CodeMapAction::Selected(self.info(index)));
                            self.redraw(cx);
                        }
                    }
                }
            }
            Hit::FingerScroll(e) => {
                if self.cam_scale <= 0.0 {
                    return;
                }
                self.flight = None;
                // zoom around the point under the mouse
                let factor = (-e.scroll.y * 0.01).exp();
                let anchor = self.to_world(e.abs);
                let min_scale = (self.view.size.x / self.world.w).min(self.view.size.y / self.world.h) * 0.5;
                self.cam_scale = (self.cam_scale * factor).clamp(min_scale, min_scale * 200_000.0);
                self.cam_off = anchor - (e.abs - self.view.pos) / self.cam_scale;
                self.hover = self.hit(e.abs);
                self.redraw(cx);
            }
            _ => {}
        }
    }
}

// `CodeMapRef` is generated by `#[derive(Widget)]`: a handle the app uses to
// reach this widget from outside.
impl CodeMapRef {
    pub fn open(&self, cx: &mut Cx, root: PathBuf) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.open(cx, root);
        }
    }
    pub fn set_show_ignored(&self, cx: &mut Cx, show: bool) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.set_show_ignored(cx, show);
        }
    }
    pub fn fit(&self, cx: &mut Cx) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.fit(cx);
        }
    }
}

/// A stable color per file extension, with hand-picked ones for common types.
fn ext_hue(name: &str) -> f32 {
    let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    match ext {
        "rs" => 0.07,
        "clj" | "cljc" | "edn" => 0.33,
        "cljs" => 0.45,
        "js" | "mjs" | "jsx" => 0.14,
        "ts" | "tsx" => 0.58,
        "css" | "scss" => 0.78,
        "html" => 0.02,
        "json" | "toml" | "yaml" | "yml" => 0.5,
        "md" | "txt" => 0.62,
        _ => {
            let h = ext.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
            (h % 1000) as f32 / 1000.0
        }
    }
}

fn hsv(h: f32, s: f32, v: f32) -> Vec4f {
    let f = |n: f32| {
        let k = (n + h * 6.0) % 6.0;
        v - v * s * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    vec4(f(5.0), f(3.0), f(1.0), 1.0)
}

pub fn fmt_num(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub fn fmt_bytes(b: u64) -> String {
    match b {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.1} KB", b as f64 / 1024.0),
        b => format!("{b} bytes"),
    }
}

fn block(draw: &mut DrawBlock, cx: &mut Cx2d, rect: Rect, color: Vec4f, edge: Vec4f, border: f32, hatch: f32) {
    draw.color = color;
    draw.edge = edge;
    draw.border = border;
    draw.hatch = hatch;
    draw.draw_abs(cx, rect);
}
