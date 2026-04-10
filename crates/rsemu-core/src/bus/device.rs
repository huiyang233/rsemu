use crate::bus::traits::{
    AnalogSink, AnalogSource, CanNode, GpioListener, I2cSlave, ParallelDevice, SpiSlave,
    UartDevice, UsbDevice,
};

// ---------------------------------------------------------------------------
// Device capabilities (bitflags)
// ---------------------------------------------------------------------------

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct DeviceCapabilities: u32 {
        const SPI_SLAVE     = 0x01;
        const I2C_SLAVE     = 0x02;
        const UART_DEVICE   = 0x04;
        const PARALLEL      = 0x08;
        const GPIO_LISTENER = 0x10;
        const ANALOG_SOURCE = 0x20;
        const ANALOG_SINK   = 0x40;
        const CAN_NODE      = 0x80;
        const USB_DEVICE    = 0x100;
    }
}

// ---------------------------------------------------------------------------
// BusAttach — capability entry point, no Any::downcast
// ---------------------------------------------------------------------------

/// Device capability trait: default implementations return `None`.
/// Concrete devices override only the methods for capabilities they support.
pub trait BusAttach: Send {
    fn as_spi_slave(&mut self) -> Option<&mut dyn SpiSlave> {
        None
    }
    fn as_i2c_slave(&mut self) -> Option<&mut dyn I2cSlave> {
        None
    }
    fn as_uart_device(&mut self) -> Option<&mut dyn UartDevice> {
        None
    }
    fn as_parallel(&mut self) -> Option<&mut dyn ParallelDevice> {
        None
    }
    fn as_gpio_listener(&mut self) -> Option<&mut dyn GpioListener> {
        None
    }
    fn as_analog_source(&mut self) -> Option<&mut dyn AnalogSource> {
        None
    }
    fn as_analog_sink(&mut self) -> Option<&mut dyn AnalogSink> {
        None
    }
    fn as_can_node(&mut self) -> Option<&mut dyn CanNode> {
        None
    }
    fn as_usb_device(&mut self) -> Option<&mut dyn UsbDevice> {
        None
    }
}

// ---------------------------------------------------------------------------
// DeviceHandle — unified handle with capability query
// ---------------------------------------------------------------------------

/// Unified device handle. Capability query through `BusAttach` vtable dispatch.
pub struct DeviceHandle {
    pub capabilities: DeviceCapabilities,
    inner: Box<dyn BusAttach>,
}

impl DeviceHandle {
    pub fn new(capabilities: DeviceCapabilities, inner: Box<dyn BusAttach>) -> Self {
        Self {
            capabilities,
            inner,
        }
    }

    pub fn as_spi_slave(&mut self) -> Option<&mut dyn SpiSlave> {
        self.inner.as_spi_slave()
    }
    pub fn as_i2c_slave(&mut self) -> Option<&mut dyn I2cSlave> {
        self.inner.as_i2c_slave()
    }
    pub fn as_uart_device(&mut self) -> Option<&mut dyn UartDevice> {
        self.inner.as_uart_device()
    }
    pub fn as_parallel(&mut self) -> Option<&mut dyn ParallelDevice> {
        self.inner.as_parallel()
    }
    pub fn as_gpio_listener(&mut self) -> Option<&mut dyn GpioListener> {
        self.inner.as_gpio_listener()
    }
    pub fn as_analog_source(&mut self) -> Option<&mut dyn AnalogSource> {
        self.inner.as_analog_source()
    }
    pub fn as_analog_sink(&mut self) -> Option<&mut dyn AnalogSink> {
        self.inner.as_analog_sink()
    }
    pub fn as_can_node(&mut self) -> Option<&mut dyn CanNode> {
        self.inner.as_can_node()
    }
    pub fn as_usb_device(&mut self) -> Option<&mut dyn UsbDevice> {
        self.inner.as_usb_device()
    }
}

// ---------------------------------------------------------------------------
// DeviceRegistry — factory for creating devices
// ---------------------------------------------------------------------------

/// Trait for device type registration. Each device type implements this once.
pub trait DeviceRegistry: Send + Sync {
    fn name(&self) -> &str;
    fn capabilities(&self) -> DeviceCapabilities;
    fn create(&self) -> DeviceHandle;
}
