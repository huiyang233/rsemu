pub mod bus;
pub mod cpu;
pub mod image;
pub mod machine;
pub mod memory;
pub mod target;

pub use bus::irq::{IrqCallback, IrqSource};
pub use bus::traits::{
    AnalogSink, AnalogSource, CanNode, GpioListener, I2cSlave, ParallelDevice, SpiSlave,
    SystemBus, UartDevice, UsbDevice,
};
pub use bus::{
    AccessWidth, AdcPeripheral, BusAttach, CanFrame, DacPeripheral, DeviceCapabilities,
    DeviceHandle, DeviceRegistry, DmaController, DmaDataWidth, DmaDirection, DmaStreamConfig,
    DmaTransfer, FixedAnalogSource, FsmcBus, GpioNotifier, GpioPin, I2cBus, NoiseAnalogSource,
    SineAnalogSource, SpiBus, UartBus,
};
pub use cpu::{ArchitectureId, CpuArchitecture, CpuCore};
pub use image::{FirmwareFormat, FirmwareLoader};
pub use machine::{Machine, MachineBusInterface, MmioWriteEvent, SerialEvent};
pub use memory::{FirmwareImage, MemoryBlock};
pub use target::{
    MemoryRegion, MemoryRegionKind, PeripheralSpec, RegisterSpec, RegisterValueSet, TargetSpec,
};
