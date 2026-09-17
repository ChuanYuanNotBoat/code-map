//! The project as a tree, plus the treemap layout that turns it into
//! nested rectangles.
//!
//! Nodes live in one flat `Vec` and point at each other by index. This is
//! the usual Rust way to build trees: no shared pointers to fight the
//! ownership rules with, and a child always has a bigger index than its
//! parent, so walking the Vec backwards visits children before parents.

use crate::history::History;
use crate::scan::{Entry, EntryKind, Line};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct R {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl R {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Dir,
    Text,
    Binary,
    /// Ignored and not read yet. `loading` while a click is scanning it.
    Ghost { dir: bool, loading: bool },
}

#[derive(Debug)]
pub struct Node {
    pub name: String,
    pub path: String,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub kind: Kind,
    pub ignored: bool,
    pub depth: u32,
    pub lines: Vec<Line>,
    pub bytes: u64,
    /// Totals for this node and everything below it.
    pub total_lines: u64,
    pub total_files: u64,
    /// Share of space in the treemap. Zero means "not laid out".
    pub weight: f64,
    /// Position in world space (the camera maps this to the screen).
    pub rect: R,
    /// Code layout inside a file: number of columns and line height.
    pub cols: u32,
    pub line_h: f64,
    /// Git activity: commits touching this file (or folder) and last change time.
    pub commits: u32,
    pub last_change: i64,
    /// 0..1 rank for the heatmap, negative when there is no git data.
    pub heat: f32,
    /// Search: `lit` if this node or a folder above it matches,
    /// `on_path` if it is lit or has a match somewhere below it.
    pub lit: bool,
    pub on_path: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ColorMode {
    #[default]
    FileType,
    Recent,
    Churn,
}

pub struct Tree {
    pub nodes: Vec<Node>,
    by_path: HashMap<String, usize>,
}

impl Node {
    fn new(name: String, path: String, parent: Option<usize>, depth: u32, ignored: bool) -> Self {
        Node {
            name,
            path,
            parent,
            children: Vec::new(),
            kind: Kind::Dir,
            ignored,
            depth,
            lines: Vec::new(),
            bytes: 0,
            total_lines: 0,
            total_files: 0,
            weight: 0.0,
            rect: R::default(),
            cols: 0,
            line_h: 0.0,
            commits: 0,
            last_change: 0,
            heat: -1.0,
            lit: true,
            on_path: true,
        }
    }

    pub fn is_file(&self) -> bool {
        matches!(self.kind, Kind::Text | Kind::Binary)
    }
}

impl Default for Tree {
    fn default() -> Self {
        Self::new("")
    }
}

impl Tree {
    pub fn new(root_name: &str) -> Self {
        let root = Node::new(root_name.to_string(), String::new(), None, 0, false);
        Tree { nodes: vec![root], by_path: HashMap::from([(String::new(), 0)]) }
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }

    pub fn find(&self, path: &str) -> Option<usize> {
        self.by_path.get(path).copied()
    }

    /// Add an entry, creating its parent folders on the way.
    pub fn insert(&mut self, entry: Entry) {
        let mut parent = 0;
        let mut path = String::new();
        let parts: Vec<&str> = entry.path.split('/').collect();
        for (i, part) in parts.iter().enumerate() {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(part);
            let last = i + 1 == parts.len();
            if let Some(&existing) = self.by_path.get(&path) {
                if last {
                    self.fill(existing, entry);
                    return;
                }
                parent = existing;
                continue;
            }
            let depth = self.nodes[parent].depth + 1;
            let index = self.nodes.len();
            // a folder created on the way is ignored only if its parent is;
            // `fill` sets the flag of the entry itself
            let ignored = self.nodes[parent].ignored;
            self.nodes.push(Node::new(part.to_string(), path.clone(), Some(parent), depth, ignored));
            self.nodes[parent].children.push(index);
            self.by_path.insert(path.clone(), index);
            if last {
                self.fill(index, entry);
                return;
            }
            parent = index;
        }
    }

    fn fill(&mut self, index: usize, entry: Entry) {
        let node = &mut self.nodes[index];
        node.ignored = entry.ignored;
        match entry.kind {
            EntryKind::Text { bytes, lines } => {
                node.kind = Kind::Text;
                node.bytes = bytes;
                node.lines = lines;
            }
            EntryKind::Binary { bytes } => {
                node.kind = Kind::Binary;
                node.bytes = bytes;
            }
            EntryKind::Ghost { dir } => {
                node.kind = Kind::Ghost { dir, loading: false };
            }
        }
    }

    /// Turn a ghost into a real folder (or file) using freshly scanned entries.
    pub fn graft(&mut self, index: usize, entries: Vec<Entry>) {
        if let Kind::Ghost { dir, .. } = self.nodes[index].kind {
            self.nodes[index].kind = if dir { Kind::Dir } else { Kind::Binary };
        }
        for entry in entries {
            self.insert(entry);
        }
    }

    /// Recompute totals and treemap weights, bottom up.
    pub fn update_weights(&mut self, show_ignored: bool) {
        for i in (0..self.nodes.len()).rev() {
            let (weight, lines, files, bytes) = match self.nodes[i].kind {
                Kind::Text => {
                    let n = self.nodes[i].lines.len() as u64;
                    (n.max(1) as f64, n, 1, self.nodes[i].bytes)
                }
                // binaries count, but a 50 MB image should not swallow the map
                Kind::Binary => ((self.nodes[i].bytes as f64 / 2048.0).clamp(1.0, 400.0), 0, 1, self.nodes[i].bytes),
                Kind::Ghost { .. } => (0.0, 0, 0, 0),
                Kind::Dir => {
                    let mut weight = 0.0;
                    let (mut lines, mut files, mut bytes) = (0, 0, 0);
                    let mut ghosts = 0;
                    for &c in &self.nodes[i].children {
                        let child = &self.nodes[c];
                        if matches!(child.kind, Kind::Ghost { .. }) {
                            ghosts += 1;
                        } else if show_ignored || !child.ignored {
                            weight += child.weight;
                        }
                        lines += child.total_lines;
                        files += child.total_files;
                        bytes += child.bytes;
                    }
                    // Unread ignored things get a small token size: big enough to
                    // see and click, small enough not to distort the map.
                    let ghost_weight = if show_ignored && ghosts > 0 { (weight * 0.02).max(3.0) } else { 0.0 };
                    for c in self.nodes[i].children.clone() {
                        if matches!(self.nodes[c].kind, Kind::Ghost { .. }) {
                            self.nodes[c].weight = ghost_weight;
                            weight += ghost_weight;
                        }
                    }
                    (weight, lines, files, bytes)
                }
            };
            let node = &mut self.nodes[i];
            node.total_lines = lines;
            node.total_files = files;
            if node.kind == Kind::Dir {
                node.bytes = bytes;
            }
            node.weight = if !show_ignored && node.ignored { 0.0 } else { weight };
        }
    }

    pub fn layout(&mut self, world: R) {
        self.layout_node(0, world);
    }

    fn layout_node(&mut self, index: usize, r: R) {
        self.nodes[index].rect = r;
        match self.nodes[index].kind {
            Kind::Dir => {
                let (pad, head) = frame(r);
                let inner = R { x: r.x + pad, y: r.y + head, w: r.w - 2.0 * pad, h: r.h - head - pad };
                if inner.w <= 0.0 || inner.h <= 0.0 {
                    return;
                }
                let mut kids: Vec<(usize, f64)> = self.nodes[index]
                    .children
                    .iter()
                    .map(|&c| (c, self.nodes[c].weight))
                    .filter(|(_, w)| *w > 0.0)
                    .collect();
                kids.sort_by(|a, b| b.1.total_cmp(&a.1));
                for (child, rect) in squarify(&kids, inner) {
                    self.layout_node(child, rect);
                }
            }
            Kind::Text => {
                let (cols, line_h) = code_columns(r, self.nodes[index].lines.len());
                self.nodes[index].cols = cols;
                self.nodes[index].line_h = line_h;
            }
            _ => {}
        }
    }

    /// Copy git activity onto files and add it up for folders.
    pub fn apply_history(&mut self, history: &History) {
        for node in &mut self.nodes {
            let (commits, last) = history.files.get(&node.path).copied().unwrap_or((0, 0));
            node.commits = commits;
            node.last_change = last;
        }
        for i in (1..self.nodes.len()).rev() {
            if let Some(parent) = self.nodes[i].parent {
                let (commits, last) = (self.nodes[i].commits, self.nodes[i].last_change);
                let p = &mut self.nodes[parent];
                if p.kind == Kind::Dir {
                    p.commits += commits;
                    p.last_change = p.last_change.max(last);
                }
            }
        }
    }

    /// Rank files by the chosen metric. Ranks instead of raw values spread the
    /// colors evenly, so one file with 900 commits does not make everything else blue.
    pub fn compute_heat(&mut self, mode: ColorMode) {
        let mut files: Vec<(usize, i64)> = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.is_file() && n.commits > 0)
            .map(|(i, n)| (i, if mode == ColorMode::Churn { n.commits as i64 } else { n.last_change }))
            .collect();
        for node in &mut self.nodes {
            node.heat = -1.0;
        }
        if mode == ColorMode::FileType || files.is_empty() {
            return;
        }
        files.sort_by_key(|f| f.1);
        let max = (files.len() - 1).max(1) as f32;
        let mut rank = 0;
        for k in 0..files.len() {
            // equal values share a rank
            if k > 0 && files[k].1 != files[k - 1].1 {
                rank = k;
            }
            self.nodes[files[k].0].heat = rank as f32 / max;
        }
    }

    /// Mark matches for a search. Every space separated word must appear in
    /// the path (case-insensitive). Returns matching nodes, biggest first.
    pub fn search(&mut self, query: &str) -> Vec<usize> {
        let words: Vec<String> = query.split_whitespace().map(|w| w.to_lowercase()).collect();
        if words.is_empty() {
            for node in &mut self.nodes {
                node.lit = true;
                node.on_path = true;
            }
            return Vec::new();
        }
        let mut matches = Vec::new();
        for i in 0..self.nodes.len() {
            let path = self.nodes[i].path.to_lowercase();
            let hit = i > 0 && words.iter().all(|w| path.contains(w.as_str()));
            if hit {
                matches.push(i);
            }
            let parent_lit = self.nodes[i].parent.is_some_and(|p| self.nodes[p].lit);
            self.nodes[i].lit = hit || parent_lit;
            self.nodes[i].on_path = self.nodes[i].lit;
        }
        for i in (1..self.nodes.len()).rev() {
            if self.nodes[i].on_path {
                if let Some(p) = self.nodes[i].parent {
                    self.nodes[p].on_path = true;
                }
            }
        }
        // a match inside a matching folder is redundant for "fly to next match"
        matches.retain(|&i| !self.nodes[i].parent.is_some_and(|p| self.nodes[p].lit));
        matches.sort_by(|a, b| self.nodes[*b].weight.total_cmp(&self.nodes[*a].weight));
        matches
    }

    /// The top-level folder a node lives in (for coloring regions in 3D).
    pub fn top_level(&self, mut index: usize) -> usize {
        while let Some(parent) = self.nodes[index].parent {
            if parent == 0 {
                return index;
            }
            index = parent;
        }
        index
    }

    /// The deepest laid-out node under a world point.
    pub fn hit(&self, x: f64, y: f64, min_size: f64) -> Option<usize> {
        if !self.nodes[0].rect.contains(x, y) {
            return None;
        }
        let mut at = 0;
        'down: loop {
            for &c in &self.nodes[at].children {
                let child = &self.nodes[c];
                if child.weight > 0.0 && child.rect.w >= min_size && child.rect.contains(x, y) {
                    at = c;
                    continue 'down;
                }
            }
            return Some(at);
        }
    }
}

