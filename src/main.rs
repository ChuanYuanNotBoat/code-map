//! Code Map: fly over a codebase as a zoomable GPU treemap.
//!
//!   cargo run --release -- /path/to/project [--3d]
//!   cargo run --release --bin scan -- /path/to/project   (no window, just stats)

pub use makepad_widgets;

mod history;
mod i18n;
mod map_view;
mod model;
mod orbit;
mod scan;

use makepad_widgets::*;
use i18n::Language;
use map_view::{CodeMapAction, CodeMapWidgetRefExt, CustomDetail, DetailLevel};
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
                        color_mode := DropDown2{
                            width: 150
                            labels: ["File type" "Recently changed" "Most changed"]
                        }
                        three_d := CheckBox{text: "3D" active: false}
                        detail_level := DropDown2{
                            width: 110
                            labels: ["Normal" "High" "Ultra" "Custom"]
                            selected_item: 0
                        }
                        language := DropDown2{
                            width: 100
                            labels: ["English" "简体中文"]
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
                            inspector_heading := Label{
                                text: "INSPECTOR"
                                draw_text +: {color: #x5a6570 text_style +: {font_size: 8.0}}
                            }
                            custom_detail_panel := View{
                                visible: false
                                width: Fill height: Fit
                                flow: Down spacing: 5
                                padding: Inset{top: 4, bottom: 8}
                                custom_detail_heading := Label{
                                    text: "CUSTOM DETAIL"
                                    draw_text +: {color: #x5a6570 text_style +: {font_size: 8.0}}
                                }
                                custom_geometry := Slider{
                                    width: Fill
                                    text: "Geometry detail"
                                    min: 0 max: 100 default: 70 step: 1 precision: 0
                                }
                                custom_text := Slider{
                                    width: Fill
                                    text: "Text detail"
                                    min: 0 max: 100 default: 70 step: 1 precision: 0
                                }
                                custom_budget := Slider{
                                    width: Fill
                                    text: "Render budget (%)"
                                    min: 50 max: 400 default: 200 step: 10 precision: 0
                                }
                            }
                            info_title := Label{
                                width: Fill
                                text: "Click something on the map"
                                draw_text +: {color: #xffffff text_style +: {font_size: 11.0}}
                            }
                            info_path := InfoLabel{text: ""}
                            info_details := InfoLabel{text: ""}
                            View{width: Fill height: Fill}
                            help := InfoLabel{
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
    #[rust]
    language: Language,
    #[rust]
    has_selection: bool,
}

/// The folder to map: first command line argument, or the current folder.
fn project_path() -> PathBuf {
    let arg = std::env::args().skip(1).find(|a| !a.starts_with("--"));
    let path = arg.map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    path.canonicalize().unwrap_or(path)
}

impl App {
    fn custom_detail(&self, cx: &mut Cx) -> CustomDetail {
        CustomDetail::new(
            self.ui
                .slider(cx, ids!(custom_geometry))
                .value()
                .unwrap_or(70.0),
            self.ui
                .slider(cx, ids!(custom_text))
                .value()
                .unwrap_or(70.0),
            self.ui
                .slider(cx, ids!(custom_budget))
                .value()
                .unwrap_or(200.0),
        )
    }

    fn apply_i18n(&mut self, cx: &mut Cx) {
        let language = self.language;
        self.ui
            .text_input(cx, ids!(search))
            .set_empty_text(cx, language.search_placeholder().to_string());
        self.ui.drop_down2(cx, ids!(color_mode)).set_labels(
            cx,
            language
                .color_modes()
                .into_iter()
                .map(str::to_string)
                .collect(),
        );
        self.ui.drop_down2(cx, ids!(detail_level)).set_labels(
            cx,
            language
                .detail_levels()
                .into_iter()
                .map(str::to_string)
                .collect(),
        );
        self.ui.drop_down2(cx, ids!(language)).set_labels(
            cx,
            Language::language_names()
                .into_iter()
                .map(str::to_string)
                .collect(),
        );
        self.ui.button(cx, ids!(fit_button)).set_text(cx, language.fit());
        self.ui
            .check_box(cx, ids!(show_ignored))
            .set_text(language.show_ignored());
        self.ui
            .label(cx, ids!(inspector_heading))
            .set_text(cx, language.inspector());
        self.ui
            .label(cx, ids!(custom_detail_heading))
            .set_text(cx, language.custom_detail());
        self.ui
            .widget(cx, ids!(custom_geometry))
            .set_text(cx, language.geometry_detail());
        self.ui
            .widget(cx, ids!(custom_text))
            .set_text(cx, language.text_detail());
        self.ui
            .widget(cx, ids!(custom_budget))
            .set_text(cx, language.render_budget());
        if !self.has_selection {
            self.ui
                .label(cx, ids!(info_title))
                .set_text(cx, language.select_hint());
        }
        self.ui
            .label(cx, ids!(help))
            .set_text(cx, language.help());
        self.ui.redraw(cx);
    }
}

impl MatchEvent for App {
    fn handle_startup(&mut self, cx: &mut Cx) {
        self.language = Language::detect();
        self.ui
            .drop_down2(cx, ids!(language))
            .set_selected_item(cx, self.language.index());
        self.apply_i18n(cx);
        let path = project_path();
        self.ui.label(cx, ids!(title)).set_text(cx, &format!("Code Map: {}", path.display()));
        let map = self.ui.code_map(cx, ids!(map));
        map.set_language(cx, self.language);
        map.open(cx, path);
        if std::env::args().any(|a| a == "--3d") {
            self.ui.check_box(cx, ids!(three_d)).set_active(cx, true, Animate::No);
            map.set_3d(cx, true);
        }
    }

    fn handle_actions(&mut self, cx: &mut Cx, actions: &Actions) {
        let map = self.ui.code_map(cx, ids!(map));
        if let Some(index) = self.ui.drop_down2(cx, ids!(language)).changed(actions) {
            self.language = Language::from_index(index);
            self.apply_i18n(cx);
            map.set_language(cx, self.language);
        }
        if self.ui.button(cx, ids!(fit_button)).clicked(actions) {
            map.fit(cx);
        }
        if let Some(show) = self.ui.check_box(cx, ids!(show_ignored)).changed(actions) {
            map.set_show_ignored(cx, show);
        }
        if let Some(on) = self.ui.check_box(cx, ids!(three_d)).changed(actions) {
            map.set_3d(cx, on);
        }
        if let Some(index) = self.ui.drop_down2(cx, ids!(detail_level)).changed(actions) {
            let level = match index {
                1 => DetailLevel::High,
                2 => DetailLevel::Ultra,
                3 => DetailLevel::Custom,
                _ => DetailLevel::Normal,
            };
            let custom = level == DetailLevel::Custom;
            self.ui
                .view(cx, ids!(custom_detail_panel))
                .set_visible(cx, custom);
            if custom {
                let detail = self.custom_detail(cx);
                map.set_custom_detail(cx, detail);
            }
            map.set_detail_level(cx, level);
        }
        let custom_changed = self
            .ui
            .slider(cx, ids!(custom_geometry))
            .slided(actions)
            .is_some()
            || self
                .ui
                .slider(cx, ids!(custom_text))
                .slided(actions)
                .is_some()
            || self
                .ui
                .slider(cx, ids!(custom_budget))
                .slided(actions)
                .is_some();
        if custom_changed {
            let detail = self.custom_detail(cx);
            map.set_custom_detail(cx, detail);
        }
        if let Some(index) = self.ui.drop_down2(cx, ids!(color_mode)).changed(actions) {
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
                    self.has_selection = true;
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
