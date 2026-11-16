//! Sentinel values and fixed strings of the TWS API.

/// "No value" for 32-bit integer fields. Encoded as an empty field where IB's
/// client does so.
pub const UNSET_INTEGER: i32 = i32::MAX;
/// "No value" for 64-bit integer fields.
pub const UNSET_LONG: i64 = i64::MAX;
/// "No value" for floating point and decimal fields.
pub const UNSET_DOUBLE: f64 = f64::MAX;
/// Wire spelling of positive infinity.
pub const INFINITY_STR: &str = "Infinite";
/// Request id used by the server for messages that belong to no request.
pub const NO_VALID_ID: i32 = -1;
/// Largest frame the client accepts (IB's `MAX_MSG_LEN`).
pub const MAX_MSG_LEN: usize = 0xFF_FFFF;
/// `competeAgainstBestOffset` value that means "up to mid".
pub const COMPETE_AGAINST_BEST_OFFSET_UP_TO_MID: f64 = f64::INFINITY;

/// Error codes that ib_async treats as warnings rather than errors.
pub fn is_warning_code(code: i32) -> bool {
    matches!(
        code,
        105 | 110 | 165 | 321 | 329 | 399 | 404 | 434 | 492 | 10167
    ) || (2100..2200).contains(&code)
}
