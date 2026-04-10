use crate::bus::irq::IrqCallback;
use crate::bus::traits::SpiSlave;

/// SPI bus controller — sits between the machine's register layer and the SPI slave device.
///
/// The machine calls `on_dr_write()` when firmware writes to SPI_DR.
/// SpiBus forwards the byte to the slave via `SpiSlave::transfer()` and latches the MISO response.
/// When firmware reads SPI_DR, the machine calls `on_dr_read()` which returns the latched value.
pub struct SpiBus {
    device: Box<dyn SpiSlave>,
    rx_latch: Option<u8>,
    irq_callback: Option<IrqCallback>,
    irq_number: Option<u8>,
}

impl SpiBus {
    pub fn new(device: Box<dyn SpiSlave>) -> Self {
        Self {
            device,
            rx_latch: None,
            irq_callback: None,
            irq_number: None,
        }
    }

    /// Set the IRQ number and callback for this SPI bus.
    pub fn set_irq(&mut self, irq_number: u8, callback: IrqCallback) {
        self.irq_number = Some(irq_number);
        self.irq_callback = Some(callback);
    }

    /// Called when firmware writes to SPI_DR.
    /// Sends `data` to the slave device, latches the MISO response.
    /// Returns the MISO byte (for the machine to store back into DR if needed).
    pub fn on_dr_write(&mut self, data: u8) -> u8 {
        let miso = self.device.transfer(data);
        self.rx_latch = Some(miso);
        self.fire_irq();
        miso
    }

    /// Called when firmware reads from SPI_DR.
    /// Consumes the latched MISO value (does NOT call the device again).
    pub fn on_dr_read(&mut self) -> Option<u8> {
        self.rx_latch.take()
    }

    /// Called when CS pin changes.
    pub fn on_cs_change(&mut self, active: bool) {
        self.device.chip_select(active);
    }

    /// Called on device reset.
    pub fn reset(&mut self) {
        self.device.reset();
        self.rx_latch = None;
    }

    /// Access the underlying device (for GPIO listener passthrough, etc.).
    pub fn device_mut(&mut self) -> &mut dyn SpiSlave {
        self.device.as_mut()
    }

    fn fire_irq(&mut self) {
        if let (Some(cb), Some(irq)) = (&self.irq_callback, self.irq_number) {
            cb(irq);
        }
    }
}
