mod access;
mod adc_dac;
mod device;
mod dma;
mod fsmc_bus;
mod gpio;
mod i2c_bus;
mod spi_bus;
mod uart_bus;

pub mod irq;
pub mod traits;

// Re-export SystemBus (legacy)
pub use traits::SystemBus;

// Re-export new types
pub use access::{AccessWidth, CanFrame};
pub use adc_dac::{
    AdcPeripheral, DacPeripheral, FixedAnalogSource, NoiseAnalogSource, SineAnalogSource,
};
pub use device::{BusAttach, DeviceCapabilities, DeviceHandle, DeviceRegistry};
pub use dma::{
    DmaController, DmaDataWidth, DmaDirection, DmaStreamConfig, DmaTransfer,
};
pub use fsmc_bus::FsmcBus;
pub use gpio::{GpioNotifier, GpioPin};
pub use i2c_bus::I2cBus;
pub use spi_bus::SpiBus;
pub use uart_bus::UartBus;
