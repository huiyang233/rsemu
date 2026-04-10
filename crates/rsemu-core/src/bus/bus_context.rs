use crate::bus::access::AccessWidth;
use crate::bus::fsmc_bus::FsmcBus;
use crate::bus::gpio::{GpioNotifier, GpioPin};
use crate::bus::i2c_bus::I2cBus;
use crate::bus::spi_bus::SpiBus;
use crate::bus::traits::{FrameUpdate, GpioListener};
use crate::execution::gpio_port_letter;
use crate::MmioWriteEvent;

/// Statistics returned by dispatch_mmio().
#[derive(Debug, Clone, Copy, Default)]
pub struct BusContextStats {
    pub display_activity: bool,
    pub mmio_routed: usize,
}

/// A named SPI bus binding with optional GPIO pin mappings for CS/DC.
struct SpiBinding {
    peripheral: String,
    bus: SpiBus,
    cs_pin: Option<GpioPin>,
    dc_pin: Option<GpioPin>,
}

/// A named I2C bus binding.
struct I2cBinding {
    peripheral: String,
    bus: I2cBus,
}

/// A named FSMC bus binding with base address range.
struct FsmcBinding {
    base_addr: u64,
    size: u64,
    bus: FsmcBus,
}

/// Unified bus context — owns all bus-device connections and routes
/// MMIO events generically. App layers call `dispatch_mmio()` and `poll_frames()`.
pub struct BusContext {
    spi_buses: Vec<SpiBinding>,
    i2c_buses: Vec<I2cBinding>,
    fsmc_buses: Vec<FsmcBinding>,
    gpio_notifier: GpioNotifier,
}

impl BusContext {
    pub fn new() -> Self {
        Self {
            spi_buses: Vec::new(),
            i2c_buses: Vec::new(),
            fsmc_buses: Vec::new(),
            gpio_notifier: GpioNotifier::new(),
        }
    }

    /// Register an SPI bus with optional GPIO-controlled CS/DC pins.
    pub fn register_spi(
        &mut self,
        peripheral: String,
        bus: SpiBus,
        cs_pin: Option<GpioPin>,
        dc_pin: Option<GpioPin>,
    ) {
        self.spi_buses.push(SpiBinding {
            peripheral: peripheral.to_ascii_uppercase(),
            bus,
            cs_pin,
            dc_pin,
        });
    }

    /// Register an I2C bus.
    pub fn register_i2c(&mut self, peripheral: String, bus: I2cBus) {
        self.i2c_buses.push(I2cBinding {
            peripheral: peripheral.to_ascii_uppercase(),
            bus,
        });
    }

    /// Register a GPIO-only listener (e.g., LED).
    pub fn register_gpio(&mut self, pin: GpioPin, listener: Box<dyn GpioListener>) {
        self.gpio_notifier.register(pin, listener);
    }

    /// Register an FSMC bus with a base address and range size.
    pub fn register_fsmc(&mut self, base_addr: u64, size: u64, bus: FsmcBus) {
        self.fsmc_buses.push(FsmcBinding { base_addr, size, bus });
    }

    /// Route a single MMIO write event to the appropriate bus devices.
    pub fn dispatch_mmio(&mut self, event: &MmioWriteEvent) -> BusContextStats {
        let mut stats = BusContextStats::default();
        let periph = event.peripheral.to_ascii_uppercase();
        let reg = event.register.to_ascii_uppercase();

        // ── SPI DR writes ────────────────────────────────────────────
        if periph.starts_with("SPI") && reg == "DR" {
            let byte = (event.value & 0xFF) as u8;
            for binding in &mut self.spi_buses {
                if binding.peripheral == periph {
                    binding.bus.on_dr_write(byte);
                    stats.display_activity = true;
                    stats.mmio_routed += 1;
                }
            }
        }

        // ── I2C CR1 writes ───────────────────────────────────────────
        if periph.starts_with("I2C") && reg == "CR1" {
            for binding in &mut self.i2c_buses {
                if binding.peripheral == periph {
                    binding.bus.on_cr1_write(event.value);
                    stats.mmio_routed += 1;
                }
            }
        }

        // ── I2C DR writes ────────────────────────────────────────────
        if periph.starts_with("I2C") && reg == "DR" {
            let byte = (event.value & 0xFF) as u8;
            for binding in &mut self.i2c_buses {
                if binding.peripheral == periph {
                    binding.bus.on_dr_write(byte);
                    stats.display_activity = true;
                    stats.mmio_routed += 1;
                }
            }
        }

        // ── GPIO writes ──────────────────────────────────────────────
        let is_gpio = periph.starts_with("GPIO")
            || (event.peripheral.len() == 1
                && event.peripheral
                    .chars()
                    .next()
                    .unwrap_or('\0')
                    .is_ascii_alphabetic());
        if is_gpio {
            let port = gpio_port_letter(&event.peripheral).unwrap_or('A');
            self.dispatch_gpio(port, event);
            stats.mmio_routed += 1;
        }

        // ── FSMC memory-mapped writes ──────────────────────────────────
        let addr = event.addr as u64;
        for binding in &mut self.fsmc_buses {
            if addr >= binding.base_addr && addr < binding.base_addr + binding.size {
                let offset = (addr - binding.base_addr) as u32;
                let width = match event.width {
                    1 => AccessWidth::Byte,
                    2 => AccessWidth::HalfWord,
                    _ => AccessWidth::Word,
                };
                binding.bus.on_write(offset, event.value, width);
                stats.display_activity = true;
                stats.mmio_routed += 1;
            }
        }

        stats
    }