/// Width of a folder's side border and height of its title bar, in world units.
pub fn frame(r: R) -> (f64, f64) {
    let side = r.w.min(r.h);
    let pad = side * 0.012;
    let head = (side * 0.045).min(r.h * 0.25).max(pad);
    (pad, head)
}

/// Code inside a file is laid out like a newspaper: columns of lines.
pub const COL_CHARS: f64 = 100.0;
/// Width of one character relative to the line height (monospace).
pub const CHAR_W: f64 = 0.5;

pub fn code_inner(r: R) -> R {
    let pad = r.w.min(r.h) * 0.03;
    R { x: r.x + pad, y: r.y + pad, w: (r.w - 2.0 * pad).max(0.0), h: (r.h - 2.0 * pad).max(0.0) }
}

fn code_columns(r: R, n: usize) -> (u32, f64) {
    let inner = code_inner(r);
    if n == 0 || inner.w <= 0.0 || inner.h <= 0.0 {
        return (1, 0.0);
    }
    let n = n as f64;
    let col_w_per_lh = COL_CHARS * CHAR_W;
    // pick the line height where n lines exactly fill the area, then snap to whole columns
    let lh = (inner.w * inner.h / (col_w_per_lh * n)).sqrt();
    let cols = (inner.w / (col_w_per_lh * lh)).floor().max(1.0);
    let rows = (n / cols).ceil();
    let lh = (inner.h / rows).min(inner.w / (cols * col_w_per_lh));
    (cols as u32, lh)
}

