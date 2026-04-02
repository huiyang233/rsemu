pub mod bus;
pub mod cpu;
pub mod image;
pub mod machine;
pub mod memory;
pub mod target;

pub use bus::SystemBus;
pub use cpu::{ArchitectureId, CpuArchitecture, CpuCore};
pub use image::{FirmwareFormat, FirmwareLoader};
pub use machine::{Machine, MachineBusInterface, MmioWriteEvent, SerialEvent};
pub use memory::{FirmwareImage, MemoryBlock};
pub use target::{
    MemoryRegion, MemoryRegionKind, PeripheralSpec, RegisterSpec, RegisterValueSet, TargetSpec,
};
