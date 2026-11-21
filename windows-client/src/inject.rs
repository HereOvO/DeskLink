//! Windows input injection through `SendInput`.

use anyhow::{bail, Result};
use desklink_protocol::InputEvent;
use std::collections::HashSet;
use tracing::debug;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MOUSEEVENTF_HWHEEL,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
    MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN,
    MOUSEEVENTF_XUP, MOUSEINPUT, MOUSE_EVENT_FLAGS,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{WHEEL_DELTA, XBUTTON1, XBUTTON2};

use crate::keys::evdev_to_scancode;

/// Injects mouse buttons, wheels and key strokes.
///
/// Cursor motion is *not* injected as relative deltas: the client keeps its own
/// logical cursor and places the real pointer absolutely, which keeps the two
/// sides exactly in sync regardless of the Windows pointer-acceleration setting.
pub struct Injector {
    dry_run: bool,
    pressed_keys: HashSet<u16>,
    pressed_buttons: HashSet<u8>,
}

impl Injector {
    pub fn new(dry_run: bool) -> Self {
        Self {
            dry_run,
            pressed_keys: HashSet::new(),
            pressed_buttons: HashSet::new(),
        }
    }

    pub fn emit(&mut self, event: InputEvent) -> Result<()> {
        match event {
            InputEvent::MouseMove { .. } => Ok(()),
            InputEvent::MouseButton { button, pressed } => {
                if pressed {
                    self.pressed_buttons.insert(button);
                } else {
                    self.pressed_buttons.remove(&button);
                }
                let (flag, data) = match (button, pressed) {
                    (1, true) => (MOUSEEVENTF_LEFTDOWN, 0),
                    (1, false) => (MOUSEEVENTF_LEFTUP, 0),
                    (2, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
                    (2, false) => (MOUSEEVENTF_RIGHTUP, 0),
                    (3, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
                    (3, false) => (MOUSEEVENTF_MIDDLEUP, 0),
                    (4, true) => (MOUSEEVENTF_XDOWN, XBUTTON1 as u32),
                    (4, false) => (MOUSEEVENTF_XUP, XBUTTON1 as u32),
                    (5, true) => (MOUSEEVENTF_XDOWN, XBUTTON2 as u32),
                    (5, false) => (MOUSEEVENTF_XUP, XBUTTON2 as u32),
                    (other, _) => return Ok(debug!(button = other, "ignored mouse button")),
                };
                self.send_mouse(0, 0, data, flag)
            }
            InputEvent::MouseWheel {
                vertical,
                horizontal,
            } => {
                if vertical != 0 {
                    self.send_mouse(
                        0,
                        0,
                        (vertical as i32 * WHEEL_DELTA as i32) as u32,
                        MOUSEEVENTF_WHEEL,
                    )?;
                }
                if horizontal != 0 {
                    self.send_mouse(
                        0,
                        0,
                        (horizontal as i32 * WHEEL_DELTA as i32) as u32,
                        MOUSEEVENTF_HWHEEL,
                    )?;
                }
                Ok(())
            }
            InputEvent::Key { code, pressed } => {
                let Some((scan_code, extended)) = evdev_to_scancode(code) else {
                    return Ok(debug!(code, "no scan-code mapping for key; ignored"));
                };
                if pressed {
                    self.pressed_keys.insert(code);
                } else {
                    self.pressed_keys.remove(&code);
                }
                let mut flags: KEYBD_EVENT_FLAGS = KEYEVENTF_SCANCODE;
                if extended {
                    flags |= KEYEVENTF_EXTENDEDKEY;
                }
                if !pressed {
                    flags |= KEYEVENTF_KEYUP;
                }
                debug!(code, scan_code, pressed, extended, "key");
                self.send_key(scan_code, flags)
            }
        }
    }

    /// Releases everything the host currently holds down. Called whenever the
    /// session ends so a key never stays stuck on this machine.
    pub fn release_all(&mut self) -> Result<()> {
        let keys: Vec<u16> = self.pressed_keys.drain().collect();
        for code in keys {
            if let Some((scan_code, extended)) = evdev_to_scancode(code) {
                let mut flags: KEYBD_EVENT_FLAGS = KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP;
                if extended {
                    flags |= KEYEVENTF_EXTENDEDKEY;
                }
                self.send_key(scan_code, flags)?;
            }
        }
        let buttons: Vec<u8> = self.pressed_buttons.drain().collect();
        for button in buttons {
            let (flag, data) = match button {
                1 => (MOUSEEVENTF_LEFTUP, 0),
                2 => (MOUSEEVENTF_RIGHTUP, 0),
                3 => (MOUSEEVENTF_MIDDLEUP, 0),
                4 => (MOUSEEVENTF_XUP, XBUTTON1 as u32),
                _ => (MOUSEEVENTF_XUP, XBUTTON2 as u32),
            };
            self.send_mouse(0, 0, data, flag)?;
        }
        Ok(())
    }

    fn send_mouse(&self, dx: i32, dy: i32, data: u32, flags: MOUSE_EVENT_FLAGS) -> Result<()> {
        if self.dry_run {
            return Ok(());
        }
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: data,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        send(input)
    }

    fn send_key(&self, scan_code: u16, flags: KEYBD_EVENT_FLAGS) -> Result<()> {
        if self.dry_run {
            return Ok(());
        }
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: scan_code,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        send(input)
    }
}

fn send(input: INPUT) -> Result<()> {
    let sent = unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
    if sent != 1 {
        bail!("SendInput rejected the event (target may be an elevated/non-interactive window)");
    }
    Ok(())
}
