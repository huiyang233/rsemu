use crate::bus::traits::I2cSlave;

/// I2C bus state machine states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum I2cPhase {
    Idle,
    AwaitAddress,
    MasterWrite,
    MasterRead,
}

/// I2C bus controller — sits between the machine's register layer and the I2C slave device.
///
/// The machine calls `on_cr1_write()` when firmware writes to I2C_CR1 (detecting START/STOP).
/// The machine calls `on_dr_write()` when firmware writes to I2C_DR (data byte).
/// The machine calls `on_dr_read()` when firmware reads from I2C_DR (slave provides data).
pub struct I2cBus {
    device: Box<dyn I2cSlave>,
    phase: I2cPhase,
    irq_callback: Option<crate::bus::irq::IrqCallback>,
    irq_number: Option<u8>,
}

impl I2cBus {
    pub fn new(device: Box<dyn I2cSlave>) -> Self {
        Self {
            device,
            phase: I2cPhase::Idle,
            irq_callback: None,
            irq_number: None,
        }
    }

    /// Set the IRQ number and callback for this I2C bus.
    pub fn set_irq(&mut self, irq_number: u8, callback: crate::bus::irq::IrqCallback) {
        self.irq_number = Some(irq_number);
        self.irq_callback = Some(callback);
    }

    /// Called when firmware writes to I2C_CR1.
    /// Detects START and STOP conditions.
    pub fn on_cr1_write(&mut self, value: u32) {
        // START condition
        if (value & (1 << 8)) != 0 {
            self.phase = I2cPhase::AwaitAddress;
        }
        // STOP condition
        if (value & (1 << 9)) != 0 {
            if self.phase != I2cPhase::Idle {
                self.device.stop();
            }
            self.phase = I2cPhase::Idle;
        }
    }

    /// Called when firmware writes to I2C_DR.
    pub fn on_dr_write(&mut self, byte: u8) {
        match self.phase {
            I2cPhase::AwaitAddress => {
                let addr7 = byte >> 1;
                let read = (byte & 1) != 0;
                if self.device.address(addr7, read) {
                    self.phase = if read {
                        I2cPhase::MasterRead
                    } else {
                        I2cPhase::MasterWrite
                    };
                } else {
                    // NACK — no matching slave
                    self.phase = I2cPhase::Idle;
                }
            }
            I2cPhase::MasterWrite => {
                self.device.write_byte(byte);
                self.fire_irq();
            }
            _ => {}
        }
    }

    /// Called when firmware reads from I2C_DR (master read mode).
    pub fn on_dr_read(&mut self) -> u8 {
        if self.phase == I2cPhase::MasterRead {
            self.fire_irq();
            self.device.read_byte()
        } else {
            0
        }
    }

    /// Access the underlying device.
    pub fn device_mut(&mut self) -> &mut dyn I2cSlave {
        self.device.as_mut()
    }

    pub fn reset(&mut self) {
        self.device.reset();
        self.phase = I2cPhase::Idle;
    }

    fn fire_irq(&mut self) {
        if let (Some(cb), Some(irq)) = (&self.irq_callback, self.irq_number) {
            cb(irq);
        }
    }
}
