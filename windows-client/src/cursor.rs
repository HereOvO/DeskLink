use crate::desktop::VirtualDesktop;
use desklink_protocol::ScreenEdge;

/// Logical cursor inside the remote-screen rectangle that the Windows host
/// advertises through `EnterRemote { width, height }`.
///
/// The host only ever sees ratios, so the cursor is tracked in that logical
/// space and projected onto this machine's real virtual desktop when the
/// pointer has to be placed.
pub struct RemoteCursor {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    return_edge: Option<ScreenEdge>,
    return_armed: bool,
}

impl RemoteCursor {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            x: width as i32 / 2,
            y: height as i32 / 2,
            width: width.max(1) as i32,
            height: height.max(1) as i32,
            return_edge: None,
            return_armed: false,
        }
    }

    pub fn enter(&mut self, edge: ScreenEdge, ratio: f32, width: u32, height: u32) {
        self.width = width.max(1) as i32;
        self.height = height.max(1) as i32;
        let ratio = ratio.clamp(0.0, 1.0);
        match edge {
            ScreenEdge::Left => {
                self.x = 1;
                self.y = (self.height as f32 * ratio) as i32;
            }
            ScreenEdge::Right => {
                self.x = self.width - 2;
                self.y = (self.height as f32 * ratio) as i32;
            }
            ScreenEdge::Top => {
                self.x = (self.width as f32 * ratio) as i32;
                self.y = 1;
            }
            ScreenEdge::Bottom => {
                self.x = (self.width as f32 * ratio) as i32;
                self.y = self.height - 2;
            }
        }
        self.x = self.x.clamp(0, self.width - 1);
        self.y = self.y.clamp(0, self.height - 1);
        self.return_edge = Some(edge);
        self.return_armed = false;
    }

    pub fn move_by(&mut self, dx: i16, dy: i16) -> Option<(ScreenEdge, f32)> {
        let next_x = self.x.saturating_add(dx as i32);
        let next_y = self.y.saturating_add(dy as i32);
        if let Some(edge) = self.return_edge {
            let inward_distance = match edge {
                ScreenEdge::Left => next_x,
                ScreenEdge::Right => self.width - 1 - next_x,
                ScreenEdge::Top => next_y,
                ScreenEdge::Bottom => self.height - 1 - next_y,
            };
            if inward_distance >= 48 {
                self.return_armed = true;
            }
            let crossed = match edge {
                ScreenEdge::Left => dx < 0 && next_x <= 0,
                ScreenEdge::Right => dx > 0 && next_x >= self.width - 1,
                ScreenEdge::Top => dy < 0 && next_y <= 0,
                ScreenEdge::Bottom => dy > 0 && next_y >= self.height - 1,
            };
            if self.return_armed && crossed {
                let ratio = match edge {
                    ScreenEdge::Left | ScreenEdge::Right => self.y as f32 / self.height as f32,
                    ScreenEdge::Top | ScreenEdge::Bottom => self.x as f32 / self.width as f32,
                };
                return Some((edge, ratio.clamp(0.0, 1.0)));
            }
        }
        self.x = next_x.clamp(0, self.width - 1);
        self.y = next_y.clamp(0, self.height - 1);
        None
    }

    pub fn snap_to_edge(&mut self, edge: ScreenEdge) {
        match edge {
            ScreenEdge::Left => self.x = 0,
            ScreenEdge::Right => self.x = self.width - 1,
            ScreenEdge::Top => self.y = 0,
            ScreenEdge::Bottom => self.y = self.height - 1,
        }
    }

    /// Projects the logical cursor onto the real virtual desktop. When the
    /// configured remote size matches this desktop the mapping is the identity.
    pub fn physical_position(&self, desktop: &VirtualDesktop) -> (i32, i32) {
        (
            desktop.x + map_axis(self.x, self.width, desktop.width),
            desktop.y + map_axis(self.y, self.height, desktop.height),
        )
    }
}

fn map_axis(value: i32, logical_length: i32, physical_length: i32) -> i32 {
    let logical_length = logical_length.max(1);
    let physical_length = physical_length.max(1);
    if logical_length <= 1 || physical_length <= 1 {
        return 0;
    }
    let scaled = value.clamp(0, logical_length - 1) as f64 * (physical_length - 1) as f64
        / (logical_length - 1) as f64;
    scaled.round().clamp(0.0, (physical_length - 1) as f64) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_when_sizes_match() {
        let cursor = RemoteCursor {
            x: 960,
            y: 540,
            width: 1920,
            height: 1080,
            return_edge: None,
            return_armed: false,
        };
        let desktop = VirtualDesktop {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        assert_eq!(cursor.physical_position(&desktop), (960, 540));
    }

    #[test]
    fn scales_and_offsets_for_other_resolution() {
        let cursor = RemoteCursor {
            x: 1920 - 1,
            y: 540,
            width: 1920,
            height: 1080,
            return_edge: None,
            return_armed: false,
        };
        let desktop = VirtualDesktop {
            x: -1080,
            y: 0,
            width: 2560,
            height: 1440,
        };
        assert_eq!(cursor.physical_position(&desktop), (-1080 + 2559, 720));
    }

    #[test]
    fn returns_to_host_once_the_cursor_travels_back() {
        let mut cursor = RemoteCursor::new(1920, 1080);
        cursor.enter(ScreenEdge::Right, 0.5, 1920, 1080);
        assert_eq!(cursor.move_by(-100, 0), None);
        let (edge, ratio) = cursor.move_by(200, 0).expect("edge return");
        assert_eq!(edge, ScreenEdge::Right);
        assert!((ratio - 0.5).abs() < 0.01, "ratio was {ratio}");
    }
}
