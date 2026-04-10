use rsemu_core::{
    AccessWidth, FrameUpdate, GpioListener, ParallelDevice, SpiSlave,
};
use tracing::info;
use std::fs;
use crate::PinMapping;

// ---------------------------------------------------------------------------
// St7789Core — single source of truth for all display state
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct St7789Core {
    pub width: u16,
    pub height: u16,
    pub framebuffer: Vec<u16>,
    pub output_dir: String,
    pub dump_frames: bool,
    pub frame_id: u32,
    pub current_cmd: Option<u8>,
    pub params_buf: [u8; 4],
    pub params_len: u8,
    pub window_x0: u16,
    pub window_x1: u16,
    pub window_y0: u16,
    pub window_y1: u16,
    pub cursor_x: u16,
    pub cursor_y: u16,
    pub pixel_hi: Option<u8>,
    pub ramwr_pixels_written: u32,
    pub latest_frame_rgba: Option<Vec<u32>>,
    pub preview_rgba: Vec<u32>,
    pub preview_enabled: bool,
    pub dc: bool,
    pub cs_active: bool,
}

impl St7789Core {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        width: u16,
        height: u16,
        output_dir: String,
        dump_frames: bool,
        preview_enabled: bool,
    ) -> Self {
        if dump_frames {
            let _ = fs::create_dir_all(&output_dir);
        }
        Self {
            width,
            height,
            framebuffer: vec![0; usize::from(width) * usize::from(height)],
            output_dir,
            dump_frames,
            frame_id: 0,
            current_cmd: None,
            params_buf: [0; 4],
            params_len: 0,
            window_x0: 0,
            window_x1: width.saturating_sub(1),
            window_y0: 0,
            window_y1: height.saturating_sub(1),
            cursor_x: 0,
            cursor_y: 0,
            pixel_hi: None,
            dc: false,
            cs_active: false,
            ramwr_pixels_written: 0,
            latest_frame_rgba: None,
            preview_rgba: if preview_enabled {
                vec![0; usize::from(width) * usize::from(height)]
            } else {
                Vec::new()
            },
            preview_enabled,
        }
    }

    pub fn write_command(&mut self, cmd: u8) {
        self.current_cmd = Some(cmd);
        self.params_len = 0;
        self.pixel_hi = None;
        if cmd == 0x2C {
            self.cursor_x = self.window_x0;
            self.cursor_y = self.window_y0;
            self.ramwr_pixels_written = 0;
        }
    }

    pub fn write_data(&mut self, byte: u8) {
        match self.current_cmd {
            Some(0x2A) => {
                if (self.params_len as usize) < self.params_buf.len() {
                    self.params_buf[self.params_len as usize] = byte;
                    self.params_len += 1;
                }
                if self.params_len == 4 {
                    self.window_x0 = u16::from_be_bytes([self.params_buf[0], self.params_buf[1]]);
                    self.window_x1 = u16::from_be_bytes([self.params_buf[2], self.params_buf[3]]);
                    info!("st7789.window.x = {}..{}", self.window_x0, self.window_x1);
                }
            }
            Some(0x2B) => {
                if (self.params_len as usize) < self.params_buf.len() {
                    self.params_buf[self.params_len as usize] = byte;
                    self.params_len += 1;
                }
                if self.params_len == 4 {
                    self.window_y0 = u16::from_be_bytes([self.params_buf[0], self.params_buf[1]]);
                    self.window_y1 = u16::from_be_bytes([self.params_buf[2], self.params_buf[3]]);
                    info!("st7789.window.y = {}..{}", self.window_y0, self.window_y1);
                }
            }
            Some(0x2C) => {
                self.on_ramwr_data(byte);
            }
            _ => {}
        }
    }

    fn on_ramwr_data(&mut self, byte: u8) {
        if self.pixel_hi.is_none() {
            self.pixel_hi = Some(byte);
            return;
        }

        let hi = self.pixel_hi.take().unwrap_or(0);
        let pixel = u16::from_be_bytes([hi, byte]);
        let x = self.cursor_x.min(self.width.saturating_sub(1));
        let y = self.cursor_y.min(self.height.saturating_sub(1));
        let idx = usize::from(y) * usize::from(self.width) + usize::from(x);

        if idx < self.framebuffer.len() {
            self.framebuffer[idx] = pixel;
            if self.preview_enabled {
                let rgb = rgb565_to_rgb888(pixel);
                // Store as u32 whose little-endian bytes are [R, G, B, A] for JS ImageData
                self.preview_rgba[idx] =
                    u32::from(rgb[0]) | (u32::from(rgb[1]) << 8) | (u32::from(rgb[2]) << 16) | 0xFF00_0000;
            }
        }

        self.ramwr_pixels_written = self.ramwr_pixels_written.saturating_add(1);

        if self.preview_enabled && self.ramwr_pixels_written.is_multiple_of(1024) {
            if self.latest_frame_rgba.is_none() {
                let len = self.preview_rgba.len();
                self.latest_frame_rgba = Some(std::mem::replace(&mut self.preview_rgba, vec![0xFF00_0000; len]));
            }
        }

        if self.cursor_x < self.window_x1 {
            self.cursor_x = self.cursor_x.saturating_add(1);
        } else {
            self.cursor_x = self.window_x0;
            if self.cursor_y < self.window_y1 {
                self.cursor_y = self.cursor_y.saturating_add(1);
            } else {
                self.cursor_y = self.window_y0;
                self.emit_frame();
            }
        }
    }

    fn emit_frame(&mut self) {
        if self.preview_enabled {
            let len = self.preview_rgba.len();
            self.latest_frame_rgba = Some(std::mem::replace(&mut self.preview_rgba, vec![0xFF00_0000; len]));
        }
        if self.dump_frames {
            let path = format!("{}/frame_{:04}.bin", self.output_dir, self.frame_id);
            let _ = fs::write(
                path,
                unsafe {
                    std::slice::from_raw_parts(
                        self.framebuffer.as_ptr() as *const u8,
                        self.framebuffer.len() * 2,
                    )
                },
            );
        }
        self.frame_id = self.frame_id.wrapping_add(1);
    }

    pub fn latest_frame(&mut self) -> Option<Vec<u32>> {
        self.latest_frame_rgba.take()
    }
}

