//! The 3D city view. The same treemap layout, but every folder is a plate
//! stacked on its parent and every file is a tower on top of its folder.
//! Tower height: size of the file (File type mode) or git activity (heatmap modes).
//!
//! How it renders: we draw into our own offscreen render pass with a depth
//! buffer (so near boxes hide far ones, like WebGL depth testing), using
//! Makepad's built-in instanced `DrawCube`. The pass output is a texture,
//! which we then show as a normal 2D quad, with 2D labels drawn on top.
//!
//! This is a child module of `map_view`, so it can use the widget's private fields.

use super::*;
use crate::orbit::{ray_box, Frame};

/// Width of the whole map in 3D units.
const CITY_SIZE: f32 = 10.0;
/// Thickness of one folder plate.
const PLATE: f32 = 0.035;
const CUBE_BUDGET: usize = 250_000;
const LABELS_3D: usize = 120;

impl CodeMap {
    /// World units (2D layout) to 3D units.
    pub(super) fn scale_3d(&self) -> f32 {
        CITY_SIZE / self.world.w.max(1.0) as f32
    }

    fn ensure_pass(&mut self, cx: &mut Cx) {
        if self.pass_ready {
            return;
        }
        self.pass_ready = true;
        self.color_texture = Texture::new_with_format(cx, TextureFormat::RenderBGRAu8 { size: TextureSize::Auto, initial: true });
        self.depth_texture = Texture::new_with_format(cx, TextureFormat::DepthD32 { size: TextureSize::Auto, initial: true });
        cx.passes[self.pass.draw_pass_id()].keep_camera_matrix = true;
    }

    pub(super) fn draw_3d(&mut self, cx: &mut Cx2d, rect: Rect) {
        self.ensure_pass(cx.cx);
        self.pass.set_size(cx, rect.size);
        self.pass.set_color_texture(cx, &self.color_texture, DrawPassClearColor::ClearWith(vec4(0.027, 0.035, 0.047, 1.0)));
        self.pass.set_depth_texture(cx, &self.depth_texture, DrawPassClearDepth::ClearWith(1.0));

        let frame = self.orbit.frame(rect);
        cx.make_child_pass(&self.pass);
        cx.begin_pass(&self.pass, None);
        set_pass_camera(cx.cx, &self.pass, &frame);
        let mut label_spots = Vec::new();
        {
            let cx3d = &mut Cx3d::new(cx.cx);
            self.draw_list.begin_always(cx3d);
            cx3d.begin_scene_3d(SceneState3D {
                time: 0.0,
                camera_pos: frame.eye,
                view: frame.view,
                projection: frame.projection,
                viewport_rect: rect,
            });
            let previous = cx3d.set_scene_world_transform_3d(Mat4f::identity());
            self.draw_city(cx3d, &frame, &mut label_spots);
            if let Some(previous) = previous {
                cx3d.set_scene_world_transform_3d(previous);
            }
            cx3d.end_scene_3d();
            self.draw_list.end(cx3d);
        }
        cx.end_pass(&self.pass);

        self.draw_scene.draw_super.draw_vars.set_texture(0, &self.color_texture);
        self.draw_scene.draw_abs(cx, rect);
        cx.set_pass_area(&self.pass, self.draw_scene.draw_super.draw_vars.area);

        // biggest folders get labels first
        label_spots.sort_by(|a: &(f32, usize, Vec3f), b| b.0.total_cmp(&a.0));
        for (px, index, point) in label_spots.into_iter().take(LABELS_3D) {
            if let Some(p) = frame.project(point) {
                let width = (px as f64).clamp(40.0, 260.0);
                self.labels.push(Label {
                    rect: Rect { pos: p - dvec2(width * 0.5, 8.0), size: dvec2(width, 16.0) },
                    text: self.tree.nodes[index].name.clone(),
                    dim: self.tree.nodes[index].ignored,
                });
            }
        }
        if let Some(h) = self.hover {
            if h < self.tree.nodes.len() {
                self.labels.push(Label {
                    rect: Rect { pos: self.hover_abs + dvec2(14.0, 12.0), size: dvec2(320.0, 16.0) },
                    text: self.tree.nodes[h].path.clone(),
                    dim: false,
                });
            }
        }
    }

