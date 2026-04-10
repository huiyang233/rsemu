use crate::{Peripheral, PinMapping};
use rsemu_core::{
    BusAttach, DeviceCapabilities, DeviceHandle, GpioListener,
    MachineBusInterface, MmioWriteEvent,
};

#[derive(Debug)]
pub struct Led {
    id: String,
    pin: PinMapping,
    active_low: bool,
    level_high: bool,
    last_on: Option<bool>,
}

impl Led {
    pub fn new(id: String, pin: PinMapping, active_low: bool) -> Self {
        Self {
            id,
            pin,
            active_low,
            level_high: true,
            last_on: None,
        }
    }

    fn on_state(&self) -> bool {
        if self.active_low {
            !self.level_high
        } else {
            self.level_high
        }
    }

    fn pin_name(&self) -> String {
        format!("P{}{}", self.pin.port.to_ascii_uppercase(), self.pin.pin)
    }

    fn update_level(&mut self, high: bool) {
        self.level_high = high;
        let on = self.on_state();
        if self.last_on == Some(on) {
            return;
        }
        self.last_on = Some(on);
        let state = if on { "on" } else { "off" };
        eprintln!("led.{} = {} ({})", self.id, state, self.pin_name());
    }

    pub fn into_device_handle(self) -> DeviceHandle {
        DeviceHandle::new(
            DeviceCapabilities::GPIO_LISTENER,
            Box::new(self),
        )
    }
}

// ---------------------------------------------------------------------------
// GpioListener implementation
// ---------------------------------------------------------------------------

impl GpioListener for Led {
    fn pin_changed(&mut self, port: char, pin: u8, high: bool) {
        let port_upper = port.to_ascii_uppercase();
        let wanted_port = self.pin.port.trim().to_ascii_uppercase();
        if port_upper.to_string() == wanted_port && pin == self.pin.pin {
            self.update_level(high);
        }
    }
}

// ---------------------------------------------------------------------------
// BusAttach implementation
// ---------------------------------------------------------------------------

impl BusAttach for Led {
    fn as_gpio_listener(&mut self) -> Option<&mut dyn GpioListener> {
        Some(self)
    }
}

// ---------------------------------------------------------------------------
// Legacy Peripheral trait
// ---------------------------------------------------------------------------

impl Peripheral for Led {
    fn name(&self) -> &str {
        &self.id
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn on_mmio_write(&mut self, _machine: &dyn MachineBusInterface, event: &MmioWriteEvent) {
        let p = event.peripheral.trim().to_ascii_uppercase();
        let wanted = self.pin.port.trim().to_ascii_uppercase();
        if p != format!("GPIO{wanted}") && p != wanted {
            return;
        }
        if event.register.eq_ignore_ascii_case("ODR") {
            let high = ((event.value >> self.pin.pin) & 1) != 0;
            self.update_level(high);
            return;
        }
        if event.register.eq_ignore_ascii_case("BSRR") {
            let set = ((event.value >> self.pin.pin) & 1) != 0;
            let reset = ((event.value >> (self.pin.pin + 16)) & 1) != 0;
            if set {
                self.update_level(true);
            } else if reset {
                self.update_level(false);
            }
        }
    }
}