// ---------------------------------------------------------------------------
// St7789 — top-level struct implementing BusAttach + legacy Peripheral
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct St7789 {
    core: St7789Core,

    // Pin wiring (used by legacy Peripheral::on_mmio_write for GPIO tracking)
    cs: PinMapping,
    dc: PinMapping,
    _res: Option<PinMapping>,
}

impl St7789 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        width: u16,
        height: u16,
        _spi_base: u64,
        cs: PinMapping,
        dc: PinMapping,
        res: Option<PinMapping>,
        output_dir: String,
        dump_frames: bool,
        preview_enabled: bool,
    ) -> Self {
        Self {
            core: St7789Core::new(width, height, output_dir, dump_frames, preview_enabled),
            cs,
            dc,
            _res: res,
        }
    }

    pub fn latest_frame(&mut self) -> Option<Vec<u32>> {
        self.core.latest_frame()
    }

    pub fn width(&self) -> u16 {
        self.core.width
    }

    pub fn height(&self) -> u16 {
        self.core.height
    }
}

// ---------------------------------------------------------------------------
// SpiSlave implementation — uses core's dc state to route command vs data
// ---------------------------------------------------------------------------

impl SpiSlave for St7789 {
    fn transfer(&mut self, mosi: u8) -> u8 {
        if !self.core.cs_active {
            return 0;
        }
        if self.core.dc {
            self.core.write_data(mosi);
        } else {
            info!("st7789.cmd_write 0x{:02x}", mosi);
            self.core.write_command(mosi);
        }
        0 // ST7789 does not return data on MISO
    }

    fn chip_select(&mut self, active: bool) {
        self.core.cs_active = active;
    }

    fn reset(&mut self) {
        // Keep framebuffer and dimensions, reset protocol state
        self.core.current_cmd = None;
        self.core.params_len = 0;
        self.core.pixel_hi = None;
    }

    fn poll_frame(&mut self) -> Option<FrameUpdate> {
        self.core.latest_frame().map(|pixels| FrameUpdate {
            width: self.core.width,
            height: self.core.height,
            pixels,
        })
    }

    fn gpio_pin_changed(&mut self, port: char, pin: u8, high: bool) {
        let pu = port.to_ascii_uppercase();
        if pu == self.cs.port.chars().next().unwrap_or('\0').to_ascii_uppercase() && pin == self.cs.pin {
            self.core.cs_active = !high; // CS is active-low: pin LOW = selected
        }
        if pu == self.dc.port.chars().next().unwrap_or('\0').to_ascii_uppercase() && pin == self.dc.pin {
            self.core.dc = high; // DC is active-high: pin HIGH = data mode
        }
    }
}

// ---------------------------------------------------------------------------
// GpioListener implementation — tracks DC and CS pins
// ---------------------------------------------------------------------------

impl GpioListener for St7789 {
    fn pin_changed(&mut self, port: char, pin: u8, high: bool) {
        let pu = port.to_ascii_uppercase();
        if pu == self.cs.port.chars().next().unwrap_or('\0').to_ascii_uppercase() && pin == self.cs.pin {
            self.core.cs_active = !high; // CS is active-low: pin LOW = selected
        }
        if pu == self.dc.port.chars().next().unwrap_or('\0').to_ascii_uppercase() && pin == self.dc.pin {
            self.core.dc = high; // DC is active-high: pin HIGH = data mode
        }
    }
}

// ---------------------------------------------------------------------------
// ParallelDevice implementation — for FSMC bus (e.g., ST7789 via FSMC on F407)
// ---------------------------------------------------------------------------

impl ParallelDevice for St7789 {
    fn write(&mut self, addr: u32, data: u32, _width: AccessWidth) {
        // FSMC: address bit 0 encodes RS/DC (0 = command, 1 = data)
        let dc = (addr & 1) == 1;
        let byte = (data & 0xFF) as u8;
        if dc {
            self.core.write_data(byte);
        } else {
            info!("st7789.cmd_write(FSMC) 0x{:02x}", byte);
            self.core.write_command(byte);
        }
    }

    fn read(&mut self, _addr: u32, _width: AccessWidth) -> u32 {
        0 // ST7789 is write-only in typical FSMC LCD usage
    }

    fn reset(&mut self) {
        self.core.current_cmd = None;
        self.core.params_len = 0;
        self.core.pixel_hi = None;
    }

    fn poll_frame(&mut self) -> Option<FrameUpdate> {
        self.core.latest_frame().map(|pixels| FrameUpdate {
            width: self.core.width,
            height: self.core.height,
            pixels,
        })
    }
}

// ---------------------------------------------------------------------------
// ParallelDevice implementation — for FSMC bus (e.g., ST7789 via FSMC on F407)
// ---------------------------------------------------------------------------

fn rgb565_to_rgb888(p: u16) -> [u8; 3] {
    let r = (((p >> 11) & 0x1F) as u8) << 3;
    let g = (((p >> 5) & 0x3F) as u8) << 2;
    let b = ((p & 0x1F) as u8) << 3;
    [r | (r >> 5), g | (g >> 6), b | (b >> 5)]
}
