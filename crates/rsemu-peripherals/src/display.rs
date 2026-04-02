use tracing::info;
use rsemu_core::{MachineBusInterface, MmioWriteEvent};
use std::fs;
use crate::{Peripheral, PinMapping};

#[derive(Debug)]
pub struct St7789 {
    width: u16,
    height: u16,
    framebuffer: Vec<u16>,
    output_dir: String,
    dump_frames: bool,
    frame_id: u32,
    current_cmd: Option<u8>,
    params: Vec<u8>,
    window_x0: u16,
    window_x1: u16,
    window_y0: u16,
    window_y1: u16,
    cursor_x: u16,
    cursor_y: u16,
    pixel_hi: Option<u8>,
    
    // Wiring
    _spi_base: u64,
    cs: PinMapping,
    dc: PinMapping,
    _res: Option<PinMapping>,

    cs_state: u8,
    dc_state: u8,

    ramwr_pixels_written: u32,
    latest_frame_argb: Option<Vec<u32>>,
    preview_argb: Vec<u32>,
    preview_enabled: bool,
}

impl St7789 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        width: u16,
        height: u16,
        spi_base: u64,
        cs: PinMapping,
        dc: PinMapping,
        res: Option<PinMapping>,
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
            params: Vec::new(),
            window_x0: 0,
            window_x1: width.saturating_sub(1),
            window_y0: 0,
            window_y1: height.saturating_sub(1),
            cursor_x: 0,
            cursor_y: 0,
            pixel_hi: None,
            _spi_base: spi_base,
            cs,
            dc,
            _res: res,
            cs_state: 1,
            dc_state: 0,
            ramwr_pixels_written: 0,
            latest_frame_argb: None,
            preview_argb: if preview_enabled {
                vec![0; usize::from(width) * usize::from(height)]
            } else {
                Vec::new()
            },
            preview_enabled,
        }
    }

    pub fn latest_frame(&mut self) -> Option<Vec<u32>> {
        self.latest_frame_argb.take()
    }

    fn on_command(&mut self, cmd: u8) {
        self.current_cmd = Some(cmd);
        self.params.clear();
        self.pixel_hi = None;
        if cmd == 0x2C {
            self.cursor_x = self.window_x0;
            self.cursor_y = self.window_y0;
            self.ramwr_pixels_written = 0;
        }
    }

    fn on_data(&mut self, byte: u8) {
        match self.current_cmd {
            Some(0x2A) => {
                self.params.push(byte);
                if self.params.len() == 4 {
                    self.window_x0 = u16::from_be_bytes([self.params[0], self.params[1]]);
                    self.window_x1 = u16::from_be_bytes([self.params[2], self.params[3]]);
                    info!("st7789.window.x = {}..{}", self.window_x0, self.window_x1);
                }
            }
            Some(0x2B) => {
                self.params.push(byte);
                if self.params.len() == 4 {
                    self.window_y0 = u16::from_be_bytes([self.params[0], self.params[1]]);
                    self.window_y1 = u16::from_be_bytes([self.params[2], self.params[3]]);
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
                self.preview_argb[idx] = 0xFF00_0000 | ((u32::from(rgb[0])) << 16) | ((u32::from(rgb[1])) << 8) | u32::from(rgb[2]);
            }
        }

        self.ramwr_pixels_written = self.ramwr_pixels_written.saturating_add(1);
        
        // Update preview periodically for real-time feel
        if self.preview_enabled && self.ramwr_pixels_written.is_multiple_of(1024) {
            self.latest_frame_argb = Some(self.preview_argb.clone());
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
            self.latest_frame_argb = Some(self.preview_argb.clone());
        }
        if self.dump_frames {
            let path = format!("{}/frame_{:04}.bin", self.output_dir, self.frame_id);
            let _ = fs::write(path, unsafe {
                std::slice::from_raw_parts(
                    self.framebuffer.as_ptr() as *const u8,
                    self.framebuffer.len() * 2,
                )
            });
        }
        self.frame_id = self.frame_id.wrapping_add(1);
    }
}

impl Peripheral for St7789 {
    fn name(&self) -> &str {
        "ST7789"
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn on_mmio_write(&mut self, _machine: &dyn MachineBusInterface, event: &MmioWriteEvent) {
        // Track GPIO state for CS/DC
        if event.peripheral.ends_with(&self.cs.port) || event.peripheral == self.cs.port {
            if event.register.eq_ignore_ascii_case("ODR") {
                self.cs_state = ((event.value >> self.cs.pin) & 1) as u8;
            } else if event.register.eq_ignore_ascii_case("BSRR") {
                let set = (event.value >> self.cs.pin) & 1;
                let reset = (event.value >> (self.cs.pin + 16)) & 1;
                if reset != 0 {
                    self.cs_state = 0;
                } else if set != 0 {
                    self.cs_state = 1;
                }
            }
        }
        if event.peripheral.ends_with(&self.dc.port) || event.peripheral == self.dc.port {
            if event.register.eq_ignore_ascii_case("ODR") {
                self.dc_state = ((event.value >> self.dc.pin) & 1) as u8;
            } else if event.register.eq_ignore_ascii_case("BSRR") {
                let set = (event.value >> self.dc.pin) & 1;
                let reset = (event.value >> (self.dc.pin + 16)) & 1;
                if reset != 0 {
                    self.dc_state = 0;
                } else if set != 0 {
                    self.dc_state = 1;
                }
            }
        }

        // Process SPI data
        if event.peripheral.starts_with("SPI") && event.register.eq_ignore_ascii_case("DR") {
            let cs = self.cs_state;
            let dc = self.dc_state;
            let byte = (event.value & 0xFF) as u8;

            if cs != 0 {
                return;
            }

            // Check Data/Command (DC)
            let dc_is_data = dc != 0;

            if dc_is_data {
                self.on_data(byte);
            } else {
                info!("st7789.cmd_write 0x{:02x}", byte);
                self.on_command(byte);
            }
        }
    }
}

fn rgb565_to_rgb888(p: u16) -> [u8; 3] {
    let r = (((p >> 11) & 0x1F) as u8) << 3;
    let g = (((p >> 5) & 0x3F) as u8) << 2;
    let b = ((p & 0x1F) as u8) << 3;
    [r | (r >> 5), g | (g >> 6), b | (b >> 5)]
}