    fn draw_city(&mut self, cx: &mut Cx3d, frame: &Frame, label_spots: &mut Vec<(f32, usize, Vec3f)>) {
        let k = self.scale_3d();
        let (ox, oz) = (self.world.w as f32 * 0.5, self.world.h as f32 * 0.5);
        let searching = self.search_active();
        self.picks.clear();
        self.draw_cube.transform = Mat4f::identity();
        self.draw_cube.depth_clip = 0.0;
        self.draw_cube.begin_many_instances(cx);

        // (node, height of the surface it stands on)
        let mut stack: Vec<(usize, f32)> = vec![(0, 0.0)];
        while let Some((index, base)) = stack.pop() {
            let node = &self.tree.nodes[index];
            if node.weight <= 0.0 {
                continue;
            }
            if self.picks.len() >= CUBE_BUDGET {
                break;
            }
            let r = node.rect;
            let (x, z, w, d) = ((r.x as f32 - ox) * k, (r.y as f32 - oz) * k, r.w as f32 * k, r.h as f32 * k);
            let footprint = w.max(d);
            let height = match node.kind {
                Kind::Dir => PLATE,
                Kind::Ghost { .. } => PLATE * 0.6,
                Kind::Text | Kind::Binary => {
                    let h = match self.color_mode {
                        ColorMode::FileType => 0.04 + 0.5 * ((node.lines.len() as f32 + 1.0).log10() / 5.0).min(1.0),
                        _ if node.heat < 0.0 => 0.02,
                        _ => 0.04 + 1.1 * node.heat * node.heat,
                    };
                    // no needles: a tiny file can't be a skyscraper
                    h.min(footprint * 3.0).max(0.002)
                }
            };
            let center = vec3f(x + w * 0.5, base + height * 0.5, z + d * 0.5);
            let radius = 0.5 * (w * w + d * d + height * height).sqrt();
            if !frame.sphere_visible(center, radius) {
                continue;
            }
            let px = frame.pixels_for(center, footprint);
            if px < 1.0 {
                continue;
            }
            let is_dir = node.kind == Kind::Dir;
            // small gap between files so neighbours read as separate towers
            let inset = if is_dir { 0.0 } else { w.min(d) * 0.08 };
            let min = vec3f(x + inset, base, z + inset);
            let max = vec3f(x + w - inset, base + height, z + d - inset);

            let mut color = if is_dir {
                let hue = (self.tree.top_level(index) as f32 * 0.618_034).fract();
                hsv(hue, 0.35, 0.16 + 0.05 * node.depth.min(6) as f32)
            } else if let Kind::Ghost { loading, .. } = node.kind {
                if loading { vec4(0.5, 0.4, 0.15, 1.0) } else { vec4(0.22, 0.2, 0.17, 1.0) }
            } else {
                self.file_color(index)
            };
            if node.ignored {
                color = scale_rgb(color, 0.6);
            }
            if searching {
                if !node.on_path {
                    color = scale_rgb(color, 0.15);
                } else if node.lit && !is_dir {
                    color = mix_rgb(color, vec4(1.0, 0.9, 0.3, 1.0), 0.5);
                }
            }
            if self.selected == Some(index) {
                color = mix_rgb(color, vec4(0.35, 0.85, 1.0, 1.0), 0.6);
            } else if self.hover == Some(index) {
                color = mix_rgb(color, vec4(1.0, 1.0, 1.0, 1.0), 0.25);
            }

            self.draw_cube.color = color;
            self.draw_cube.cube_pos = vec3f((min.x + max.x) * 0.5, (min.y + max.y) * 0.5, (min.z + max.z) * 0.5);
            self.draw_cube.cube_size = max - min;
            self.draw_cube.draw(cx);
            self.picks.push((index, min, max));

            if is_dir && px > 10.0 {
                let top = base + height;
                stack.extend(node.children.iter().rev().map(|&c| (c, top)));
                if px > 90.0 && (!searching || node.on_path) {
                    label_spots.push((px, index, vec3f(center.x, top, center.z)));
                }
            }
        }
        self.draw_cube.end_many_instances(cx);
    }

