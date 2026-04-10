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
pub use execution::{gpio_idr_addr, gpio_port_letter, StepBatchController};
pub use image::{FirmwareFormat, FirmwareLoader};
pub use machine::{Machine, MachineBusInterface, MmioWriteEvent, SerialEvent};
pub use memory::{FirmwareImage, MemoryBlock};
pub use target::{
    CpuType, MemoryRegion, MemoryRegionKind, PeripheralSpec, RegisterSpec, RegisterValueSet,
    TargetSpec,
};
