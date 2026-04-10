use crate::bus::traits::GpioListener;

/// A (port, pin) pair used as a listener registration key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GpioPin {
    pub port: char,
    pub pin: u8,
}

impl GpioPin {
    pub fn new(port: char, pin: u8) -> Self {
        Self {
            port: port.to_ascii_uppercase(),
            pin,
        }
    }
}

/// Entry in the listener list: which pin to watch + the listener.
struct GpioListenerEntry {
    pin: GpioPin,
    listener: Box<dyn GpioListener>,
}

/// GPIO event notifier — dispatches pin-change events to registered listeners.
///
/// The application creates one `GpioNotifier` and registers listeners for
/// specific (port, pin) pairs. When the machine's GPIO registers change
/// (ODR/BSRR writes), the app extracts which pins changed and calls
/// `notify()`.
pub struct GpioNotifier {
    entries: Vec<GpioListenerEntry>,
}

impl GpioNotifier {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Register a listener for a specific pin.
    pub fn register(&mut self, pin: GpioPin, listener: Box<dyn GpioListener>) {
        self.entries.push(GpioListenerEntry { pin, listener });
    }

    /// Notify all listeners watching this pin that its state changed.
    pub fn notify(&mut self, port: char, pin: u8, high: bool) {
        let port = port.to_ascii_uppercase();
        for entry in &mut self.entries {
            if entry.pin.port == port && entry.pin.pin == pin {
                entry.listener.pin_changed(port, pin, high);
            }
        }
    }

    /// Batch-notify from a 16-bit pin mask (e.g. BSRR set/reset or ODR value).
    /// For each bit that differs between `old_val` and `new_val`, notify listeners.
    pub fn notify_mask_diff(&mut self, port: char, old_val: u16, new_val: u16) {
        let changed = old_val ^ new_val;
        if changed == 0 {
            return;
        }
        let port_upper = port.to_ascii_uppercase();
        // Only iterate bits that have registered listeners.
        let mut relevant_bits = 0u16;
        for entry in &self.entries {
            if entry.pin.port == port_upper {
                relevant_bits |= 1 << entry.pin.pin;
            }
        }
        let to_check = changed & relevant_bits;
        if to_check == 0 {
            return;
        }
        for pin in 0..16u8 {
            if (to_check >> pin) & 1 != 0 {
                let high = (new_val >> pin) & 1 != 0;
                self.notify(port_upper, pin, high);
            }
        }
    }
}

impl std::fmt::Debug for GpioNotifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpioNotifier")
            .field("listener_count", &self.entries.len())
            .finish()
    }
}
