use rsemu_core::PeripheralSpec;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SvdDevice {
    pub name: String,
    pub peripherals: Vec<PeripheralSpec>,
}
