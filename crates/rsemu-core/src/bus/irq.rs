/// Interrupt callback type.
/// All interrupt-capable peripherals/buses use this to notify NVIC.
pub type IrqCallback = Box<dyn Fn(u8) + Send + Sync>;

/// Interrupt source: anything that can trigger an IRQ.
pub trait IrqSource {
    /// Set the interrupt callback.
    fn set_irq_callback(&mut self, cb: IrqCallback);
}
