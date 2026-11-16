//! Bevy rendering into a surface owned by a Flutter app.
//!
//! [`host`] runs a Bevy app on a caller-provided surface handle, without winit, showing either
//! the [`scene`] demo or a live office room (`officexr-client`, reported on by [`office`]). On
//! Android, `android` exposes it to Kotlin over JNI; on iOS, `ios` exposes it to Swift as C
//! functions.

pub mod events;
pub mod host;
pub mod office;
pub mod scene;

#[cfg(target_os = "android")]
mod android;

#[cfg(target_os = "ios")]
mod ios;
