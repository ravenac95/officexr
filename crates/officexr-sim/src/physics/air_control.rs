/// Next air velocity `(x, z)` after blending `current` toward `intent`.
/// `air_control` is the fraction of the gap closed per 1/60 s, so the
/// result does not depend on frame rate.
pub fn step_air_velocity(
    current: (f64, f64),
    intent: (f64, f64),
    air_control: f64,
    dt_sec: f64,
) -> (f64, f64) {
    let alpha = 1.0 - (1.0 - air_control).powf(dt_sec * 60.0);
    (
        current.0 + (intent.0 - current.0) * alpha,
        current.1 + (intent.1 - current.1) * alpha,
    )
}
