pub mod bus;
pub mod clock;
pub mod cpu;
pub mod execution;
pub mod image;
pub mod machine;
pub mod memory;
pub mod target;

pub use bus::irq::{IrqCallback, IrqSource};
pub use bus::traits::{
    FrameUpdate, GpioListener, I2cSlave, ParallelDevice, SpiSlave, SystemBus, UartDevice,
};
pub use bus::{
    AccessWidth, AdcPeripheral, BusContext, BusContextStats, CanFrame, DacPeripheral,
    DmaController, DmaDataWidth, DmaDirection, DmaStreamConfig, DmaTransfer, FixedAnalogSource,
    FsmcBus, GpioNotifier, GpioPin, I2cBus, NoiseAnalogSource, SineAnalogSource, SpiBus, UartBus,
};
pub use clock::RccClockModel;
pub use cpu::{ArchitectureId, CpuArchitecture, CpuCore};
pub use execution::{adc_sr_dr_addrs, gpio_idr_addr, gpio_port_letter, pin_to_adc_channel, StepBatchController};
pub use image::{FirmwareFormat, FirmwareLoader};
pub use machine::{
    Machine, MachineBusInterface, MmioWriteEvent, SerialEvent,
    META_GPIO_ANY, META_GPIO_BSRR, META_GPIO_ODR, META_I2C_CR1, META_I2C_DR,
    META_SPI_DATA, META_SPI_STATUS, META_USART_DATA,
};
pub use memory::{FirmwareImage, MemoryBlock};
pub use target::{
    CpuType, MemoryRegion, MemoryRegionKind, PeripheralSpec, RegisterSpec, RegisterValueSet,
    TargetSpec,
};