    /// The box under the mouse: closest hit along the ray.
    pub(super) fn pick_3d(&self, abs: DVec2) -> Option<usize> {
        let frame = self.orbit.frame(self.view);
        let (origin, dir) = frame.ray(abs);
        let mut best: Option<(f32, usize)> = None;
        for &(index, min, max) in &self.picks {
            if let Some(t) = ray_box(origin, dir, min, max) {
                if best.is_none_or(|(bt, _)| t < bt) {
                    best = Some((t, index));
                }
            }
        }
        best.map(|(_, index)| index)
    }

    pub(super) fn fly_3d(&mut self, cx: &mut Cx, index: usize) {
        let k = self.scale_3d();
        let r = self.tree.nodes[index].rect;
        let target = vec3f(
            ((r.x + r.w * 0.5) as f32 - self.world.w as f32 * 0.5) * k,
            0.0,
            ((r.y + r.h * 0.5) as f32 - self.world.h as f32 * 0.5) * k,
        );
        let size = (r.w.max(r.h) as f32) * k;
        let distance = size / (2.0 * (self.orbit.fov_y.to_radians() * 0.5).tan()) * 1.4;
        self.orbit_flight = Some((target, distance.max(0.05)));
        self.frame = cx.new_next_frame();
    }

    pub(super) fn step_orbit_flight(&mut self, cx: &mut Cx) {
        let Some((target, distance)) = self.orbit_flight else { return };
        let t = 0.18;
        self.orbit.target = self.orbit.target + (target - self.orbit.target) * t;
        self.orbit.distance = (self.orbit.distance.ln() + (distance.ln() - self.orbit.distance.ln()) * t).exp();
        let close = (self.orbit.target - target).length() < distance * 0.002 && (self.orbit.distance / distance - 1.0).abs() < 0.002;
        if close {
            self.orbit.target = target;
            self.orbit.distance = distance;
            self.orbit_flight = None;
        } else {
            self.frame = cx.new_next_frame();
        }
        self.redraw(cx);
    }

    pub(super) fn handle_3d_input(&mut self, cx: &mut Cx, hit: Hit) {
        match hit {
            Hit::FingerMove(e) => {
                if let Some(mut drag) = self.drag {
                    if (e.abs - drag.start).length() > 4.0 {
                        drag.moved = true;
                    }
                    if drag.moved {
                        let delta = e.abs - drag.last;
                        if drag.pan {
                            self.orbit.pan(delta, self.view);
                        } else {
                            // drag sideways to spin, up and down to tilt
                            self.orbit.yaw -= delta.x as f32 * 0.008;
                            self.orbit.pitch = (self.orbit.pitch - delta.y as f32 * 0.006).clamp(-1.55, -0.08);
                        }
                        cx.set_cursor(MouseCursor::Grabbing);
                        self.redraw(cx);
                    }
                    drag.last = e.abs;
                    self.drag = Some(drag);
                }
            }
            Hit::FingerScroll(e) => {
                // zoom toward the point on the ground under the mouse
                let frame = self.orbit.frame(self.view);
                let (origin, dir) = frame.ray(e.abs);
                let old = self.orbit.distance;
                let new = (old * (e.scroll.y as f32 * 0.01).exp()).clamp(0.02, 60.0);
                if dir.y < -0.01 {
                    let t = -origin.y / dir.y;
                    let ground = origin + dir * t;
                    self.orbit.target = ground + (self.orbit.target - ground) * (new / old);
                }
                self.orbit.distance = new;
                self.redraw(cx);
            }
            _ => {}
        }
    }
}

fn set_pass_camera(cx: &mut Cx, pass: &DrawPass, frame: &Frame) {
    let camera_inv = frame.view.invert();
    let u = &mut cx.passes[pass.draw_pass_id()].pass_uniforms;
    u.camera_projection = frame.projection;
    u.camera_projection_r = frame.projection;
    u.camera_view = frame.view;
    u.camera_view_r = frame.view;
    u.depth_projection = frame.projection;
    u.depth_projection_r = frame.projection;
    u.depth_view = frame.view;
    u.depth_view_r = frame.view;
    u.camera_inv = camera_inv;
    u.camera_inv_r = camera_inv;
}
