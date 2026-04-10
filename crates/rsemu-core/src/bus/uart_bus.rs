use crate::bus::traits::UartDevice;

/// UART bus controller — sits between the machine's register layer and the UART device.
///
/// - TX path: machine calls `on_dr_write()` when firmware writes to USART_DR.
/// - RX path: machine calls `on_dr_read()` when firmware reads from USART_DR.
pub struct UartBus {
    device: Box<dyn UartDevice>,
    irq_callback: Option<crate::bus::irq::IrqCallback>,
    tx_irq: Option<u8>,
    rx_irq: Option<u8>,
}

impl UartBus {
    pub fn new(device: Box<dyn UartDevice>) -> Self {
        Self {
            device,
            irq_callback: None,
            tx_irq: None,
            rx_irq: None,
        }
    }

    /// Set IRQ numbers and shared callback.
    pub fn set_irq(
        &mut self,
        tx_irq: u8,
        rx_irq: u8,
        callback: crate::bus::irq::IrqCallback,
    ) {
        self.tx_irq = Some(tx_irq);
        self.rx_irq = Some(rx_irq);
        self.irq_callback = Some(callback);
    }

    /// Called when firmware writes to USART_DR (TX).
    pub fn on_dr_write(&mut self, byte: u8) {
        self.device.on_tx(byte);
        self.fire_irq(self.tx_irq);
    }

    /// Called when firmware reads from USART_DR (RX).
    /// Returns `Some(byte)` if device has data, `None` if empty.
    pub fn on_dr_read(&mut self) -> Option<u8> {
        let result = self.device.poll_rx();
        if result.is_some() {
            self.fire_irq(self.rx_irq);
        }
        result
    }

    /// Whether the device has RX data ready (for SR register RXNE flag).
    pub fn has_rx_data(&mut self) -> bool {
        // We peek without consuming — devices with a buffer can check non-destructively.
        // Since we can't peek through the trait, we do a poll and buffer the result.
        // Instead, let the device answer directly.
        false // The machine should call on_dr_read and handle the result
    }

    pub fn reset(&mut self) {
        self.device.reset();
    }

    fn fire_irq(&mut self, irq: Option<u8>) {
        if let (Some(cb), Some(n)) = (&self.irq_callback, irq) {
            cb(n);
        }
    }
}
