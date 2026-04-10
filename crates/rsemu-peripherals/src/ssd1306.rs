use rsemu_core::{FrameUpdate, I2cSlave};

// ---------------------------------------------------------------------------
// SSD1306 Core — single source of truth for all display state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AddressingMode {
    Horizontal,
    Vertical,
    Page,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamKind {
    Command,
    Data,
}

#[derive(Debug)]
pub struct Ssd1306Core {
    pub width: u16,
    pub height: u16,

    pub(crate) addressing_mode: AddressingMode,
    pub page_start: u8,
    pub page_end: u8,
    pub col_start: u8,
    pub col_end: u8,
    pub page: u8,
    pub col: u8,

    pub(crate) stream_kind: Option<StreamKind>,
    pub pending_cmd: Option<u8>,
    pub pending_left: u8,
    pub pending_args: [u8; 2],
    pub pending_filled: u8,

    pub gddram: Vec<u8>,
    pub preview_argb: Vec<u32>,
    pub latest_frame_argb: Option<Vec<u32>>,
    pub data_bytes_written: u32,
}

impl Ssd1306Core {
    pub fn new(width: u16, height: u16) -> Self {
        let pages = (height.saturating_add(7) / 8).max(1) as u8;
        let fb_len = usize::from(width) * usize::from(pages);
        let px_len = usize::from(width) * usize::from(height);
        let col_end = width.saturating_sub(1).min(u16::from(u8::MAX)) as u8;
        let page_end = pages.saturating_sub(1);
        Self {
            width,
            height,
            addressing_mode: AddressingMode::Page,
            page_start: 0,
            page_end,
            col_start: 0,
            col_end,
            page: 0,
            col: 0,
            stream_kind: None,
            pending_cmd: None,
            pending_left: 0,
            pending_args: [0; 2],
            pending_filled: 0,
            gddram: vec![0; fb_len],
            preview_argb: vec![0xFF00_0000; px_len],
            latest_frame_argb: None,
            data_bytes_written: 0,
        }
    }

    pub fn latest_frame(&mut self) -> Option<Vec<u32>> {
        self.latest_frame_argb.take()
    }

    fn command_arg_count(cmd: u8) -> u8 {
        match cmd {
            0x20 => 1,
            0x21 => 2,
            0x22 => 2,
            0x81 => 1,
            0x8D => 1,
            0xA8 => 1,
            0xD3 => 1,
            0xD5 => 1,
            0xD9 => 1,
            0xDA => 1,
            0xDB => 1,
            _ => 0,
        }
    }

    pub fn write_command(&mut self, byte: u8) {
        if self.pending_left > 0 {
            let idx = usize::from(self.pending_filled.min(1));
            self.pending_args[idx] = byte;
            self.pending_filled = self.pending_filled.saturating_add(1);
            self.pending_left = self.pending_left.saturating_sub(1);
            if self.pending_left == 0 {
                self.apply_pending_command();
            }
            return;
        }

        match byte {
            0x00..=0x0F => {
                self.col = (self.col & 0xF0) | (byte & 0x0F);
                self.col = self.col.min(self.col_end);
                return;
            }
            0x10..=0x1F => {
                self.col = (self.col & 0x0F) | ((byte & 0x0F) << 4);
                self.col = self.col.min(self.col_end);
                return;
            }
            0xB0..=0xB7 => {
                self.page = (byte & 0x0F).min(self.page_end);
                return;
            }
            _ => {}
        }

        let argc = Self::command_arg_count(byte);
        if argc > 0 {
            self.pending_cmd = Some(byte);
            self.pending_left = argc;
            self.pending_filled = 0;
        }
    }

    fn apply_pending_command(&mut self) {
        let Some(cmd) = self.pending_cmd.take() else {
            return;
        };
        let a0 = self.pending_args[0];
        let a1 = self.pending_args[1];
        self.pending_left = 0;
        self.pending_filled = 0;

        match cmd {
            0x20 => {
                self.addressing_mode = match a0 & 0x03 {
                    0x00 => AddressingMode::Horizontal,
                    0x01 => AddressingMode::Vertical,
                    _ => AddressingMode::Page,
                };
            }
            0x21 => {
                self.col_start = a0.min(self.col_end);
                self.col_end = a1.min(self.width.saturating_sub(1) as u8).max(self.col_start);
                self.col = self.col_start;
            }
            0x22 => {
                self.page_start = a0.min(self.page_end);
                self.page_end = a1.min(self.page_end).max(self.page_start);
                self.page = self.page_start;
            }
            _ => {}
        }
    }

    pub fn write_data(&mut self, byte: u8) {
        let page = self.page.min(self.page_end);
        let col = self.col.min(self.col_end);
        let idx = usize::from(page) * usize::from(self.width) + usize::from(col);
        if idx < self.gddram.len() {
            self.gddram[idx] = byte;
            self.paint_byte(page, col, byte);
            self.data_bytes_written = self.data_bytes_written.saturating_add(1);
            if self.data_bytes_written.is_multiple_of(128) {
                self.snapshot_if_empty();
            }
        }
        self.advance_cursor();
    }

