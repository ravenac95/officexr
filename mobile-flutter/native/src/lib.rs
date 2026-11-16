//! Bevy rendering into a surface owned by a Flutter app.
//!
//! [`host`] runs a Bevy app on a caller-provided surface handle, without winit;
//! [`scene`] is the demo it draws. On Android, `android` exposes both to Kotlin over JNI.

pub mod host;
pub mod scene;

#[cfg(target_os = "android")]
mod android;
