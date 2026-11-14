use serde::Deserialize;

/// The camera's runtime settings, from world settings and user preferences.
/// The defaults are the TypeScript ones; the web app sets the real values.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct CameraConfig {
    pub fixed: FixedCamera,
    pub orbit: OrbitCamera,
    /// Horizontal distance (m) of the conversation camera from the focus.
    pub conversation_distance: f32,
    /// Height (m) of the conversation camera above the focus.
    pub conversation_height: f32,
    pub invert_look: bool,
    /// The followed player's collider radius, which places its feet.
    pub char_radius: f32,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            fixed: FixedCamera::default(),
            orbit: OrbitCamera::default(),
            conversation_distance: 7.0,
            conversation_height: 5.0,
            invert_look: false,
            char_radius: 0.4,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct FixedCamera {
    /// Compass heading from the character to the camera: 0 = N, 180 = S.
    pub azimuth_deg: f32,
    /// Negative looks down.
    pub pitch_deg: f32,
    /// The camera's height above the character.
    pub height: f32,
    /// The largest share of the screen height the character may fill before the camera backs off.
    pub max_on_screen_frac: f32,
    /// The smallest share before the camera follows.
    pub min_on_screen_frac: f32,
    /// Lateral leash as a fraction of the view's half-width at the far depth.
    pub lateral_frac: f32,
    pub fov: f32,
    /// Turns camera-relative movement by this much.
    pub movement_yaw_offset_deg: f32,
    /// Pins the XZ distance to the character, with no leash.
    pub distance_m: Option<f32>,
    /// A world point to orbit and look at instead of the followed player.
    pub look_at: Option<[f32; 3]>,
}

impl Default for FixedCamera {
    fn default() -> Self {
        Self {
            azimuth_deg: 202.5,
            pitch_deg: -35.0,
            height: 25.0,
            max_on_screen_frac: 0.25,
            min_on_screen_frac: 0.08,
            lateral_frac: 0.4,
            fov: 50.0,
            movement_yaw_offset_deg: 0.0,
            distance_m: None,
            look_at: None,
        }
    }
}

/// The editors' orbit camera. The defaults are the scene editor's.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct OrbitCamera {
    pub fov: f32,
    pub min_distance: f32,
    pub max_distance: f32,
    /// Positive looks down on the target from above.
    pub min_elevation_deg: f32,
    pub max_elevation_deg: f32,
    /// Right is the scene editor's layout and left drei `OrbitControls`'.
    pub rotate_button: RotateButton,
    pub pan: bool,
    /// Time constant (s) the camera eases toward its goal with; 0 snaps.
    /// drei `OrbitControls`' damping factor of 0.05 a frame is ~0.33 s at 60 fps.
    pub damping_secs: f32,
    /// WASD moves the target across the ground and Q/E up and down, as the
    /// map editor's camera does.
    pub move_keys: bool,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        let pole = crate::look::ORBIT_POLE.to_degrees();
        Self {
            fov: 45.0,
            min_distance: 2.0,
            max_distance: 200.0,
            min_elevation_deg: -pole,
            max_elevation_deg: pole,
            rotate_button: RotateButton::Right,
            pan: true,
            damping_secs: 0.0,
            move_keys: false,
        }
    }
}

impl OrbitCamera {
    /// The field of view, kept to one a camera can have.
    pub fn fov(&self) -> f32 {
        self.fov.clamp(1.0, 179.0)
    }
}

/// Right: right-drags rotate, shift+right- and middle-drags pan, and the left
/// button is left to the editor's tools. Left, drei `OrbitControls`': left-drags
/// rotate, right-drags pan, ctrl, cmd or shift swaps the two, and middle-drags zoom.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RotateButton {
    Left,
    Right,
}
