use crate::config::AppSettings;
use crate::types::ControlIntent;
use anyhow::Context;
use tracing::debug;

pub struct SafeInputDriver {
    settings: AppSettings,
    move_remainder_x: f32,
    move_remainder_y: f32,
}

impl SafeInputDriver {
    pub fn new(settings: AppSettings) -> Self {
        Self {
            settings,
            move_remainder_x: 0.0,
            move_remainder_y: 0.0,
        }
    }

    pub fn update_settings(&mut self, settings: AppSettings) {
        self.settings = settings;
    }

    pub fn apply(&mut self, intent: &ControlIntent) -> anyhow::Result<()> {
        if !self.settings.input_injection_enabled {
            debug!(?intent, "input injection disabled; intent ignored");
            return Ok(());
        }

        if self.settings.safe_mode {
            return self.apply_safe_mode(intent);
        }

        self.apply_full(intent)
    }

    fn apply_safe_mode(&mut self, intent: &ControlIntent) -> anyhow::Result<()> {
        match intent {
            ControlIntent::MoveDelta { dx, dy } if self.settings.allow_safe_mode_movement => {
                let (px_dx, px_dy) = self.map_move_delta(*dx, *dy, false);
                if px_dx != 0 || px_dy != 0 {
                    platform::mouse_move_relative(px_dx, px_dy)
                        .context("safe-mode mouse move failed")?;
                }
            }
            _ => {
                debug!(?intent, "safe_mode active; non-move intent suppressed");
            }
        }
        Ok(())
    }

    fn apply_full(&mut self, intent: &ControlIntent) -> anyhow::Result<()> {
        match intent {
            ControlIntent::MoveDelta { dx, dy } => {
                let (px_dx, px_dy) = self.map_move_delta(*dx, *dy, true);
                if px_dx != 0 || px_dy != 0 {
                    platform::mouse_move_relative(px_dx, px_dy).context("mouse move failed")?;
                }
            }
            ControlIntent::LeftClick => platform::left_click().context("left click failed")?,
            ControlIntent::RightClick => platform::right_click().context("right click failed")?,
            ControlIntent::Scroll { dy } => {
                platform::wheel_scroll(clamp_scaled(*dy, 120.0)).context("scroll failed")?;
            }
            ControlIntent::ControlOn
            | ControlIntent::ControlOff
            | ControlIntent::ClutchOn
            | ControlIntent::ClutchOff
            | ControlIntent::Paused => {}
        }
        Ok(())
    }

    fn map_move_delta(&mut self, dx: f32, dy: f32, full_mode: bool) -> (i32, i32) {
        let speed = (dx * dx + dy * dy).sqrt();
        let (base_scale, accel_scale, cap) = if full_mode {
            (55.0_f32, 380.0_f32, 180_i32)
        } else {
            (18.0_f32, 120.0_f32, 36_i32)
        };
        let gain = base_scale + accel_scale * speed;

        let raw_x = dx * gain + self.move_remainder_x;
        let raw_y = dy * gain + self.move_remainder_y;

        let out_x = raw_x.round() as i32;
        let out_y = raw_y.round() as i32;
        let clamped_x = out_x.clamp(-cap, cap);
        let clamped_y = out_y.clamp(-cap, cap);

        self.move_remainder_x = if clamped_x == out_x {
            raw_x - out_x as f32
        } else {
            0.0
        };
        self.move_remainder_y = if clamped_y == out_y {
            raw_y - out_y as f32
        } else {
            0.0
        };

        (clamped_x, clamped_y)
    }
}

fn clamp_scaled(value: f32, scale: f32) -> i32 {
    let raw = (value * scale).round() as i32;
    raw.clamp(-120, 120)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulates_subpixel_motion() {
        let mut driver = SafeInputDriver::new(AppSettings::default());
        let first = driver.map_move_delta(0.003, 0.0, true);
        let second = driver.map_move_delta(0.003, 0.0, true);
        let third = driver.map_move_delta(0.003, 0.0, true);
        assert_eq!(first.0, 0);
        assert!(second.0 >= 0);
        assert!(third.0 >= second.0);
    }

    #[test]
    fn faster_swipe_produces_larger_delta() {
        let mut slow_driver = SafeInputDriver::new(AppSettings::default());
        let mut fast_driver = SafeInputDriver::new(AppSettings::default());
        let slow = slow_driver.map_move_delta(0.012, 0.0, true).0;
        let fast = fast_driver.map_move_delta(0.045, 0.0, true).0;
        assert!(fast.abs() > slow.abs());
    }
}

#[cfg(windows)]
mod platform {
    use anyhow::anyhow;
    use std::mem::size_of;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
        MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL,
        MOUSEINPUT,
    };

    fn send_mouse(input: MOUSEINPUT) -> anyhow::Result<()> {
        let packet = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 { mi: input },
        };
        let sent = unsafe { SendInput(1, &packet, size_of::<INPUT>() as i32) };
        if sent != 1 {
            return Err(anyhow!("SendInput failed"));
        }
        Ok(())
    }

    pub fn mouse_move_relative(dx: i32, dy: i32) -> anyhow::Result<()> {
        send_mouse(MOUSEINPUT {
            dx,
            dy,
            mouseData: 0,
            dwFlags: MOUSEEVENTF_MOVE,
            time: 0,
            dwExtraInfo: 0,
        })
    }

    pub fn left_click() -> anyhow::Result<()> {
        send_mouse(MOUSEINPUT {
            dx: 0,
            dy: 0,
            mouseData: 0,
            dwFlags: MOUSEEVENTF_LEFTDOWN,
            time: 0,
            dwExtraInfo: 0,
        })?;
        send_mouse(MOUSEINPUT {
            dx: 0,
            dy: 0,
            mouseData: 0,
            dwFlags: MOUSEEVENTF_LEFTUP,
            time: 0,
            dwExtraInfo: 0,
        })
    }

    pub fn right_click() -> anyhow::Result<()> {
        send_mouse(MOUSEINPUT {
            dx: 0,
            dy: 0,
            mouseData: 0,
            dwFlags: MOUSEEVENTF_RIGHTDOWN,
            time: 0,
            dwExtraInfo: 0,
        })?;
        send_mouse(MOUSEINPUT {
            dx: 0,
            dy: 0,
            mouseData: 0,
            dwFlags: MOUSEEVENTF_RIGHTUP,
            time: 0,
            dwExtraInfo: 0,
        })
    }

    pub fn wheel_scroll(dy: i32) -> anyhow::Result<()> {
        send_mouse(MOUSEINPUT {
            dx: 0,
            dy: 0,
            mouseData: dy as u32,
            dwFlags: MOUSEEVENTF_WHEEL,
            time: 0,
            dwExtraInfo: 0,
        })
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn mouse_move_relative(_dx: i32, _dy: i32) -> anyhow::Result<()> {
        Ok(())
    }
    pub fn left_click() -> anyhow::Result<()> {
        Ok(())
    }
    pub fn right_click() -> anyhow::Result<()> {
        Ok(())
    }
    pub fn wheel_scroll(_dy: i32) -> anyhow::Result<()> {
        Ok(())
    }
}
