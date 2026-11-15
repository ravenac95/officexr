//! THROWAWAY SPIKE (OSO-7951): WebXR per-eye cameras driven from
//! `XRSession.requestAnimationFrame`. See docs/webxr-bevy.md. Not for merge.
//!
//! JS calls `xrFrame` inside the XR frame callback with the viewer's views;
//! this stores them and wakes winit through its proxy. winit runs the wake-up
//! in a microtask, so Bevy updates and renders before the XR frame ends.

use std::cell::RefCell;

use bevy::camera::{CameraProjection, SubCameraView, Viewport};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::math::Vec3A;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::winit::{EventLoopProxyWrapper, UpdateMode, WinitSettings, WinitUserEvent};
use officexr_render::views::{View, ViewId};
use serde::Deserialize;
use wasm_bindgen::prelude::*;

use super::js_err;

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Eye {
    /// `[x, y, w, h]` in framebuffer pixels, origin bottom-left (WebGL's).
    viewport: [u32; 4],
    /// `XRView.transform.matrix`, relative to the XR origin.
    transform: [f32; 16],
    /// `XRView.projectionMatrix` (GL convention).
    projection: [f32; 16],
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Frame {
    view: ViewId,
    /// Where the XR reference space's origin sits in the world, and its yaw.
    origin: [f32; 3],
    yaw: f32,
    framebuffer_height: u32,
    eyes: Vec<Eye>,
}

thread_local! {
    static FRAME: RefCell<Option<Frame>> = const { RefCell::new(None) };
    static ENDED: RefCell<bool> = const { RefCell::new(false) };
    static UPDATES: RefCell<u32> = const { RefCell::new(0) };
    static DEBUG: RefCell<String> = const { RefCell::new(String::new()) };
    static PROXY: RefCell<Option<bevy::winit::EventLoopProxy<WinitUserEvent>>> =
        const { RefCell::new(None) };
}

/// Eye cameras are told apart by order (no `#[derive(Component)]` here: the
/// derive can't find `bevy` behind this crate's wasm-only dependency table).
const EYE_ORDER: isize = 100;

/// Bevy's reverse-Z infinite perspective from an XR view's (possibly
/// asymmetric) GL projection.
#[derive(Debug, Clone)]
struct XrProjection {
    p00: f32,
    p11: f32,
    p20: f32,
    p21: f32,
    near: f32,
}

impl XrProjection {
    fn from_gl(m: &[f32; 16], near: f32) -> Self {
        Self {
            p00: m[0],
            p11: m[5],
            p20: m[8],
            p21: m[9],
            near,
        }
    }
}

impl CameraProjection for XrProjection {
    fn get_clip_from_view(&self) -> Mat4 {
        Mat4::from_cols(
            Vec4::new(self.p00, 0.0, 0.0, 0.0),
            Vec4::new(0.0, self.p11, 0.0, 0.0),
            Vec4::new(self.p20, self.p21, 0.0, -1.0),
            Vec4::new(0.0, 0.0, self.near, 0.0),
        )
    }
    fn get_clip_from_view_for_sub(&self, _: &SubCameraView) -> Mat4 {
        self.get_clip_from_view()
    }
    fn update(&mut self, _: f32, _: f32) {}
    fn far(&self) -> f32 {
        1000.0
    }
    fn get_frustum_corners(&self, z_near: f32, z_far: f32) -> [Vec3A; 8] {
        let right = (1.0 - self.p20) / self.p00;
        let left = (-1.0 - self.p20) / self.p00;
        let top = (1.0 - self.p21) / self.p11;
        let bottom = (-1.0 - self.p21) / self.p11;
        let at = |z: f32, x: f32, y: f32| Vec3A::new(x * z.abs(), y * z.abs(), z);
        [
            at(z_near, right, bottom),
            at(z_near, right, top),
            at(z_near, left, top),
            at(z_near, left, bottom),
            at(z_far, right, bottom),
            at(z_far, right, top),
            at(z_far, left, top),
            at(z_far, left, bottom),
        ]
    }
}

pub(super) fn add(app: &mut App) {
    app.add_systems(Startup, keep_proxy)
        .add_systems(PostUpdate, apply_frame.before(TransformSystems::Propagate));
}

fn keep_proxy(proxy: Option<Res<EventLoopProxyWrapper>>) {
    if let Some(proxy) = proxy {
        PROXY.with_borrow_mut(|p| *p = Some((**proxy).clone()));
    }
}

fn wake() {
    PROXY.with_borrow(|p| {
        if let Some(p) = p {
            let _ = p.send_event(WinitUserEvent::WakeUp);
        }
    });
}

/// Called from the XR frame callback; Bevy renders this frame in a microtask.
#[wasm_bindgen(js_name = xrFrame)]
pub fn xr_frame(frame: JsValue) -> Result<(), JsError> {
    let frame: Frame = serde_wasm_bindgen::from_value(frame).map_err(js_err)?;
    FRAME.with_borrow_mut(|f| *f = Some(frame));
    wake();
    Ok(())
}

/// How many updates applied an XR frame, to check they run inside the XR callback.
#[wasm_bindgen(js_name = xrUpdates)]
pub fn xr_updates() -> u32 {
    UPDATES.with_borrow(|u| *u)
}

#[wasm_bindgen(js_name = xrDebug)]
pub fn xr_debug() -> String {
    DEBUG.with_borrow(Clone::clone)
}

#[wasm_bindgen(js_name = xrEnd)]
pub fn xr_end() {
    ENDED.with_borrow_mut(|e| *e = true);
    FRAME.with_borrow_mut(|f| *f = None);
    wake();
}

fn apply_frame(
    mut commands: Commands,
    views: Query<(Entity, &View, &Children)>,
    mut cameras: Query<(Entity, &mut Camera, &mut Transform, &mut Projection), With<Camera3d>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut settings: ResMut<WinitSettings>,
) {
    if ENDED.with_borrow_mut(std::mem::take) {
        *settings = WinitSettings::game();
        for mut window in &mut windows {
            window.visible = true;
        }
        for (entity, mut camera, _, _) in &mut cameras {
            if camera.order >= EYE_ORDER {
                commands.entity(entity).despawn();
            } else {
                camera.is_active = true;
            }
        }
        return;
    }
    let Some(frame) = FRAME.with_borrow(Clone::clone) else {
        return;
    };
    UPDATES.with_borrow_mut(|u| *u += 1);
    let dump: Vec<String> = cameras
        .iter()
        .map(|(e, c, t, _)| {
            format!(
                "{e} order={} active={} viewport={:?} pos={:?} size={:?}",
                c.order,
                c.is_active,
                c.viewport
                    .as_ref()
                    .map(|v| (v.physical_position, v.physical_size)),
                t.translation,
                c.physical_target_size(),
            )
        })
        .collect();
    DEBUG.with_borrow_mut(|d| *d = dump.join("\n"));
    let Some((view_entity, _, children)) = views.iter().find(|(_, v, _)| v.id == frame.view) else {
        return;
    };
    // bevy_winit updates once per window `RedrawRequested` (window rAF, which a
    // headset pauses in immersive mode) unless every window is invisible; then
    // each wake-up updates. Reactive mode stops the window rAF updates too.
    // `visible` is a no-op for winit's web canvas.
    for mut window in &mut windows {
        if window.visible {
            window.visible = false;
        }
    }
    if matches!(settings.focused_mode, UpdateMode::Continuous) {
        let xr = UpdateMode::Reactive {
            wait: std::time::Duration::MAX,
            react_to_device_events: false,
            react_to_user_events: true,
            react_to_window_events: false,
        };
        *settings = WinitSettings {
            focused_mode: xr,
            unfocused_mode: xr,
        };
    }
    let origin = Transform::from_translation(frame.origin.into())
        .with_rotation(Quat::from_rotation_y(frame.yaw));
    let mut spawned = vec![false; frame.eyes.len()];
    for (_, mut camera, mut transform, mut projection) in &mut cameras {
        if camera.order < EYE_ORDER {
            continue;
        }
        let i = (camera.order - EYE_ORDER) as usize;
        let Some(input) = frame.eyes.get(i) else {
            continue;
        };
        spawned[i] = true;
        *transform = eye_transform(&origin, input);
        *projection = Projection::custom(XrProjection::from_gl(&input.projection, 0.05));
        camera.viewport = Some(viewport(input, frame.framebuffer_height));
    }
    // ponytail: the view's own camera just goes inactive; its sky dome stays behind on it.
    for &child in children {
        if let Ok((_, mut camera, _, _)) = cameras.get_mut(child)
            && camera.order < EYE_ORDER
        {
            camera.is_active = false;
        }
    }
    for (i, input) in frame.eyes.iter().enumerate() {
        if spawned[i] {
            continue;
        }
        commands.spawn((
            Camera3d::default(),
            Camera {
                order: EYE_ORDER + i as isize,
                viewport: Some(viewport(input, frame.framebuffer_height)),
                // Both eyes share the window's main texture; only the first clears it.
                clear_color: if i == 0 {
                    ClearColorConfig::Default
                } else {
                    ClearColorConfig::None
                },
                ..default()
            },
            Tonemapping::AcesFitted,
            bevy::camera::Exposure { ev100: -1.0 },
            eye_transform(&origin, input),
            Projection::custom(XrProjection::from_gl(&input.projection, 0.05)),
            ChildOf(view_entity),
        ));
    }
}

fn eye_transform(origin: &Transform, eye: &Eye) -> Transform {
    Transform::from_matrix(origin.to_matrix() * Mat4::from_cols_array(&eye.transform))
}

/// WebGL viewports count from the bottom; Bevy's from the top.
fn viewport(eye: &Eye, framebuffer_height: u32) -> Viewport {
    let [x, y, w, h] = eye.viewport;
    Viewport {
        physical_position: UVec2::new(x, framebuffer_height.saturating_sub(y + h)),
        physical_size: UVec2::new(w.max(1), h.max(1)),
        ..default()
    }
}