/// Squarified treemap (Bruls, Huizing, van Wijk): fill rows so the
/// rectangles stay as close to squares as possible.
fn squarify(items: &[(usize, f64)], r: R) -> Vec<(usize, R)> {
    let total: f64 = items.iter().map(|i| i.1).sum();
    if total <= 0.0 {
        return Vec::new();
    }
    let scale = r.w * r.h / total;
    let areas: Vec<f64> = items.iter().map(|i| i.1 * scale).collect();
    let mut out = Vec::with_capacity(items.len());
    let mut rect = r;
    let mut i = 0;
    while i < items.len() {
        let side = rect.w.min(rect.h);
        let mut j = i + 1;
        let mut best = worst(&areas[i..j], side);
        while j < items.len() {
            let next = worst(&areas[i..j + 1], side);
            if next > best {
                break;
            }
            best = next;
            j += 1;
        }
        let row: f64 = areas[i..j].iter().sum();
        if rect.w >= rect.h {
            let t = (row / rect.h).min(rect.w);
            let mut y = rect.y;
            for k in i..j {
                let h = areas[k] / t;
                out.push((items[k].0, R { x: rect.x, y, w: t, h }));
                y += h;
            }
            rect = R { x: rect.x + t, w: rect.w - t, ..rect };
        } else {
            let t = (row / rect.w).min(rect.h);
            let mut x = rect.x;
            for k in i..j {
                let w = areas[k] / t;
                out.push((items[k].0, R { x, y: rect.y, w, h: t }));
                x += w;
            }
            rect = R { y: rect.y + t, h: rect.h - t, ..rect };
        }
        i = j;
    }
    out
}

fn worst(row: &[f64], side: f64) -> f64 {
    let sum: f64 = row.iter().sum();
    let max = row.iter().cloned().fold(0.0, f64::max);
    let min = row.iter().cloned().fold(f64::INFINITY, f64::min);
    let s2 = side * side;
    (s2 * max / (sum * sum)).max(sum * sum / (s2 * min))
}
