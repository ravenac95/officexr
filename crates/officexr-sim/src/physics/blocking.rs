/// The fraction in `[0, 1]` of the intended horizontal `(x, z)` motion the
/// corrected motion achieved along the intent; 1 when there is no intent.
pub fn horizontal_progress(corrected: (f64, f64), intent: (f64, f64)) -> f64 {
    let intent_len_sq = intent.0 * intent.0 + intent.1 * intent.1;
    if intent_len_sq <= 1e-12 {
        return 1.0;
    }
    let dot = corrected.0 * intent.0 + corrected.1 * intent.1;
    (dot / intent_len_sq).clamp(0.0, 1.0)
}
