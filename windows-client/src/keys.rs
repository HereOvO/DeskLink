//! Linux evdev key codes -> Windows set-1 scan codes.
//!
//! The Windows host converts `VK` codes to Linux key codes (with the PC scan
//! code as a fallback) before putting them on the wire. evdev deliberately
//! reuses the PC scan code numbering for the classic keys, so only the handful
//! of keys that Windows addresses with an extended scan code need a mapping
//! here; everything else can be forwarded as-is.

/// Returns `(scan_code, is_extended)`.
pub fn evdev_to_scancode(code: u16) -> Option<(u16, bool)> {
    let mapped = match code {
        // Navigation cluster and keypad keys that use extended scan codes on
        // Windows but plain numbers in the evdev namespace.
        96 => (0x1C, true),  // KP_ENTER
        97 => (0x1D, true),  // RIGHT_CTRL
        98 => (0x35, true),  // KP_SLASH
        100 => (0x38, true), // RIGHT_ALT
        102 => (0x47, true), // HOME
        103 => (0x48, true), // UP
        104 => (0x49, true), // PAGE_UP
        105 => (0x4B, true), // LEFT
        106 => (0x4D, true), // RIGHT
        107 => (0x4F, true), // END
        108 => (0x50, true), // DOWN
        109 => (0x51, true), // PAGE_DOWN
        110 => (0x52, true), // INSERT
        111 => (0x53, true), // DELETE
        125 | 126 => (0x5B, true), // LEFT_META / RIGHT_META
        // Everything else in the classic range already is a set-1 scan code.
        1..=0x7F => (code, false),
        _ => return None,
    };
    Some(mapped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_keys_are_identity() {
        // evdev KEY_A / KEY_1 / KEY_SEMICOLON / KEY_F1
        assert_eq!(evdev_to_scancode(30), Some((0x1E, false)));
        assert_eq!(evdev_to_scancode(2), Some((0x02, false)));
        assert_eq!(evdev_to_scancode(39), Some((0x27, false)));
        assert_eq!(evdev_to_scancode(59), Some((0x3B, false)));
    }

    #[test]
    fn arrows_use_extended_scan_codes() {
        assert_eq!(evdev_to_scancode(103), Some((0x48, true)));
        assert_eq!(evdev_to_scancode(105), Some((0x4B, true)));
        assert_eq!(evdev_to_scancode(106), Some((0x4D, true)));
        assert_eq!(evdev_to_scancode(108), Some((0x50, true)));
    }

    #[test]
    fn out_of_range_is_rejected() {
        assert_eq!(evdev_to_scancode(0), None);
        assert_eq!(evdev_to_scancode(0x100), None);
    }
}
