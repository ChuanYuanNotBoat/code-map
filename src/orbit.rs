//! An orbit camera for the 3D city view, plus the two bits of math we need
//! on the CPU: projecting a 3D point to the screen (for labels) and turning
//! a mouse position into a ray (for clicking on boxes).
//!
//! Same idea as OrbitControls + PerspectiveCamera in Three.js.

use makepad_widgets::*;

#[derive(Clone, Copy, Debug)]
pub struct Orbit {
    /// Rotation around the vertical axis, radians.
    pub yaw: f32,
    /// Tilt: 0 looks at the horizon, -PI/2 looks straight down.
    pub pitch: f32,
    pub distance: f32,
    pub target: Vec3f,
    pub fov_y: f32,
}

impl Default for Orbit {
    fn default() -> Self {
        Orbit { yaw: 0.35, pitch: -0.85, distance: 12.0, target: vec3f(0.0, 0.0, 0.0), fov_y: 40.0 }
    }
}

pub struct Frame {
    pub view: Mat4f,
    pub projection: Mat4f,
    pub eye: Vec3f,
    forward: Vec3f,
    side: Vec3f,
    up: Vec3f,
    tan_half: f32,
    aspect: f32,
    pub rect: Rect,
}

impl Orbit {
    pub fn forward(&self) -> Vec3f {
        vec3f(self.yaw.sin() * self.pitch.cos(), self.pitch.sin(), -self.yaw.cos() * self.pitch.cos()).normalize()
    }

    pub fn near_far(&self) -> (f32, f32) {
        ((self.distance * 0.01).max(0.001), self.distance * 20.0 + 50.0)
    }

    pub fn frame(&self, rect: Rect) -> Frame {
        let aspect = (rect.size.x / rect.size.y.max(1.0)).max(0.001) as f32;
        let forward = self.forward();
        let eye = self.target - forward * self.distance;
        let (near, far) = self.near_far();
        let side = Vec3f::cross(forward, vec3f(0.0, 1.0, 0.0)).normalize();
        let up = Vec3f::cross(side, forward);
        Frame {
            view: Mat4f::look_at(eye, self.target, vec3f(0.0, 1.0, 0.0)),
            projection: Mat4f::perspective(self.fov_y, aspect, near, far),
            eye,
            forward,
            side,
            up,
            tan_half: (self.fov_y.to_radians() * 0.5).tan(),
            aspect,
            rect,
        }
    }

    /// Move the target along the ground plane by a screen-space drag.
    pub fn pan(&mut self, delta: DVec2, rect: Rect) {
        let f = self.forward();
        let right = vec3f(-f.z, 0.0, f.x).normalize();
        let ahead = vec3f(f.x, 0.0, f.z).normalize();
        let per_px = 2.0 * self.distance * (self.fov_y.to_radians() * 0.5).tan() / rect.size.y.max(1.0) as f32;
        self.target = self.target - right * (delta.x as f32 * per_px) + ahead * (delta.y as f32 * per_px);
    }
}

impl Frame {
    /// World point to screen position. None if it is behind the camera.
    pub fn project(&self, p: Vec3f) -> Option<DVec2> {
        let clip = self.projection.transform_vec4(self.view.transform_vec4(vec4f(p.x, p.y, p.z, 1.0)));
        if clip.w <= 0.0001 {
            return None;
        }
        let nx = clip.x / clip.w;
        let ny = clip.y / clip.w;
        Some(dvec2(
            self.rect.pos.x + (nx as f64 * 0.5 + 0.5) * self.rect.size.x,
            self.rect.pos.y + (0.5 - ny as f64 * 0.5) * self.rect.size.y,
        ))
    }

    /// Ray from the eye through a screen position.
    pub fn ray(&self, abs: DVec2) -> (Vec3f, Vec3f) {
        let nx = (((abs.x - self.rect.pos.x) / self.rect.size.x) * 2.0 - 1.0) as f32;
        let ny = (1.0 - ((abs.y - self.rect.pos.y) / self.rect.size.y) * 2.0) as f32;
        let dir = self.forward + self.side * (nx * self.tan_half * self.aspect) + self.up * (ny * self.tan_half);
        (self.eye, dir.normalize())
    }

    /// How many pixels tall something of `size` world units looks at point `p`.
    pub fn pixels_for(&self, p: Vec3f, size: f32) -> f32 {
        let depth = (p - self.eye).dot(self.forward).max(0.0001);
        size / (depth * self.tan_half) * 0.5 * self.rect.size.y as f32
    }

    /// How many pixels something of `size` world units looks at `distance` from the eye.
    pub fn pixels_at_distance(&self, distance: f32, size: f32) -> f32 {
        size / (distance.max(0.00001) * self.tan_half) * 0.5 * self.rect.size.y as f32
    }

    /// Rough frustum test for a sphere, so we skip boxes that are off screen.
    pub fn sphere_visible(&self, c: Vec3f, radius: f32) -> bool {
        let rel = c - self.eye;
        let depth = rel.dot(self.forward);
        if depth < -radius {
            return false;
        }
        let d = depth.max(0.0);
        let x = rel.dot(self.side).abs();
        let y = rel.dot(self.up).abs();
        x <= d * self.tan_half * self.aspect + radius * 1.5 && y <= d * self.tan_half + radius * 1.5
    }
}

/// Ray vs axis-aligned box. Returns the distance along the ray if it hits.
pub fn ray_box(origin: Vec3f, dir: Vec3f, min: Vec3f, max: Vec3f) -> Option<f32> {
    let mut t0 = 0.0f32;
    let mut t1 = f32::MAX;
    for (o, d, lo, hi) in [(origin.x, dir.x, min.x, max.x), (origin.y, dir.y, min.y, max.y), (origin.z, dir.z, min.z, max.z)] {
        if d.abs() < 1e-8 {
            if o < lo || o > hi {
                return None;
            }
            continue;
        }
        let a = (lo - o) / d;
        let b = (hi - o) / d;
        t0 = t0.max(a.min(b));
        t1 = t1.min(a.max(b));
        if t0 > t1 {
            return None;
        }
    }
    Some(t0)
}
