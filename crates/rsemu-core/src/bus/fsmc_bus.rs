use crate::bus::access::AccessWidth;
use crate::bus::traits::ParallelDevice;

/// FSMC bus controller — routes memory-mapped parallel bus reads/writes to a device.
///
/// The FSMC address range on STM32F407 is typically `0x6000_0000 - 0x9FFF_FFFF`.
/// The address's lowest bit often encodes the RS/DC (data/command) signal for LCDs.
///
/// The machine intercepts reads/writes to the FSMC address range and calls
/// `FsmcBus::on_write()` / `FsmcBus::on_read()`.
pub struct FsmcBus {
    device: Box<dyn ParallelDevice>,
    irq_callback: Option<crate::bus::irq::IrqCallback>,
    irq_number: Option<u8>,
}

impl FsmcBus {
    pub fn new(device: Box<dyn ParallelDevice>) -> Self {
        Self {
            device,
            irq_callback: None,
            irq_number: None,
        }
    }

    pub fn set_irq(&mut self, irq_number: u8, callback: crate::bus::irq::IrqCallback) {
        self.irq_number = Some(irq_number);
        self.irq_callback = Some(callback);
    }

    /// Called when firmware writes to the FSMC address range.
    pub fn on_write(&mut self, addr: u32, data: u32, width: AccessWidth) {
        self.device.write(addr, data, width);
        self.fire_irq();
    }

    /// Called when firmware reads from the FSMC address range.
    pub fn on_read(&mut self, addr: u32, width: AccessWidth) -> u32 {
        self.device.read(addr, width)
    }

    pub fn reset(&mut self) {
        self.device.reset();
    }

    /// Access the underlying device (for frame polling, etc.).
    pub fn device_mut(&mut self) -> &mut dyn ParallelDevice {
        self.device.as_mut()
    }

    fn fire_irq(&mut self) {
        if let (Some(cb), Some(irq)) = (&self.irq_callback, self.irq_number) {
            cb(irq);
        }
    }
}
