# Code Map

Fly over a codebase as a zoomable GPU treemap. A small demo app built with
[Makepad](https://github.com/makepad/makepad), a Rust UI framework that renders everything on
the GPU. Inspired by Rik Arends' "Makepad Scope" demo.

- Every file is a box sized by its line count, grouped by folder. Zoom in far enough and the
  actual code appears inside the box.
- Respects `.gitignore` (the file list comes from `git ls-files`). Ignored entries show up as
  striped collapsed boxes and are only read when you click them.
- Search: type to highlight matches, Enter flies to the next one, Escape clears.
- Git heatmap: color by "Recently changed" or "Most changed" instead of file type.
- 3D city mode: folders become districts, files become towers with their code on the roof.

## Setup

You need Rust (https://rustup.rs) and a Makepad checkout **next to** this repo, because
`Cargo.toml` points at `../makepad`:

```sh
mkdir makepad-demo && cd makepad-demo
git clone https://github.com/makepad/makepad.git
git -C makepad checkout a4ea2536a4ab223fb31e0282be923191230f44fe   # dev branch, 2026-09-15
git clone https://github.com/Peeter95/code-map.git
```

Makepad's API moves fast, so stick to the pinned commit above. Newer commits may or may not
build.

## Run

```sh
cd code-map
cargo run --release -- /path/to/some/project          # 2D treemap
cargo run --release -- /path/to/some/project --3d     # start in 3D city mode
```

The first build compiles Makepad and takes a few minutes. Later builds are fast. Without a path
it maps the current folder. It works best on a git repository: the ignore rules then match
`git status` exactly and the heatmap has history to show. Outside git it falls back to a simple
ignore list.

Headless check, no window, just scan statistics:

```sh
cargo run --release --bin scan -- /path/to/some/project [ignored/path ...]
```

## Controls

| Input | Action |
| --- | --- |
| Scroll | Zoom |
| Drag | Pan (2D) or orbit (3D) |
| Shift drag or right drag | Pan (3D) |
| Click | Inspect a file or folder |
| Double click | Fly to it |
| Enter in search | Next match |

## Code tour

| File | What it does |
| --- | --- |
| `src/main.rs` | App shell: window, toolbar and inspector written in Makepad's Splash DSL |
| `src/scan.rs` | Reads the project from disk, asks git which files are ignored, summarizes every line |
| `src/model.rs` | Treemap layout and colors |
| `src/history.rs` | Reads `git log` for the heatmap modes |
| `src/map_view.rs` | The custom `CodeMap` widget: drawing, zoom, picking, search |
| `src/map_view/city.rs` | 3D city rendering (offscreen pass, cubes, projected labels) |
| `src/orbit.rs` | Orbit camera for 3D mode |

Tested on macOS. Makepad also targets Windows, Linux and web, but this app has not been tried
there.
