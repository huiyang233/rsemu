use crate::PinMapping;
use rsemu_core::GpioListener;

pub struct Led {
    id: String,
    pin: PinMapping,
    active_low: bool,
    level_high: bool,
    last_on: Option<bool>,
    #[allow(clippy::type_complexity)]
    on_change: Option<Box<dyn FnMut(&str, bool) + Send>>,
}

impl std::fmt::Debug for Led {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Led")
            .field("id", &self.id)
            .field("pin", &self.pin)
            .field("active_low", &self.active_low)
            .field("level_high", &self.level_high)
            .field("last_on", &self.last_on)
            .finish()
    }
}

impl Led {
    pub fn new(id: String, pin: PinMapping, active_low: bool) -> Self {
        Self {
            id,
            pin,
            active_low,
            level_high: true,
            last_on: None,
            on_change: None,
        }
    }

    /// Attach a callback invoked when the LED state changes: `callback(id, on)`.
    pub fn with_callback(mut self, cb: Box<dyn FnMut(&str, bool) + Send>) -> Self {
        self.on_change = Some(cb);
        self
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
        if let Some(cb) = &mut self.on_change {
            cb(&self.id, on);
        }
    }
}

impl GpioListener for Led {
    fn pin_changed(&mut self, port: char, pin: u8, high: bool) {
        let port_upper = port.to_ascii_uppercase();
        let wanted_port = self.pin.port.trim().to_ascii_uppercase();
        if port_upper.to_string() == wanted_port && pin == self.pin.pin {
            self.update_level(high);
        }
    }
}
