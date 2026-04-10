use crate::bus::access::{AccessWidth, CanFrame};

// ---------------------------------------------------------------------------
// Bus device traits — each bus type has its own trait reflecting its protocol
// ---------------------------------------------------------------------------

/// Legacy system bus trait (memory-mapped read/write).
pub trait SystemBus {
    fn read8(&mut self, addr: u64) -> Result<u8, String>;
    fn write8(&mut self, addr: u64, value: u8) -> Result<(), String>;

    fn read_block(&mut self, addr: u64, buf: &mut [u8]) -> Result<(), String> {
        for (i, byte) in buf.iter_mut().enumerate() {
            *byte = self.read8(addr + i as u64)?;
        }
        Ok(())
    }

    fn write_block(&mut self, addr: u64, buf: &[u8]) -> Result<(), String> {
        for (i, &byte) in buf.iter().enumerate() {
            self.write8(addr + i as u64, byte)?;
        }
        Ok(())
    }

    fn read16(&mut self, addr: u64) -> Result<u16, String> {
        let lo = self.read8(addr)? as u16;
        let hi = self.read8(addr + 1)? as u16;
        Ok(lo | (hi << 8))
    }

    fn read32(&mut self, addr: u64) -> Result<u32, String> {
        let b0 = self.read8(addr)? as u32;
        let b1 = self.read8(addr + 1)? as u32;
        let b2 = self.read8(addr + 2)? as u32;
        let b3 = self.read8(addr + 3)? as u32;
        Ok(b0 | (b1 << 8) | (b2 << 16) | (b3 << 24))
    }

    fn write16(&mut self, addr: u64, value: u16) -> Result<(), String> {
        self.write8(addr, (value & 0xFF) as u8)?;
        self.write8(addr + 1, (value >> 8) as u8)
    }

    fn write32(&mut self, addr: u64, value: u32) -> Result<(), String> {
        self.write8(addr, (value & 0xFF) as u8)?;
        self.write8(addr + 1, ((value >> 8) & 0xFF) as u8)?;
        self.write8(addr + 2, ((value >> 16) & 0xFF) as u8)?;
        self.write8(addr + 3, ((value >> 24) & 0xFF) as u8)
    }
}

/// SPI slave device — reflects full-duplex transfer timing.
pub trait SpiSlave: Send {
    /// SPI transfer: master sends `mosi`, slave returns `miso` synchronously.
    fn transfer(&mut self, mosi: u8) -> u8;

    /// CS (chip select) line changed.
    fn chip_select(&mut self, active: bool);

    /// Device reset.
    fn reset(&mut self);
}

/// I2C slave device — reflects START / address / read-write / STOP timing.
pub trait I2cSlave: Send {
    /// After START, bus sends 7-bit address + R/W direction.
    /// Return `true` to ACK (address matched).
    /// `addr7` range: 0x00..=0x7F.
    fn address(&mut self, addr7: u8, read: bool) -> bool;

    /// Master write mode: one byte received from bus.
    fn write_byte(&mut self, data: u8);

    /// Master read mode: device provides one byte to bus.
    fn read_byte(&mut self) -> u8;

    /// STOP condition.
    fn stop(&mut self);

    /// Device reset.
    fn reset(&mut self);
}

/// UART device — bidirectional, asynchronous.
pub trait UartDevice: Send {
    /// MCU TX: firmware wrote a byte to DR.
    fn on_tx(&mut self, byte: u8);

    /// MCU RX: firmware reads DR. Device returns a byte from its internal buffer,
    /// or `None` if empty (bus will not set RXNE).
    fn poll_rx(&mut self) -> Option<u8>;

    /// Device reset.
    fn reset(&mut self);
}

/// Parallel bus device (FSMC / memory-mapped LCD).
pub trait ParallelDevice: Send {
    fn write(&mut self, addr: u32, data: u32, width: AccessWidth);
    fn read(&mut self, addr: u32, width: AccessWidth) -> u32;
    fn reset(&mut self);
}

/// GPIO pin-change listener (for DC/RS control lines, LEDs, buttons, etc.).
pub trait GpioListener: Send {
    fn pin_changed(&mut self, port: char, pin: u8, high: bool);
}

/// Analog signal source (ADC input).
pub trait AnalogSource: Send {
    /// Sample a channel, return 0-4095 (12-bit).
    fn sample(&mut self, channel: u8) -> u16;
}

/// Analog signal sink (DAC output).
pub trait AnalogSink: Send {
    /// DAC output on a channel.
    fn output(&mut self, channel: u8, value: u16);
}

/// CAN node (stub).
pub trait CanNode: Send {
    fn receive_frame(&mut self, frame: &CanFrame);
    fn poll_tx(&mut self) -> Option<CanFrame>;
}

/// USB device (stub).
pub trait UsbDevice: Send {
    fn setup_packet(&mut self, pkt: &[u8]);
    fn data_in(&mut self, endpoint: u8, data: &[u8]);
    fn poll_data_out(&mut self, endpoint: u8) -> Option<Vec<u8>>;
}
