// SPDX-FileCopyrightText: 2026 Miikka Koskinen
//
// SPDX-License-Identifier: MIT

//! Server-side limits that bound how much data a single request or meeting can create.

/// Maximum request body size. A maximal meeting form (100 slots with
/// 100-character labels, percent-encoded) fits comfortably.
pub const MAX_BODY_BYTES: usize = 256 * 1024;

pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_LABEL_CHARS: usize = 100;
/// `YYYY-MM-DD`
pub const MAX_DATE_CHARS: usize = 10;
/// `HH:MM` or `HH:MM:SS`
pub const MAX_TIME_CHARS: usize = 8;
pub const MAX_NAME_CHARS: usize = 100;

pub const MAX_SLOTS_PER_MEETING: usize = 100;
/// Latest allowed slot date, in days from today.
pub const MAX_SLOT_DAYS_AHEAD: u64 = 2 * 366;
/// Largest allowed gap between a meeting's earliest and latest slot dates.
pub const MAX_SLOT_SPAN_DAYS: i64 = 366;
pub const MAX_PARTICIPANTS_PER_MEETING: usize = 200;

/// Reject `value` if it is longer than `max` characters.
pub fn check_len(field: &str, value: &str, max: usize) -> Result<(), String> {
    if value.chars().count() > max {
        Err(format!("{field} must be at most {max} characters"))
    } else {
        Ok(())
    }
}

/// Reject `values` if there are more than `max` of them or any is longer than `max_chars`.
pub fn check_all(
    field: &str,
    values: &[String],
    max: usize,
    max_chars: usize,
) -> Result<(), String> {
    if values.len() > max {
        return Err(format!("at most {max} {field} values are allowed"));
    }
    values
        .iter()
        .try_for_each(|value| check_len(field, value, max_chars))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_len_counts_chars_not_bytes() {
        assert!(check_len("name", "ääää", 4).is_ok());
        assert!(check_len("name", "äääää", 4).is_err());
    }

    #[test]
    fn check_all_limits_count_and_length() {
        let values = vec!["ab".to_string(), "cd".to_string()];
        assert!(check_all("slot", &values, 2, 2).is_ok());
        assert!(check_all("slot", &values, 1, 2).is_err());
        assert!(check_all("slot", &values, 2, 1).is_err());
    }
}
