use crate::config::AppSettings;
use crate::types::ControlIntent;
use anyhow::Context;
use tracing::debug;

pub struct SafeInputDriver {
    settings: AppSettings,
}

impl SafeInputDriver {
    pub fn new(settings: AppSettings) -> Self {
        Self { settings }
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
                let clamped_dx = clamp_scaled(*dx, 12.0);
                let clamped_dy = clamp_scaled(*dy, 12.0);
                platform::mouse_move_relative(clamped_dx, clamped_dy)
                    .context("safe-mode mouse move failed")?;
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
                platform::mouse_move_relative(clamp_scaled(*dx, 40.0), clamp_scaled(*dy, 40.0))
                    .context("mouse move failed")?;
            }
            ControlIntent::LeftClick => platform::left_click().context("left click failed")?,
            ControlIntent::RightClick => platform::right_click().context("right click failed")?,
            ControlIntent::Scroll { dy } => {
                platform::wheel_scroll(clamp_scaled(*dy, 120.0)).context("scroll failed")?;
            }
            ControlIntent::ControlOn | ControlIntent::ControlOff | ControlIntent::Paused => {}
        }
        Ok(())
    }
}

fn clamp_scaled(value: f32, scale: f32) -> i32 {
    let raw = (value * scale).round() as i32;
    raw.clamp(-120, 120)
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
