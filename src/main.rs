//! Code Map: fly over a codebase as a zoomable GPU treemap.
//!
//!   cargo run --release -- /path/to/project [--3d]
//!   cargo run --release --bin scan -- /path/to/project   (no window, just stats)

pub use makepad_widgets;

mod history;
mod map_view;
mod model;
mod orbit;
mod scan;

use makepad_widgets::*;
use map_view::{CodeMapAction, CodeMapWidgetRefExt, DetailLevel};
use model::ColorMode;
use std::path::PathBuf;

app_main!(App);

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    let InfoLabel = Label{
        width: Fill
        draw_text +: {color: #xaab4be text_style +: {font_size: 9.0}}
    }

    startup() do #(App::script_component(vm)){
        ui: Root{
            main_window := Window{
                window.inner_size: vec2(1400, 900)
                pass.clear_color: #x07090c
                body +: {
                    flow: Down
                    toolbar := View{
                        width: Fill height: 40
                        flow: Right spacing: 14
                        padding: Inset{left: 12, right: 12}
                        align: Align{y: 0.5}
                        show_bg: true
                        draw_bg +: {color: #x12161c}
                        title := Label{
                            text: "Code Map"
                            draw_text +: {color: #xffffff text_style +: {font_size: 10.0}}
                        }
                        search := TextInput{
                            width: 220 height: 28
                            empty_text: "Search files and folders"
                        }
                        color_mode := DropDown{
                            width: 150
                            labels: ["File type" "Recently changed" "Most changed"]
                        }
                        three_d := CheckBox{text: "3D" active: false}
                        detail_level := DropDown{
                            width: 110
                            labels: ["Normal" "High" "Ultra"]
                            selected_item: 0
                        }
                        fit_button := Button{text: "Fit"}
                        show_ignored := CheckBox{text: "Show ignored" active: true}
                        status := Label{
                            width: Fill
                            text: ""
                            draw_text +: {color: #x8a949e text_style +: {font_size: 9.0}}
                        }
                    }
                    View{
                        width: Fill height: Fill
                        flow: Right
                        map := CodeMap{}
                        inspector := View{
                            width: 290 height: Fill
                            flow: Down spacing: 8
                            padding: Inset{left: 14, right: 14, top: 14, bottom: 14}
                            show_bg: true
                            draw_bg +: {color: #x12161c}
                            Label{
                                text: "INSPECTOR"
                                draw_text +: {color: #x5a6570 text_style +: {font_size: 8.0}}
                            }
                            info_title := Label{
                                width: Fill
                                text: "Click something on the map"
                                draw_text +: {color: #xffffff text_style +: {font_size: 11.0}}
                            }
                            info_path := InfoLabel{text: ""}
                            info_details := InfoLabel{text: ""}
                            View{width: Fill height: Fill}
                            InfoLabel{
                                text: "Scroll: zoom\nDrag: pan (2D) or orbit (3D)\nShift or right drag: pan (3D)\nClick: inspect\nDouble click: fly to it\nEnter in search: next match\nStriped boxes are ignored by git.\nClick one to read it."
                            }
                        }
                    }
                }
            }
        }
    }
}

#[derive(Script, ScriptHook)]
pub struct App {
    #[live]
    ui: WidgetRef,
}

/// The folder to map: first command line argument, or the current folder.
fn project_path() -> PathBuf {
    let arg = std::env::args().skip(1).find(|a| !a.starts_with("--"));
    let path = arg.map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    path.canonicalize().unwrap_or(path)
}

impl MatchEvent for App {
    fn handle_startup(&mut self, cx: &mut Cx) {
        let path = project_path();
        self.ui.label(cx, ids!(title)).set_text(cx, &format!("Code Map: {}", path.display()));
        let map = self.ui.code_map(cx, ids!(map));
        map.open(cx, path);
        if std::env::args().any(|a| a == "--3d") {
            self.ui.check_box(cx, ids!(three_d)).set_active(cx, true, Animate::No);
            map.set_3d(cx, true);
        }
    }

    fn handle_actions(&mut self, cx: &mut Cx, actions: &Actions) {
        let map = self.ui.code_map(cx, ids!(map));
        if self.ui.button(cx, ids!(fit_button)).clicked(actions) {
            map.fit(cx);
        }
        if let Some(show) = self.ui.check_box(cx, ids!(show_ignored)).changed(actions) {
            map.set_show_ignored(cx, show);
        }
        if let Some(on) = self.ui.check_box(cx, ids!(three_d)).changed(actions) {
            map.set_3d(cx, on);
        }
        if let Some(index) = self.ui.drop_down(cx, ids!(detail_level)).changed(actions) {
            let level = match index {
                1 => DetailLevel::High,
                2 => DetailLevel::Ultra,
                _ => DetailLevel::Normal,
            };
            map.set_detail_level(cx, level);
        }
        if let Some(index) = self.ui.drop_down(cx, ids!(color_mode)).changed(actions) {
            let mode = match index {
                1 => ColorMode::Recent,
                2 => ColorMode::Churn,
                _ => ColorMode::FileType,
            };
            map.set_color_mode(cx, mode);
        }
        let search = self.ui.text_input(cx, ids!(search));
        if let Some(query) = search.changed(actions) {
            map.set_search(cx, &query);
        }
        if search.returned(actions).is_some() {
            map.next_match(cx);
        }
        if search.escaped(actions) {
            search.set_text(cx, "");
            map.set_search(cx, "");
        }
        // A widget can emit several actions in one batch, so look at all of them.
        let uid = map.widget_uid();
        for action in actions {
            let Some(wa) = action.as_widget_action() else { continue };
            if wa.widget_uid != uid {
                continue;
            }
            match wa.cast::<CodeMapAction>() {
                CodeMapAction::Status(text) => self.ui.label(cx, ids!(status)).set_text(cx, &text),
                CodeMapAction::Selected(info) => {
                    self.ui.label(cx, ids!(info_title)).set_text(cx, &info.title);
                    self.ui.label(cx, ids!(info_path)).set_text(cx, &info.path);
                    self.ui.label(cx, ids!(info_details)).set_text(cx, &info.details);
                }
                CodeMapAction::None => {}
            }
        }
    }
}

impl AppMain for App {
    fn script_mod(vm: &mut ScriptVm) -> ScriptValue {
        crate::makepad_widgets::script_mod(vm);
        crate::map_view::script_mod(vm);
        self::script_mod(vm)
    }

    fn handle_event(&mut self, cx: &mut Cx, event: &Event) {
        self.match_event(cx, event);
        self.ui.handle_event(cx, event, &mut Scope::empty());
    }
}