    fn paint_byte(&mut self, page: u8, col: u8, byte: u8) {
        let x = usize::from(col);
        for bit in 0..8u8 {
            let y = usize::from(page) * 8 + usize::from(bit);
            if y >= usize::from(self.height) || x >= usize::from(self.width) {
                continue;
            }
            let on = ((byte >> bit) & 1) != 0;
            let px = y * usize::from(self.width) + x;
            self.preview_argb[px] = if on { 0xFFFF_FFFF } else { 0xFF00_0000 };
        }
    }

    fn advance_cursor(&mut self) {
        match self.addressing_mode {
            AddressingMode::Page => {
                if self.col < self.col_end {
                    self.col = self.col.saturating_add(1);
                } else {
                    self.col = self.col_start;
                }
            }
            AddressingMode::Horizontal => {
                if self.col < self.col_end {
                    self.col = self.col.saturating_add(1);
                } else {
                    self.col = self.col_start;
                    if self.page < self.page_end {
                        self.page = self.page.saturating_add(1);
                    } else {
                        self.page = self.page_start;
                    }
                }
            }
            AddressingMode::Vertical => {
                if self.page < self.page_end {
                    self.page = self.page.saturating_add(1);
                } else {
                    self.page = self.page_start;
                    if self.col < self.col_end {
                        self.col = self.col.saturating_add(1);
                    } else {
                        self.col = self.col_start;
                    }
                }
            }
        }
    }

    fn snapshot_if_empty(&mut self) {
        if self.latest_frame_argb.is_none() {
            self.latest_frame_argb = Some(self.preview_argb.clone());
        }
    }
}

// ---------------------------------------------------------------------------
// Ssd1306I2c — top-level struct with I2cSlave + BusAttach + legacy Peripheral
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct Ssd1306I2c {
    core: Ssd1306Core,
    address: u8,
}

impl Ssd1306I2c {
    pub fn new(width: u16, height: u16, _i2c_peripheral: String, address: u8) -> Self {
        Self {
            core: Ssd1306Core::new(width, height),
            address: address & 0x7f,
        }
    }

    pub fn width(&self) -> u16 {
        self.core.width
    }

    pub fn height(&self) -> u16 {
        self.core.height
    }

    pub fn latest_frame(&mut self) -> Option<Vec<u32>> {
        self.core.latest_frame()
    }
}

// ---------------------------------------------------------------------------
// I2cSlave implementation
// ---------------------------------------------------------------------------

impl I2cSlave for Ssd1306I2c {
    fn address(&mut self, addr7: u8, read: bool) -> bool {
        if addr7 == self.address && !read {
            self.core.stream_kind = None;
            self.core.pending_cmd = None;
            self.core.pending_left = 0;
            self.core.pending_filled = 0;
            true
        } else {
            false
        }
    }

    fn write_byte(&mut self, data: u8) {
        // First byte after address match is the control byte
        if self.core.stream_kind.is_none() {
            self.core.stream_kind = if (data & 0x40) != 0 {
                Some(StreamKind::Data)
            } else {
                Some(StreamKind::Command)
            };
            self.core.pending_cmd = None;
            self.core.pending_left = 0;
            self.core.pending_filled = 0;
            return;
        }

        match self.core.stream_kind {
            Some(StreamKind::Command) => self.core.write_command(data),
            Some(StreamKind::Data) => self.core.write_data(data),
            None => {}
        }
    }

    fn read_byte(&mut self) -> u8 {
        0 // SSD1306 is write-only in typical I2C usage
    }

    fn stop(&mut self) {
        self.core.snapshot_if_empty();
        self.core.stream_kind = None;
        self.core.pending_cmd = None;
        self.core.pending_left = 0;
        self.core.pending_filled = 0;
    }

    fn reset(&mut self) {
        self.core.stream_kind = None;
        self.core.pending_cmd = None;
        self.core.pending_left = 0;
    }

    fn poll_frame(&mut self) -> Option<FrameUpdate> {
        self.core.latest_frame().map(|pixels| FrameUpdate {
            width: self.core.width,
            height: self.core.height,
            pixels,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssd1306_i2c_slave_trait() {
        let mut d = Ssd1306I2c::new(128, 64, "I2C1".into(), 0x3c);

        // Simulate I2cSlave protocol
        assert!(d.address(0x3c, false)); // ACK our address
        d.write_byte(0x00); // control byte: command stream
        d.write_byte(0xB0); // set page 0
        d.write_byte(0x00); // set low col
        d.write_byte(0x10); // set high col
        d.stop();
        let _ = d.latest_frame(); // clear initial snapshot

        // Data transfer
        assert!(d.address(0x3c, false));
        d.write_byte(0x40); // control byte: data stream
        d.write_byte(0b0000_1111);
        d.stop();

        let frame = d.latest_frame().expect("expected a frame snapshot");
        let on_pixels = frame.iter().filter(|&&px| px == 0xFFFF_FFFF).count();
        assert!(on_pixels >= 4, "expected lit pixels from written byte");
    }
}