    /// Internal: dispatch GPIO ODR/BSRR events.
    fn dispatch_gpio(&mut self, port: char, event: &MmioWriteEvent) {
        let reg = event.register.to_ascii_uppercase();

        if reg == "ODR" {
            let val16 = (event.value & 0xFFFF) as u16;
            self.gpio_notifier.notify_mask_diff(port, 0, val16);
            self.bridge_gpio_to_spi_odr(port, val16);
        } else if reg == "BSRR" {
            let set_mask = (event.value & 0xFFFF) as u16;
            let rst_mask = ((event.value >> 16) & 0xFFFF) as u16;
            for pin in 0..16u8 {
                if (set_mask >> pin) & 1 != 0 {
                    self.gpio_notifier.notify(port, pin, true);
                }
                if (rst_mask >> pin) & 1 != 0 {
                    self.gpio_notifier.notify(port, pin, false);
                }
            }
            self.bridge_gpio_to_spi_bsrr(port, set_mask, rst_mask);
        }
    }

    /// Bridge GPIO ODR changes to SPI device CS/DC pins.
    fn bridge_gpio_to_spi_odr(&mut self, port: char, val16: u16) {
        for binding in &mut self.spi_buses {
            if let Some(ref cs) = binding.cs_pin {
                if cs.port == port {
                    let high = (val16 >> cs.pin) & 1 != 0;
                    binding.bus.on_cs_change(!high); // CS is active-low
                }
            }
            if let Some(ref dc) = binding.dc_pin {
                if dc.port == port {
                    let high = (val16 >> dc.pin) & 1 != 0;
                    binding.bus.on_gpio_pin_changed(port, dc.pin, high);
                }
            }
        }
    }

    /// Bridge GPIO BSRR changes to SPI device CS/DC pins.
    fn bridge_gpio_to_spi_bsrr(&mut self, port: char, set_mask: u16, rst_mask: u16) {
        for binding in &mut self.spi_buses {
            if let Some(ref cs) = binding.cs_pin {
                if cs.port == port {
                    let cs_set = (set_mask >> cs.pin) & 1 != 0;
                    let cs_rst = (rst_mask >> cs.pin) & 1 != 0;
                    // CS active-low: set bit → pin HIGH → not selected; reset → pin LOW → selected
                    if cs_set { binding.bus.on_cs_change(false); }
                    if cs_rst { binding.bus.on_cs_change(true); }
                }
            }
            if let Some(ref dc) = binding.dc_pin {
                if dc.port == port {
                    let dc_set = (set_mask >> dc.pin) & 1 != 0;
                    let dc_rst = (rst_mask >> dc.pin) & 1 != 0;
                    if dc_set { binding.bus.on_gpio_pin_changed(port, dc.pin, true); }
                    if dc_rst { binding.bus.on_gpio_pin_changed(port, dc.pin, false); }
                }
            }
        }
    }

    /// Poll all registered display devices for new frames.
    pub fn poll_frames(&mut self) -> Vec<FrameUpdate> {
        let mut frames = Vec::new();
        for binding in &mut self.spi_buses {
            if let Some(frame) = binding.bus.device_mut().poll_frame() {
                frames.push(frame);
            }
        }
        for binding in &mut self.i2c_buses {
            if let Some(frame) = binding.bus.device_mut().poll_frame() {
                frames.push(frame);
            }
        }
        for binding in &mut self.fsmc_buses {
            if let Some(frame) = binding.bus.device_mut().poll_frame() {
                frames.push(frame);
            }
        }
        frames
    }

    /// Check if any SPI, I2C, or FSMC buses are registered (used for display activity detection).
    pub fn has_bus_devices(&self) -> bool {
        !self.spi_buses.is_empty() || !self.i2c_buses.is_empty() || !self.fsmc_buses.is_empty()
    }
}

impl std::fmt::Debug for BusContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BusContext")
            .field("spi_buses", &self.spi_buses.len())
            .field("i2c_buses", &self.i2c_buses.len())
            .field("fsmc_buses", &self.fsmc_buses.len())
            .finish()
    }
}
