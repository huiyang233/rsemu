use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tracing::trace;

use crate::bus::SystemBus;
use crate::cpu::CpuCore;
use crate::memory::{FirmwareImage, MemoryBlock};
use crate::target::{MemoryRegionKind, TargetSpec};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialEvent {
    pub peripheral: Arc<str>,
    pub byte: u8,
}

#[derive(Debug, Clone)]
pub struct MmioWriteEvent {
    pub peripheral: Arc<str>,
    pub register: Arc<str>,
    pub addr: u64,
    pub width: u8,
    pub value: u32,
    /// Classification flags from RegisterMeta, forwarded for fast dispatch.
    pub flags: u16,
}

#[derive(Debug, Clone)]
struct RegisterMeta {
    peripheral: Arc<str>,
    register: Arc<str>,
    byte_offset: u8,
    flags: u16,
    systick: Option<SystickReg>,
    paired_addr: u64,
}

pub const META_USART_STATUS: u16 = 1 << 0;
pub const META_USART_DATA: u16 = 1 << 1;
pub const META_SPI_STATUS: u16 = 1 << 2;
pub const META_SPI_DATA: u16 = 1 << 3;
pub const META_RCC_CONTROL: u16 = 1 << 4;
pub const META_TIMER_STATUS: u16 = 1 << 5;
pub const META_GPIO_BSRR: u16 = 1 << 6;
pub const META_RCC_CFGR: u16 = 1 << 7;
pub const META_RCC_CSR: u16 = 1 << 8;
pub const META_PWR_CSR: u16 = 1 << 9;
pub const META_I2C_CR1: u16 = 1 << 10;
pub const META_I2C_DR: u16 = 1 << 11;
pub const META_GPIO_ODR: u16 = 1 << 12;
pub const META_GPIO_ANY: u16 = META_GPIO_ODR | META_GPIO_BSRR;

#[derive(Debug, Clone)]
struct SystickState {
    csr: u32,
    rvr_raw: u32,
    cvr: u32,
    countflag: bool,
    pending_interrupts: u32,
    reload_divider: u32,
    scale_reload: bool,
}

impl SystickState {
    fn new(reload_divider: u32) -> Self {
        Self {
            csr: 0,
            rvr_raw: 0,
            cvr: 0,
            countflag: false,
            pending_interrupts: 0,
            reload_divider: reload_divider.max(1),
            scale_reload: true,
        }
    }

    fn enabled(&self) -> bool {
        self.csr & 0x1 != 0
    }

    fn effective_period(&self) -> u32 {
        let raw_period = (self.rvr_raw & 0x00FF_FFFF).wrapping_add(1);
        if self.scale_reload {
            let scaled = raw_period / self.reload_divider;
            scaled.max(1)
        } else {
            raw_period.max(1)
        }
    }

    fn tick_many(&mut self, ticks: u64) {
        if ticks == 0 || !self.enabled() {
            return;
        }

        let period = u64::from(self.effective_period().max(1));
        let mut remaining = ticks;

        if self.cvr == 0 {
            self.countflag = true;
            self.pending_interrupts = self.pending_interrupts.saturating_add(1);
            self.cvr = (period.saturating_sub(1)).min(u64::from(u32::MAX)) as u32;
            remaining = remaining.saturating_sub(1);
            if remaining == 0 {
                return;
            }
        }

        let current = u64::from(self.cvr);
        if remaining <= current {
            self.cvr = (current - remaining) as u32;
            return;
        }

        remaining -= current + 1;
        self.countflag = true;
        self.pending_interrupts = self.pending_interrupts.saturating_add(1);

        if remaining > 0 {
            let wraps = remaining / period;
            if wraps > 0 {
                let add = wraps.min(u64::from(u32::MAX)) as u32;
                self.pending_interrupts = self.pending_interrupts.saturating_add(add);
                self.countflag = true;
                remaining %= period;
            }
        }

        self.cvr = (period - 1 - remaining).min(u64::from(u32::MAX)) as u32;
    }

    fn take_pending_interrupt(&mut self) -> bool {
        if self.pending_interrupts == 0 {
            return false;
        }
        self.pending_interrupts = self.pending_interrupts.saturating_sub(1);
        true
    }

    fn read8(&mut self, reg: SystickReg, byte_offset: u8) -> u8 {
        let value = match reg {
            SystickReg::Ctrl => (self.csr & 0x7) | if self.countflag { 1 << 16 } else { 0 },
            SystickReg::Load => self.rvr_raw & 0x00FF_FFFF,
            SystickReg::Val => self.cvr,
            SystickReg::Calib => 0,
        };
        ((value >> ((byte_offset as u32) * 8)) & 0xFF) as u8
    }

    fn read32(&mut self, reg: SystickReg) -> u32 {
        match reg {
            SystickReg::Ctrl => {
                let value = (self.csr & 0x7) | if self.countflag { 1 << 16 } else { 0 };
                self.countflag = false;
                value
            }
            SystickReg::Load => self.rvr_raw & 0x00FF_FFFF,
            SystickReg::Val => self.cvr,
            SystickReg::Calib => 0,
        }
    }

    fn write32(&mut self, reg: SystickReg, value: u32) {
        match reg {
            SystickReg::Ctrl => {
                self.csr = value & 0x7;
            }
            SystickReg::Load => {
                self.rvr_raw = value & 0x00FF_FFFF;
            }
            SystickReg::Val => {
                self.cvr = 0;
                self.countflag = false;
                self.pending_interrupts = 0;
            }
            SystickReg::Calib => {}
        }
    }

    fn write8(&mut self, reg: SystickReg, byte_offset: u8, value: u8) {
        let current = match reg {
            SystickReg::Ctrl => self.csr,
            SystickReg::Load => self.rvr_raw,
            SystickReg::Val => self.cvr,
            SystickReg::Calib => 0,
        };
        let shift = (byte_offset as u32) * 8;
        let mask = !(0xFFu32 << shift);
        let merged = (current & mask) | ((value as u32) << shift);
        self.write32(reg, merged);
    }

    fn set_reload_scaling(&mut self, enabled: bool) {
        self.scale_reload = enabled;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SystickReg {
    Ctrl,
    Load,
    Val,
    Calib,
}

#[derive(Debug, Clone, Copy)]
struct TimerRegs {
    cr1: u64,
    dier: u64,
    sr: u64,
    cnt: u64,
    psc: u64,
    arr: u64,
}

#[derive(Debug, Clone)]
struct TimerModel {
    peripheral: String,
    regs: TimerRegs,
    irq: Option<u8>,
    tick_residual: u64,
}

#[derive(Debug, Clone, Copy)]
struct RccRegs {
    cr: u64,
    cfgr: u64,
    apb1enr: Option<u64>,
}

pub trait MachineBusInterface {
    fn read_mmio(&self, addr: u64) -> u8;
}

impl<C: CpuCore> MachineBusInterface for Machine<C> {
    fn read_mmio(&self, addr: u64) -> u8 {
        mmio_read_byte(&self.mmio, addr)
    }
}

pub struct Machine<C: CpuCore> {
    cpu: C,
    target: TargetSpec,
    flash_base: u64,
    flash_alias_base: Option<u64>,
    periph_bb_base: Option<u64>,
    periph_bb_alias_start: Option<u64>,
    periph_bb_alias_end: Option<u64>,
    memory: Vec<MemoryBlock>,
    mmio: HashMap<u64, u32>,
    register_meta: HashMap<u64, RegisterMeta>,
    usart_peripheral_map: HashMap<String, (u64, u64)>,
    usart_rx_queues: HashMap<u64, VecDeque<u8>>,
    serial_output: Vec<SerialEvent>,
    mmio_writes: Vec<MmioWriteEvent>,
    systick: SystickState,
    step_driven_systick: bool,
    step_driven_timers: bool,
    nvic_any_enabled: bool,
    timers: Vec<TimerModel>,
    rcc_regs: Option<RccRegs>,
    pending_irqs: Arc<Mutex<Vec<u8>>>,
}

impl<C: CpuCore> Machine<C> {
    pub fn new(cpu: C, target: TargetSpec) -> Self {
        let arch_mem = cpu.architecture().memory_config();
        let flash_base = target
            .memory_map
            .iter()
            .find(|region| region.kind == MemoryRegionKind::Flash)
            .map(|region| region.range.start)
            .unwrap_or(0);

        let reload_divider = target.systick_reload_divider;
        let mut memory = Vec::new();
        for region in &target.memory_map {
            match region.kind {
                MemoryRegionKind::Flash => {
                    memory.push(MemoryBlock::new(
                        region.range.start,
                        (region.range.end - region.range.start) as usize,
                        false,
                    ));
                }
                MemoryRegionKind::Ram => {
                    memory.push(MemoryBlock::new(
                        region.range.start,
                        (region.range.end - region.range.start) as usize,
                        true,
                    ));
                }
                _ => {}
            }
        }

        let mut mmio = HashMap::new();
        let mut register_meta = HashMap::new();
        let mut usart_peripheral_map = HashMap::new();
        for peripheral in &target.peripherals {
            let spi_sr_addr = peripheral
                .registers
                .iter()
                .find(|r| r.name.eq_ignore_ascii_case("SR"))
                .map(|r| r.address)
                .unwrap_or(0);
            let usart_sr_addr = peripheral
                .registers
                .iter()
                .find(|r| r.name.eq_ignore_ascii_case("SR"))
                .map(|r| r.address)
                .unwrap_or(0);
            let usart_dr_addr = peripheral
                .registers
                .iter()
                .find(|r| r.name.eq_ignore_ascii_case("DR"))
                .map(|r| r.address)
                .unwrap_or(0);
            let gpio_odr_addr = peripheral
                .registers
                .iter()
                .find(|r| r.name.eq_ignore_ascii_case("ODR"))
                .map(|r| r.address)
                .unwrap_or(0);
            if peripheral.name.starts_with("USART") || peripheral.name.starts_with("UART") {
                if usart_sr_addr != 0 && usart_dr_addr != 0 {
                    usart_peripheral_map.insert(
                        peripheral.name.to_ascii_uppercase(),
                        (usart_sr_addr, usart_dr_addr),
                    );
                }
            }

            for register in &peripheral.registers {
                let flags = classify_meta_flags(&peripheral.name, &register.name);
                let systick = classify_systick_reg(&peripheral.name, &register.name);
                let paired_addr = if flags & META_SPI_DATA != 0 || flags & META_SPI_STATUS != 0 {
                    spi_sr_addr
                } else if flags & META_USART_DATA != 0 || flags & META_USART_STATUS != 0 {
                    usart_sr_addr
                } else if flags & META_GPIO_BSRR != 0 {
                    gpio_odr_addr
                } else {
                    0
                };
                let width = (register.width_bits / 8).clamp(1, 8) as usize;
                // Initialise mmio with the reset value; store as word-aligned u32
                let reset_word = register.reset_value as u32;
                write_mmio_u32(&mut mmio, register.address, reset_word);
                let p_arc: Arc<str> = Arc::from(peripheral.name.to_ascii_uppercase());
                let r_arc: Arc<str> = Arc::from(register.name.to_ascii_uppercase());
                for index in 0..width {
                    let addr = register.address + index as u64;
                    register_meta.insert(
                        addr,
                        RegisterMeta {
                            peripheral: Arc::clone(&p_arc),
                            register: Arc::clone(&r_arc),
                            byte_offset: index as u8,
                            flags,
                            systick,
                            paired_addr,
                        },
                    );
                }
            }
        }

        let timers = resolve_timer_models(&target);
        let rcc_regs = resolve_rcc_regs(&target);
        let nvic_any_enabled = nvic_any_enabled(&mmio);

        Self {
            cpu,
            target,
            flash_base,
            flash_alias_base: arch_mem.flash_alias_base,
            periph_bb_base: arch_mem.periph_bitband_base,
            periph_bb_alias_start: arch_mem.periph_bitband_alias_start,
            periph_bb_alias_end: arch_mem.periph_bitband_alias_end,
            memory,
            mmio,
            register_meta,
            usart_peripheral_map,
            usart_rx_queues: HashMap::new(),
            serial_output: Vec::new(),
            mmio_writes: Vec::new(),
            systick: SystickState::new(reload_divider),
            step_driven_systick: true,
            step_driven_timers: true,
            nvic_any_enabled,
            timers,
            rcc_regs,
            pending_irqs: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn cpu(&self) -> &C {
        &self.cpu
    }

    pub fn target(&self) -> &TargetSpec {
        &self.target
    }

    pub fn load_firmware(&mut self, firmware: &FirmwareImage) -> Result<(), String> {
        for segment in firmware.segments() {
            let block = self
                .memory
                .iter_mut()
                .find(|block| {
                    block.contains(segment.load_address)
                        && segment.load_address + segment.bytes.len() as u64 <= block.end()
                })
                .ok_or_else(|| {
                    format!(
                        "firmware segment 0x{:08x}..0x{:08x} does not match any writable or flash memory region",
                        segment.load_address,
                        segment.load_address + segment.bytes.len() as u64
                    )
                })?;

            block.load_bytes(segment.load_address, &segment.bytes)?;
        }

        Ok(())
    }

    pub fn reset_cpu(&mut self) -> Result<(), String> {
        let vector_table_base = self.target.vector_table_base;
        let mut bus = MachineBus {
            memory: &mut self.memory,
            flash_base: self.flash_base,
            flash_alias_base: self.flash_alias_base,
            periph_bb_base: self.periph_bb_base,
            periph_bb_alias_start: self.periph_bb_alias_start,
            periph_bb_alias_end: self.periph_bb_alias_end,
            mmio: &mut self.mmio,
            register_meta: &self.register_meta,
            usart_rx_queues: &mut self.usart_rx_queues,
            serial_output: &mut self.serial_output,
            mmio_writes: &mut self.mmio_writes,
            systick: &mut self.systick,
            nvic_any_enabled: &mut self.nvic_any_enabled,
        };
        self.cpu.reset(&mut bus, vector_table_base, &self.target.memory_map)
    }

    pub fn read_mmio(&self, addr: u64) -> u8 {
        mmio_read_byte(&self.mmio, addr)
    }

    pub fn step_cpu(&mut self, max_steps: usize) -> Result<u32, String> {
        let mut ran_total = 0u32;

        let mut bus = MachineBus {
            memory: &mut self.memory,
            flash_base: self.flash_base,
            flash_alias_base: self.flash_alias_base,
            periph_bb_base: self.periph_bb_base,
            periph_bb_alias_start: self.periph_bb_alias_start,
            periph_bb_alias_end: self.periph_bb_alias_end,
            mmio: &mut self.mmio,
            register_meta: &self.register_meta,
            usart_rx_queues: &mut self.usart_rx_queues,
            serial_output: &mut self.serial_output,
            mmio_writes: &mut self.mmio_writes,
            systick: &mut self.systick,
            nvic_any_enabled: &mut self.nvic_any_enabled,
        };
        let ran = self.cpu.step(&mut bus, max_steps)?;
        ran_total = ran_total.saturating_add(ran);
        if self.step_driven_systick {
            self.systick.tick_many(ran as u64);
        }
        if self.step_driven_timers {
            let emu_cycles = (self.target.systick_reload_divider.max(1) as u64) * (ran as u64);
            self.advance_timers(emu_cycles);
        }
        self.try_service_interrupt()?;

        // If we just entered an exception (e.g. PendSV/SysTick), execute a small
        // bounded chunk immediately so RTOS context switches are not delayed by the
        // next large batch step.
        let mut exception_budget = 256usize;
        while self.cpu.in_exception() && exception_budget > 0 {
            let chunk = exception_budget.min(64);
            let mut bus = MachineBus {
                memory: &mut self.memory,
                flash_base: self.flash_base,
                flash_alias_base: self.flash_alias_base,
                periph_bb_base: self.periph_bb_base,
                periph_bb_alias_start: self.periph_bb_alias_start,
                periph_bb_alias_end: self.periph_bb_alias_end,
                mmio: &mut self.mmio,
                register_meta: &self.register_meta,
                usart_rx_queues: &mut self.usart_rx_queues,
                serial_output: &mut self.serial_output,
                mmio_writes: &mut self.mmio_writes,
                systick: &mut self.systick,
                nvic_any_enabled: &mut self.nvic_any_enabled,
            };
            let ran_exc = self.cpu.step(&mut bus, chunk)?;
            if ran_exc == 0 {
                break;
            }
            ran_total = ran_total.saturating_add(ran_exc);
            if self.step_driven_systick {
                self.systick.tick_many(ran_exc as u64);
            }
            if self.step_driven_timers {
                let emu_cycles =
                    (self.target.systick_reload_divider.max(1) as u64) * (ran_exc as u64);
                self.advance_timers(emu_cycles);
            }
            self.try_service_interrupt()?;
            exception_budget = exception_budget.saturating_sub(ran_exc as usize);
        }

        Ok(ran_total)
    }

    pub fn set_step_driven_systick(&mut self, enabled: bool) {
        self.step_driven_systick = enabled;
    }

    pub fn set_step_driven_timers(&mut self, enabled: bool) {
        self.step_driven_timers = enabled;
    }

    pub fn advance_timers_cycles(&mut self, cycles: u64) {
        self.advance_timers(cycles);
    }

    pub fn advance_systick_ticks(&mut self, ticks: u64) {
        self.systick.tick_many(ticks);
    }

    pub fn set_systick_reload_scaling(&mut self, enabled: bool) {
        self.systick.set_reload_scaling(enabled);
    }

    /// Set an IRQ pending in NVIC. Safe to call from IrqCallback.
    /// `irq` is the NVIC interrupt number (0-based, exception 16 maps to irq 0).
    pub fn set_irq_pending(&mut self, irq: u8) {
        nvic_pending_write(&mut self.mmio, irq, true);
    }

    /// Create an IrqCallback that sets an IRQ pending on this machine.
    /// Usage: `let cb = machine.make_irq_callback(); spi_bus.set_irq(irq_num, cb);`
    /// The callback pushes IRQ numbers to a shared queue; call `flush_pending_irqs()`
    /// after each step batch to apply them to NVIC.
    pub fn make_irq_callback(&self) -> crate::IrqCallback {
        let queue = Arc::clone(&self.pending_irqs);
        Box::new(move |irq: u8| {
            if let Ok(mut q) = queue.lock() {
                q.push(irq);
            }
        })
    }

    /// Flush any IRQs queued by IrqCallback into the NVIC pending registers.
    /// Call this after each step batch in the main loop.
    pub fn flush_pending_irqs(&mut self) {
        let irqs: Vec<u8> = {
            let mut q = self.pending_irqs.lock().unwrap();
            std::mem::take(&mut *q)
        };
        for irq in irqs {
            nvic_pending_write(&mut self.mmio, irq, true);
        }
    }

    /// Access the shared pending-IRQ queue (for wiring up callbacks).
    pub fn pending_irqs(&self) -> &Arc<Mutex<Vec<u8>>> {
        &self.pending_irqs
    }

    pub fn read8(&mut self, addr: u64) -> Result<u8, String> {
        let mut bus = MachineBus {
            memory: &mut self.memory,
            flash_base: self.flash_base,
            flash_alias_base: self.flash_alias_base,
            periph_bb_base: self.periph_bb_base,
            periph_bb_alias_start: self.periph_bb_alias_start,
            periph_bb_alias_end: self.periph_bb_alias_end,
            mmio: &mut self.mmio,
            register_meta: &self.register_meta,
            usart_rx_queues: &mut self.usart_rx_queues,
            serial_output: &mut self.serial_output,
            mmio_writes: &mut self.mmio_writes,
            systick: &mut self.systick,
            nvic_any_enabled: &mut self.nvic_any_enabled,
        };
        bus.read8(addr)
    }

    pub fn write8(&mut self, addr: u64, value: u8) -> Result<(), String> {
        let mut bus = MachineBus {
            memory: &mut self.memory,
            flash_base: self.flash_base,
            flash_alias_base: self.flash_alias_base,
            periph_bb_base: self.periph_bb_base,
            periph_bb_alias_start: self.periph_bb_alias_start,
            periph_bb_alias_end: self.periph_bb_alias_end,
            mmio: &mut self.mmio,
            register_meta: &self.register_meta,
            usart_rx_queues: &mut self.usart_rx_queues,
            serial_output: &mut self.serial_output,
            mmio_writes: &mut self.mmio_writes,
            systick: &mut self.systick,
            nvic_any_enabled: &mut self.nvic_any_enabled,
        };
        bus.write8(addr, value)
    }

    pub fn usart_push_rx_byte(&mut self, peripheral: &str, byte: u8) -> Result<(), String> {
        let key = peripheral.to_ascii_uppercase();
        let (sr_addr, dr_addr) = self
            .usart_peripheral_map
            .get(&key)
            .copied()
            .ok_or_else(|| format!("unknown USART peripheral: {peripheral}"))?;
        self.usart_rx_queues
            .entry(sr_addr)
            .or_default()
            .push_back(byte);
        mmio_write_byte(&mut self.mmio, dr_addr, byte);
        Ok(())
    }

    pub fn serial_output(&self) -> &[SerialEvent] {
        &self.serial_output
    }

    pub fn mmio_writes(&self) -> &[MmioWriteEvent] {
        &self.mmio_writes
    }

    pub fn clear_outputs(&mut self) {
        self.serial_output.clear();
        self.mmio_writes.clear();
    }

    fn try_service_interrupt(&mut self) -> Result<(), String> {
        if self.cpu.in_exception() {
            return Ok(());
        }

        // PendSV (Exception 14), used by RTOS context switching.
        let icsr = read_mmio_u32(&self.mmio, 0xE000_ED04);
        if (icsr & (1 << 28)) != 0 {
            let cleared = icsr & !(1 << 28);
            write_mmio_u32(&mut self.mmio, 0xE000_ED04, cleared);
            self.service_exception(14)?;
            return Ok(());
        }

        // Check Systick first (Exception 15)
        if (self.systick.csr & 0x3) == 0x3 && self.systick.take_pending_interrupt() {
            // Systick enabled, interrupt enabled, and it just rolled over
            self.service_exception(15)?;
            return Ok(());
        }

        if let Some(irq) = self.next_pending_enabled_irq() {
            self.service_irq(irq)?;
        }
        Ok(())
    }

    #[allow(dead_code)]
    fn service_exception(&mut self, exception_number: u16) -> Result<(), String> {
        let vector_table_base = read_mmio_u32(&self.mmio, 0xE000_ED08) as u64;
        let vector_table_base = if vector_table_base == 0 {
            self.target.vector_table_base
        } else {
            vector_table_base
        };

        let mut bus = MachineBus {
            memory: &mut self.memory,
            flash_base: self.flash_base,
            flash_alias_base: self.flash_alias_base,
            periph_bb_base: self.periph_bb_base,
            periph_bb_alias_start: self.periph_bb_alias_start,
            periph_bb_alias_end: self.periph_bb_alias_end,
            mmio: &mut self.mmio,
            register_meta: &self.register_meta,
            usart_rx_queues: &mut self.usart_rx_queues,
            serial_output: &mut self.serial_output,
            mmio_writes: &mut self.mmio_writes,
            systick: &mut self.systick,
            nvic_any_enabled: &mut self.nvic_any_enabled,
        };
        let entered = self
            .cpu
            .enter_exception(&mut bus, vector_table_base, exception_number)?;
        if !entered {
            if exception_number == 14 {
                let icsr = read_mmio_u32(&self.mmio, 0xE000_ED04) | (1 << 28);
                write_mmio_u32(&mut self.mmio, 0xE000_ED04, icsr);
            } else if exception_number == 15 {
                let icsr = read_mmio_u32(&self.mmio, 0xE000_ED04) | (1 << 26);
                write_mmio_u32(&mut self.mmio, 0xE000_ED04, icsr);
            }
        }
        Ok(())
    }

    fn service_irq(&mut self, irq: u8) -> Result<(), String> {
        let pending = nvic_pending_read(&self.mmio, irq);
        if !pending {
            return Ok(());
        }

        nvic_pending_write(&mut self.mmio, irq, false);
        let vector_table_base = read_mmio_u32(&self.mmio, 0xE000_ED08) as u64;
        let vector_table_base = if vector_table_base == 0 {
            self.target.vector_table_base
        } else {
            vector_table_base
        };

        let mut bus = MachineBus {
            memory: &mut self.memory,
            flash_base: self.flash_base,
            flash_alias_base: self.flash_alias_base,
            periph_bb_base: self.periph_bb_base,
            periph_bb_alias_start: self.periph_bb_alias_start,
            periph_bb_alias_end: self.periph_bb_alias_end,
            mmio: &mut self.mmio,
            register_meta: &self.register_meta,
            usart_rx_queues: &mut self.usart_rx_queues,
            serial_output: &mut self.serial_output,
            mmio_writes: &mut self.mmio_writes,
            systick: &mut self.systick,
            nvic_any_enabled: &mut self.nvic_any_enabled,
        };
        let entered = self
            .cpu
            .enter_exception(&mut bus, vector_table_base, u16::from(irq) + 16)?;
        if !entered {
            nvic_pending_write(&mut self.mmio, irq, true);
        }
        Ok(())
    }

    fn advance_timers(&mut self, emu_core_cycles: u64) {
        let (core_hz, tim_clk_hz) = self.compute_tim_clock_hz();
        if core_hz == 0 || tim_clk_hz == 0 {
            return;
        }

        for index in 0..self.timers.len() {
            self.advance_one_timer(index, emu_core_cycles, core_hz, tim_clk_hz);
        }
    }

    fn advance_one_timer(
        &mut self,
        index: usize,
        emu_core_cycles: u64,
        core_hz: u64,
        tim_clk_hz: u64,
    ) {
        // Copy lightweight fields to avoid borrowing self through the clone
        let regs = self.timers[index].regs;
        let irq = self.timers[index].irq;

        if !self.timer_clock_enabled(&self.timers[index].peripheral) {
            return;
        }
        let cr1 = read_mmio_u32(&self.mmio, regs.cr1);
        if (cr1 & 0x1) == 0 {
            return;
        }

        let psc = read_mmio_u32(&self.mmio, regs.psc) as u64;
        let arr = read_mmio_u32(&self.mmio, regs.arr) as u64;
        let cnt = read_mmio_u32(&self.mmio, regs.cnt) as u64;

        let counter_hz = (tim_clk_hz / (psc + 1)).max(1);
        let numer = self.timers[index]
            .tick_residual
            .saturating_add(emu_core_cycles.saturating_mul(counter_hz));
        let ticks = numer / core_hz;
        self.timers[index].tick_residual = numer % core_hz;
        if ticks == 0 {
            return;
        }

        let period = arr.saturating_add(1).max(1);
        let total = cnt.saturating_add(ticks);
        let wraps = total / period;
        let new_cnt = total % period;
        write_mmio_u32(&mut self.mmio, regs.cnt, new_cnt as u32);

        if wraps != 0 {
            let sr = read_mmio_u32(&self.mmio, regs.sr) | 0x1;
            write_mmio_u32(&mut self.mmio, regs.sr, sr);
            if let Some(event) = decode_mmio_write(&self.register_meta, regs.sr, 4, sr) {
                self.mmio_writes.push(event);
            }

            let dier = read_mmio_u32(&self.mmio, regs.dier);
            if (dier & 0x1) != 0 && let Some(irq) = irq {
                nvic_pending_write(&mut self.mmio, irq, true);
            }
        }
    }

    fn compute_tim_clock_hz(&self) -> (u64, u64) {
        let default_hz = self.target.core_clock_hz.max(1) as u64;
        let Some(rcc) = self.rcc_regs else {
            return (default_hz, default_hz);
        };
        let cr = read_mmio_u32(&self.mmio, rcc.cr);
        let cfgr = read_mmio_u32(&self.mmio, rcc.cfgr);
        let hsi_hz = 8_000_000u64;
        let hse_hz = 8_000_000u64;

        let pll_mul = match (cfgr >> 18) & 0xF {
            0..=13 => ((cfgr >> 18) & 0xF) as u64 + 2,
            _ => 16,
        };
        let pll_src_hz = if (cfgr >> 16) & 1 == 0 {
            hsi_hz / 2
        } else if (cfgr >> 17) & 1 == 0 {
            hse_hz
        } else {
            hse_hz / 2
        };
        let pll_ready = (cr >> 25) & 1 == 1;
        let hse_ready = (cr >> 17) & 1 == 1;
        let sysclk = match cfgr & 0x3 {
            0b00 => hsi_hz,
            0b01 => {
                if hse_ready {
                    hse_hz
                } else {
                    hsi_hz
                }
            }
            0b10 => {
                if pll_ready {
                    pll_src_hz.saturating_mul(pll_mul)
                } else {
                    hsi_hz
                }
            }
            _ => hsi_hz,
        };

        let ahb_div = match (cfgr >> 4) & 0xF {
            0..=7 => 1,
            8 => 2,
            9 => 4,
            10 => 8,
            11 => 16,
            12 => 64,
            13 => 128,
            14 => 256,
            _ => 512,
        } as u64;
        let ppre1_div = match (cfgr >> 8) & 0x7 {
            0..=3 => 1,
            4 => 2,
            5 => 4,
            6 => 8,
            _ => 16,
        } as u64;
        let hclk = (sysclk / ahb_div.max(1)).max(1);
        let pclk1 = (hclk / ppre1_div.max(1)).max(1);
        let tim_clk = if ppre1_div == 1 {
            pclk1
        } else {
            pclk1.saturating_mul(2)
        };
        (hclk, tim_clk.max(1))
    }

    fn next_pending_enabled_irq(&self) -> Option<u8> {
        let pending0 = read_mmio_u32(&self.mmio, 0xE000_E200);
        let pending1 = read_mmio_u32(&self.mmio, 0xE000_E204);
        if pending0 == 0 && pending1 == 0 {
            return None;
        }

        let enabled0 = read_mmio_u32(&self.mmio, 0xE000_E100);
        let enabled1 = read_mmio_u32(&self.mmio, 0xE000_E104);

        let ready0 = enabled0 & pending0;
        if ready0 != 0 {
            return Some(ready0.trailing_zeros() as u8);
        }

        let ready1 = enabled1 & pending1;
        if ready1 != 0 {
            return Some(32 + ready1.trailing_zeros() as u8);
        }

        None
    }

    fn timer_clock_enabled(&self, peripheral: &str) -> bool {
        let Some(rcc) = self.rcc_regs else {
            return true;
        };
        let Some((reg_addr, bit)) = timer_enable_bit(peripheral, rcc) else {
            return true;
        };
        let reg = read_mmio_u32(&self.mmio, reg_addr);
        (reg & (1u32 << bit)) != 0
    }
}

struct MachineBus<'a> {
    memory: &'a mut [MemoryBlock],
    flash_base: u64,
    flash_alias_base: Option<u64>,
    periph_bb_base: Option<u64>,
    periph_bb_alias_start: Option<u64>,
    periph_bb_alias_end: Option<u64>,
    mmio: &'a mut HashMap<u64, u32>,
    register_meta: &'a HashMap<u64, RegisterMeta>,
    usart_rx_queues: &'a mut HashMap<u64, VecDeque<u8>>,
    serial_output: &'a mut Vec<SerialEvent>,
    mmio_writes: &'a mut Vec<MmioWriteEvent>,
    systick: &'a mut SystickState,
    nvic_any_enabled: &'a mut bool,
}

impl SystemBus for MachineBus<'_> {
    #[inline]
    fn read8(&mut self, addr: u64) -> Result<u8, String> {
        // Fast path: memory blocks (flash/RAM live below 0x4000_0000)
        if addr < 0x4000_0000 {
            for block in self.memory.iter() {
                if let Some(value) = block.read8(addr) {
                    return Ok(value);
                }
            }
            if let Some(value) =
                flash_alias_read(self.memory, self.flash_base, self.flash_alias_base, addr)
            {
                return Ok(value);
            }
        }

        if let Some(meta) = self.register_meta.get(&addr) {
            if let Some(reg) = meta.systick {
                return Ok(self.systick.read8(reg, meta.byte_offset));
            }
            if &*meta.peripheral == "RCC" {
                if &*meta.register == "CR"
                    || &*meta.register == "CSR"
                    || &*meta.register == "BDCR"
                {
                    let base_addr = addr & !3;
                    let raw = read_mmio_u32(self.mmio, base_addr);
                    let patched = apply_rcc_ready_flags(base_addr, raw, raw);
                    let byte = ((patched >> ((meta.byte_offset as u32) * 8)) & 0xFF) as u8;
                    return Ok(byte);
                }
            }
            if is_usart_status(meta) {
                let has_rx = self
                    .usart_rx_queues
                    .get(&meta.paired_addr)
                    .is_some_and(|q| !q.is_empty());
                let sr = 0x0000_00C0u32 | if has_rx { 1 << 5 } else { 0 };
                return Ok(((sr >> ((meta.byte_offset as u32) * 8)) & 0xFF) as u8);
            }
            if is_usart_data(meta) && meta.byte_offset == 0 {
                if let Some(queue) = self.usart_rx_queues.get_mut(&meta.paired_addr)
                    && let Some(byte) = queue.pop_front()
                {
                    mmio_write_byte(self.mmio, addr, byte);
                    return Ok(byte);
                }
                return Ok(mmio_read_byte(self.mmio, addr));
            }
            if is_spi_status(meta) {
                let sr = spi_sr_sanitized(self.mmio, meta.paired_addr);
                let byte = ((sr >> ((meta.byte_offset as u32) * 8)) & 0xFF) as u8;
                return Ok(byte);
            }
            if is_spi_data(meta) && meta.byte_offset == 0 {
                let value = mmio_read_byte(self.mmio, addr);
                spi_mark_data_consumed(self.mmio, meta.paired_addr);
                return Ok(value);
            }
        }

        if is_peripheral_addr(addr) {
            Ok(mmio_read_byte(self.mmio, addr))
        } else {
            Err(format!("read from unmapped address 0x{addr:08x}"))
        }
    }

    fn read_block(&mut self, addr: u64, buf: &mut [u8]) -> Result<(), String> {
        if addr < 0x4000_0000 {
            for block in self.memory.iter() {
                if block.contains(addr) && block.contains(addr + buf.len() as u64 - 1) {
                    let offset = (addr - block.base()) as usize;
                    buf.copy_from_slice(&block.data()[offset..offset + buf.len()]);
                    return Ok(());
                }
            }
        }
        for (i, byte) in buf.iter_mut().enumerate() {
            *byte = self.read8(addr + i as u64)?;
        }
        Ok(())
    }

    fn write_block(&mut self, addr: u64, buf: &[u8]) -> Result<(), String> {
        if addr < 0x4000_0000 {
            for block in self.memory.iter_mut() {
                if block.writable() && block.contains(addr) && block.contains(addr + buf.len() as u64 - 1) {
                    let offset = (addr - block.base()) as usize;
                    block.data_mut()[offset..offset + buf.len()].copy_from_slice(buf);
                    return Ok(());
                }
            }
        }
        for (i, &byte) in buf.iter().enumerate() {
            self.write8(addr + i as u64, byte)?;
        }
        Ok(())
    }

    #[inline]
    fn read16(&mut self, addr: u64) -> Result<u16, String> {
        // Fast path: native 16-bit read from memory blocks
        if addr < 0x4000_0000 {
            for block in self.memory.iter() {
                if let Some(value) = block.read16(addr) {
                    return Ok(value);
                }
            }
            if let Some(value) =
                flash_alias_read16(self.memory, self.flash_base, self.flash_alias_base, addr)
            {
                return Ok(value);
            }
        }
        // Fallback to 2x read8 for MMIO
        let lo = self.read8(addr)? as u16;
        let hi = self.read8(addr + 1)? as u16;
        Ok(lo | (hi << 8))
    }

    fn write8(&mut self, addr: u64, value: u8) -> Result<(), String> {
        // Fast path: memory blocks
        if addr < 0x4000_0000
            && let Some(block) = self.memory.iter_mut().find(|block| block.contains(addr))
        {
            return block.write8(addr, value);
        }

        if write_special_mmio(
            self.mmio,
            self.serial_output,
            self.mmio_writes,
            self.register_meta,
            self.systick,
            addr,
            value,
        ) {
            if is_nvic_enable_addr(addr) {
                *self.nvic_any_enabled = nvic_any_enabled(self.mmio);
            }
            return Ok(());
        }
        if write_periph_bitband_alias(
            self.mmio,
            self.mmio_writes,
            self.register_meta,
            addr,
            value as u32,
            self.periph_bb_base,
            self.periph_bb_alias_start,
            self.periph_bb_alias_end,
        ) {
            return Ok(());
        }

        if is_peripheral_addr(addr) {
            mmio_write_byte(self.mmio, addr, value);
            if let Some(event) = decode_mmio_write(self.register_meta, addr, 1, value as u32) {
                self.mmio_writes.push(event);
            }
            if is_nvic_enable_addr(addr) {
                *self.nvic_any_enabled = nvic_any_enabled(self.mmio);
            }
            Ok(())
        } else {
            Err(format!("write to unmapped address 0x{addr:08x}"))
        }
    }

    #[inline]
    fn write16(&mut self, addr: u64, value: u16) -> Result<(), String> {
        // Fast path: native 16-bit write to memory blocks
        if addr < 0x4000_0000
            && let Some(block) = self.memory.iter_mut().find(|block| block.contains(addr))
        {
            return block.write16(addr, value);
        }
        // For word-aligned MMIO, try atomic word write first.
        // Splitting into 2× write8 causes side effects (serial output,
        // SPI data-written flag, etc.) to fire twice per logical write.
        if addr & 3 == 0 {
            if write_special_mmio_word(
                self.mmio,
                self.serial_output,
                self.mmio_writes,
                self.register_meta,
                self.systick,
                addr,
                value as u32,
            ) {
                if is_nvic_enable_addr(addr) {
                    *self.nvic_any_enabled = nvic_any_enabled(self.mmio);
                }
                return Ok(());
            }
        }
        // Fallback to 2x write8 for non-special or non-aligned MMIO
        self.write8(addr, (value & 0xFF) as u8)?;
        self.write8(addr + 1, (value >> 8) as u8)
    }

    #[inline]
    fn read32(&mut self, addr: u64) -> Result<u32, String> {
        // Fast path: native 32-bit read from memory blocks
        if addr < 0x4000_0000 {
            for block in self.memory.iter() {
                if let Some(value) = block.read32(addr) {
                    return Ok(value);
                }
            }
            if let Some(value) =
                flash_alias_read32(self.memory, self.flash_base, self.flash_alias_base, addr)
            {
                return Ok(value);
            }
        }

        if let Some(meta) = self.register_meta.get(&addr)
            && meta.byte_offset == 0
        {
            if let Some(reg) = meta.systick {
                return Ok(self.systick.read32(reg));
            }
            if &*meta.peripheral == "RCC" {
                if &*meta.register == "CR"
                    || &*meta.register == "CSR"
                    || &*meta.register == "BDCR"
                {
                    let raw = read_mmio_u32(self.mmio, addr);
                    return Ok(apply_rcc_ready_flags(addr, raw, raw));
                }
            }
            if is_usart_status(meta) {
                let has_rx = self
                    .usart_rx_queues
                    .get(&meta.paired_addr)
                    .is_some_and(|q| !q.is_empty());
                return Ok(0x0000_00C0u32 | if has_rx { 1 << 5 } else { 0 });
            }
            if is_usart_data(meta) {
                let mut word = read_mmio_u32(self.mmio, addr);
                if let Some(queue) = self.usart_rx_queues.get_mut(&meta.paired_addr)
                    && let Some(byte) = queue.pop_front()
                {
                    word = (word & !0xFF) | u32::from(byte);
                    mmio_write_byte(self.mmio, addr, byte);
                }
                return Ok(word);
            }
            if is_spi_status(meta) {
                return Ok(spi_sr_sanitized(self.mmio, meta.paired_addr));
            }
            if is_spi_data(meta) {
                let val = read_mmio_u32(self.mmio, addr);
                spi_mark_data_consumed(self.mmio, meta.paired_addr);
                return Ok(val);
            }
        }

        if is_peripheral_addr(addr) {
            let val = read_mmio_u32(self.mmio, addr);
            trace!("mmio.read32 @ 0x{:08x} -> 0x{:08x}", addr, val);
            return Ok(val);
        }

        Err(format!("read from unmapped address 0x{addr:08x}"))
    }

    fn write32(&mut self, addr: u64, value: u32) -> Result<(), String> {
        // Fast path: native 32-bit write to memory blocks
        if addr < 0x4000_0000
            && let Some(block) = self.memory.iter_mut().find(|block| block.contains(addr))
        {
            return block.write32(addr, value);
        }

        if write_special_mmio_word(
            self.mmio,
            self.serial_output,
            self.mmio_writes,
            self.register_meta,
            self.systick,
            addr,
            value,
        ) {
            if is_nvic_enable_word_addr(addr) {
                *self.nvic_any_enabled = nvic_any_enabled(self.mmio);
            }
            return Ok(());
        }
        if write_periph_bitband_alias(
            self.mmio,
            self.mmio_writes,
            self.register_meta,
            addr,
            value,
            self.periph_bb_base,
            self.periph_bb_alias_start,
            self.periph_bb_alias_end,
        ) {
            return Ok(());
        }

        // Memory fallback for aliased addresses
        if let Some(block) = self.memory.iter_mut().find(|block| block.contains(addr)) {
            return block.write32(addr, value);
        }

        if is_peripheral_addr(addr) {
            write_mmio_u32(self.mmio, addr, value);
            if let Some(event) = decode_mmio_write(self.register_meta, addr, 4, value) {
                self.mmio_writes.push(event);
            }
            if is_nvic_enable_word_addr(addr) {
                *self.nvic_any_enabled = nvic_any_enabled(self.mmio);
            }
            return Ok(());
        }

        Err(format!("write to unmapped address 0x{addr:08x}"))
    }
}

fn flash_alias_read(
    blocks: &[MemoryBlock],
    flash_base: u64,
    flash_alias_base: Option<u64>,
    addr: u64,
) -> Option<u8> {
    let flash_alias_base = flash_alias_base?;
    blocks.iter().find_map(|block| {
        if !block.writable()
            && addr >= flash_alias_base
            && addr < flash_alias_base + block.len() as u64
        {
            block.read8(flash_base + (addr - flash_alias_base))
        } else {
            None
        }
    })
}

fn flash_alias_read16(
    blocks: &[MemoryBlock],
    flash_base: u64,
    flash_alias_base: Option<u64>,
    addr: u64,
) -> Option<u16> {
    let flash_alias_base = flash_alias_base?;
    blocks.iter().find_map(|block| {
        if !block.writable()
            && addr >= flash_alias_base
            && addr + 1 < flash_alias_base + block.len() as u64
        {
            block.read16(flash_base + (addr - flash_alias_base))
        } else {
            None
        }
    })
}

fn flash_alias_read32(
    blocks: &[MemoryBlock],
    flash_base: u64,
    flash_alias_base: Option<u64>,
    addr: u64,
) -> Option<u32> {
    let flash_alias_base = flash_alias_base?;
    blocks.iter().find_map(|block| {
        if !block.writable()
            && addr >= flash_alias_base
            && addr + 3 < flash_alias_base + block.len() as u64
        {
            block.read32(flash_base + (addr - flash_alias_base))
        } else {
            None
        }
    })
}

fn is_peripheral_addr(addr: u64) -> bool {
    (0x4000_0000..0xF000_0000).contains(&addr)
}

fn is_nvic_enable_addr(addr: u64) -> bool {
    (0xE000_E100..=0xE000_E107).contains(&addr)
}

fn is_nvic_enable_word_addr(addr: u64) -> bool {
    addr == 0xE000_E100 || addr == 0xE000_E104
}

fn nvic_any_enabled(mmio: &HashMap<u64, u32>) -> bool {
    read_mmio_u32(mmio, 0xE000_E100) != 0 || read_mmio_u32(mmio, 0xE000_E104) != 0
}

/// Read a 32-bit value from the word-aligned mmio map.
#[inline]
fn read_mmio_u32(mmio: &HashMap<u64, u32>, addr: u64) -> u32 {
    mmio.get(&(addr & !3)).copied().unwrap_or(0)
}

/// Write a 32-bit value to the word-aligned mmio map.
#[inline]
fn write_mmio_u32(mmio: &mut HashMap<u64, u32>, addr: u64, value: u32) {
    mmio.insert(addr & !3, value);
}

/// Read a single byte from the word-aligned mmio map.
#[inline]
fn mmio_read_byte(mmio: &HashMap<u64, u32>, addr: u64) -> u8 {
    let shift = (addr & 3) * 8;
    ((mmio.get(&(addr & !3)).copied().unwrap_or(0) >> shift) & 0xFF) as u8
}

/// Write a single byte into the word-aligned mmio map (read-modify-write).
#[inline]
fn mmio_write_byte(mmio: &mut HashMap<u64, u32>, addr: u64, value: u8) {
    let word_addr = addr & !3;
    let shift = (addr & 3) * 8;
    let entry = mmio.entry(word_addr).or_insert(0);
    *entry = (*entry & !(0xFF << shift)) | ((value as u32) << shift);
}

fn nvic_pending_read(mmio: &HashMap<u64, u32>, irq: u8) -> bool {
    let reg = 0xE000_E200u64 + (u64::from(irq / 32) * 4);
    let bit = irq % 32;
    (read_mmio_u32(mmio, reg) & (1u32 << bit)) != 0
}

fn nvic_pending_write(mmio: &mut HashMap<u64, u32>, irq: u8, pending: bool) {
    let reg = 0xE000_E200u64 + (u64::from(irq / 32) * 4);
    let bit = irq % 32;
    let mut value = read_mmio_u32(mmio, reg);
    if pending {
        value |= 1u32 << bit;
    } else {
        value &= !(1u32 << bit);
    }
    write_mmio_u32(mmio, reg, value);
}

fn resolve_register_addr(target: &TargetSpec, peripheral: &str, register: &str) -> Option<u64> {
    target
        .peripherals
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(peripheral))
        .and_then(|p| {
            p.registers
                .iter()
                .find(|r| r.name.eq_ignore_ascii_case(register))
                .map(|r| r.address)
        })
}

fn resolve_timer_models(target: &TargetSpec) -> Vec<TimerModel> {
    let mut models = Vec::new();
    for peripheral in &target.peripherals {
        if !peripheral.name.starts_with("TIM") {
            continue;
        }
        let regs = TimerRegs {
            cr1: match resolve_register_addr(target, &peripheral.name, "CR1") {
                Some(v) => v,
                None => continue,
            },
            dier: match resolve_register_addr(target, &peripheral.name, "DIER") {
                Some(v) => v,
                None => continue,
            },
            sr: match resolve_register_addr(target, &peripheral.name, "SR") {
                Some(v) => v,
                None => continue,
            },
            cnt: match resolve_register_addr(target, &peripheral.name, "CNT") {
                Some(v) => v,
                None => continue,
            },
            psc: match resolve_register_addr(target, &peripheral.name, "PSC") {
                Some(v) => v,
                None => continue,
            },
            arr: match resolve_register_addr(target, &peripheral.name, "ARR") {
                Some(v) => v,
                None => continue,
            },
        };
        models.push(TimerModel {
            peripheral: peripheral.name.clone(),
            regs,
            irq: timer_irq_number(&peripheral.name),
            tick_residual: 0,
        });
    }
    models
}

fn resolve_rcc_regs(target: &TargetSpec) -> Option<RccRegs> {
    Some(RccRegs {
        cr: resolve_register_addr(target, "RCC", "CR")?,
        cfgr: resolve_register_addr(target, "RCC", "CFGR")?,
        apb1enr: resolve_register_addr(target, "RCC", "APB1ENR"),
    })
}

fn timer_irq_number(peripheral: &str) -> Option<u8> {
    match peripheral {
        "TIM1" => Some(25),
        "TIM2" => Some(28),
        "TIM3" => Some(29),
        "TIM4" => Some(30),
        "TIM5" => Some(50),
        "TIM6" => Some(54),
        "TIM7" => Some(55),
        _ => None,
    }
}

fn timer_enable_bit(peripheral: &str, rcc: RccRegs) -> Option<(u64, u8)> {
    match peripheral {
        "TIM2" => rcc.apb1enr.map(|addr| (addr, 0)),
        "TIM3" => rcc.apb1enr.map(|addr| (addr, 1)),
        "TIM4" => rcc.apb1enr.map(|addr| (addr, 2)),
        "TIM5" => rcc.apb1enr.map(|addr| (addr, 3)),
        "TIM6" => rcc.apb1enr.map(|addr| (addr, 4)),
        "TIM7" => rcc.apb1enr.map(|addr| (addr, 5)),
        _ => None,
    }
}

/// Handle 8-bit special MMIO write by merging into word and delegating to the
/// unified `write_special_mmio_word`.
fn write_special_mmio(
    mmio: &mut HashMap<u64, u32>,
    serial_output: &mut Vec<SerialEvent>,
    mmio_writes: &mut Vec<MmioWriteEvent>,
    register_meta: &HashMap<u64, RegisterMeta>,
    systick: &mut SystickState,
    addr: u64,
    value: u8,
) -> bool {
    // SysTick byte writes need byte-level precision (different reg per byte_offset).
    if let Some(meta) = register_meta.get(&addr) {
        if let Some(reg) = meta.systick {
            systick.write8(reg, meta.byte_offset, value);
            mmio_write_byte(mmio, addr, value);
            if let Some(event) = decode_mmio_write(register_meta, addr, 1, value as u32) {
                mmio_writes.push(event);
            }
            return true;
        }
    }

    // For everything else, merge the byte into the containing word and delegate.
    let word_addr = addr & !3;
    let old_word = read_mmio_u32(mmio, word_addr);
    let shift = (addr & 3) * 8;
    let new_word = (old_word & !(0xFF << shift)) | ((value as u32) << shift);
    write_special_mmio_word(
        mmio,
        serial_output,
        mmio_writes,
        register_meta,
        systick,
        word_addr,
        new_word,
    )
}

/// Unified special MMIO write (always word-aligned, 32-bit value).
fn write_special_mmio_word(
    mmio: &mut HashMap<u64, u32>,
    serial_output: &mut Vec<SerialEvent>,
    mmio_writes: &mut Vec<MmioWriteEvent>,
    register_meta: &HashMap<u64, RegisterMeta>,
    systick: &mut SystickState,
    addr: u64,
    value: u32,
) -> bool {
    if let Some(meta) = register_meta.get(&addr) {
        if is_rcc_control(meta) || is_rcc_csr(meta) || is_pwr_csr(meta) {
            let old_val = read_mmio_u32(mmio, addr);
            let updated = apply_rcc_ready_flags(addr, old_val, value);
            write_mmio_u32(mmio, addr, updated);
            if let Some(event) = decode_mmio_write(register_meta, addr, 4, updated) {
                mmio_writes.push(event);
            }
            return true;
        }
        if is_rcc_cfgr(meta) {
            let updated = apply_rcc_cfgr_flags(value);
            write_mmio_u32(mmio, addr, updated);
            if let Some(event) = decode_mmio_write(register_meta, addr, 4, updated) {
                mmio_writes.push(event);
            }
            return true;
        }

        if let Some(reg) = meta.systick {
            if meta.byte_offset == 0 {
                systick.write32(reg, value);
                write_mmio_u32(mmio, addr, value);
                if let Some(event) = decode_mmio_write(register_meta, addr, 4, value) {
                    mmio_writes.push(event);
                }
                return true;
            }
        }

        if meta.byte_offset == 0 {
            if is_timer_status(meta) {
                let current = read_mmio_u32(mmio, addr);
                let merged = current & value;
                write_mmio_u32(mmio, addr, merged);
                if let Some(event) = decode_mmio_write(register_meta, addr, 4, merged) {
                    mmio_writes.push(event);
                }
                return true;
            }
            if is_gpio_bsrr(meta) {
                write_mmio_u32(mmio, addr, value);
                if let Some(event) = decode_mmio_write(register_meta, addr, 4, value) {
                    mmio_writes.push(event);
                }
                if meta.paired_addr != 0 {
                    let odr_addr = meta.paired_addr;
                    let odr = read_mmio_u32(mmio, odr_addr);
                    let set_mask = value & 0xFFFF;
                    let reset_mask = (value >> 16) & 0xFFFF;
                    let next = (odr | set_mask) & !reset_mask;
                    write_mmio_u32(mmio, odr_addr, next);
                    if let Some(event) = decode_mmio_write(register_meta, odr_addr, 4, next) {
                        mmio_writes.push(event);
                    }
                }
                return true;
            }
            if is_usart_data(meta) {
                serial_output.push(SerialEvent {
                    peripheral: Arc::clone(&meta.peripheral),
                    byte: (value & 0xFF) as u8,
                });
                if let Some(event) = decode_mmio_write(register_meta, addr, 4, value) {
                    mmio_writes.push(event);
                }
                write_mmio_u32(mmio, addr, value);
                return true;
            }
            if is_spi_data(meta) {
                write_mmio_u32(mmio, addr, value);
                if let Some(event) = decode_mmio_write(register_meta, addr, 4, value) {
                    mmio_writes.push(event);
                }
                spi_mark_data_written(mmio, meta.paired_addr);
                return true;
            }
        }
    }

    false
}

#[allow(clippy::too_many_arguments)]
fn write_periph_bitband_alias(
    mmio: &mut HashMap<u64, u32>,
    mmio_writes: &mut Vec<MmioWriteEvent>,
    register_meta: &HashMap<u64, RegisterMeta>,
    alias_addr: u64,
    value: u32,
    periph_bb_base: Option<u64>,
    periph_bb_alias_start: Option<u64>,
    periph_bb_alias_end: Option<u64>,
) -> bool {
    let (Some(periph_bb_base), Some(periph_bb_alias_start), Some(periph_bb_alias_end)) =
        (periph_bb_base, periph_bb_alias_start, periph_bb_alias_end)
    else {
        return false;
    };

    if !(periph_bb_alias_start..periph_bb_alias_end).contains(&alias_addr) {
        return false;
    }

    let bit_word_offset = alias_addr - periph_bb_alias_start;
    let byte_offset = bit_word_offset / 32;
    let bit = ((bit_word_offset % 32) / 4) as u8;
    let target_addr = periph_bb_base + byte_offset;
    let current = mmio_read_byte(mmio, target_addr);
    let bit_mask = 1u8 << bit;
    let updated = if value & 1 == 0 {
        current & !bit_mask
    } else {
        current | bit_mask
    };
    mmio_write_byte(mmio, target_addr, updated);

    if let Some(event) = decode_mmio_write(register_meta, target_addr, 1, updated as u32) {
        mmio_writes.push(event);
    }

    true
}

fn is_usart_status(meta: &RegisterMeta) -> bool {
    (meta.flags & META_USART_STATUS) != 0
}

fn is_usart_data(meta: &RegisterMeta) -> bool {
    (meta.flags & META_USART_DATA) != 0
}

fn is_spi_status(meta: &RegisterMeta) -> bool {
    (meta.flags & META_SPI_STATUS) != 0
}

fn is_spi_data(meta: &RegisterMeta) -> bool {
    (meta.flags & META_SPI_DATA) != 0
}

fn is_rcc_control(meta: &RegisterMeta) -> bool {
    (meta.flags & META_RCC_CONTROL) != 0
}

fn is_timer_status(meta: &RegisterMeta) -> bool {
    (meta.flags & META_TIMER_STATUS) != 0
}

fn is_gpio_bsrr(meta: &RegisterMeta) -> bool {
    (meta.flags & META_GPIO_BSRR) != 0
}

fn is_rcc_cfgr(meta: &RegisterMeta) -> bool {
    (meta.flags & META_RCC_CFGR) != 0
}

fn is_rcc_csr(meta: &RegisterMeta) -> bool {
    (meta.flags & META_RCC_CSR) != 0
}

fn is_pwr_csr(meta: &RegisterMeta) -> bool {
    (meta.flags & META_PWR_CSR) != 0
}

fn classify_meta_flags(peripheral: &str, register: &str) -> u16 {
    let mut flags = 0u16;
    let is_uart_like =
        peripheral.starts_with("USART") || peripheral.starts_with("UART");
    if is_uart_like && register.eq_ignore_ascii_case("SR") {
        flags |= META_USART_STATUS;
    }
    if is_uart_like && register.eq_ignore_ascii_case("DR") {
        flags |= META_USART_DATA;
    }
    if peripheral.starts_with("SPI") && register.eq_ignore_ascii_case("SR") {
        flags |= META_SPI_STATUS;
    }
    if peripheral.starts_with("SPI") && register.eq_ignore_ascii_case("DR") {
        flags |= META_SPI_DATA;
    }
    if peripheral.eq_ignore_ascii_case("RCC") && register.eq_ignore_ascii_case("CR") {
        flags |= META_RCC_CONTROL;
    }
    if peripheral.eq_ignore_ascii_case("RCC") && register.eq_ignore_ascii_case("CFGR") {
        flags |= META_RCC_CFGR;
    }
    if peripheral.eq_ignore_ascii_case("RCC") && register.eq_ignore_ascii_case("CSR") {
        flags |= META_RCC_CSR;
    }
    if peripheral.eq_ignore_ascii_case("PWR") && register.eq_ignore_ascii_case("CSR") {
        flags |= META_PWR_CSR;
    }
    if peripheral.starts_with("TIM") && register.eq_ignore_ascii_case("SR") {
        flags |= META_TIMER_STATUS;
    }
    if peripheral.starts_with("GPIO") && register.eq_ignore_ascii_case("BSRR") {
        flags |= META_GPIO_BSRR;
    }
    if peripheral.starts_with("GPIO") && register.eq_ignore_ascii_case("ODR") {
        flags |= META_GPIO_ODR;
    }
    if peripheral.starts_with("I2C") && register.eq_ignore_ascii_case("CR1") {
        flags |= META_I2C_CR1;
    }
    if peripheral.starts_with("I2C") && register.eq_ignore_ascii_case("DR") {
        flags |= META_I2C_DR;
    }
    flags
}

fn classify_systick_reg(peripheral: &str, register: &str) -> Option<SystickReg> {
    let p = peripheral.to_ascii_uppercase();
    let r = register.to_ascii_uppercase();
    if !(p == "STK" || p == "SYST" || p.contains("SYST")) {
        return None;
    }
    if r == "CTRL" {
        return Some(SystickReg::Ctrl);
    }
    if r.contains("LOAD") {
        return Some(SystickReg::Load);
    }
    if r == "VAL" || r.contains("CVR") {
        return Some(SystickReg::Val);
    }
    if r.contains("CALIB") {
        return Some(SystickReg::Calib);
    }
    None
}

fn spi_sr_sanitized(mmio: &HashMap<u64, u32>, sr_addr: u64) -> u32 {
    let mut sr = if sr_addr == 0 {
        0
    } else {
        read_mmio_u32(mmio, sr_addr)
    };
    // Keep SPI master write path non-blocking for now:
    // TXE=1, BSY=0, and never expose sticky error flags unless modeled explicitly.
    sr |= 1 << 1; // TXE
    sr &= !(1 << 7); // BSY
    sr &= !((1 << 6) | (1 << 5) | (1 << 4)); // OVR/MODF/CRCERR
    sr
}

fn spi_mark_data_written(mmio: &mut HashMap<u64, u32>, sr_addr: u64) {
    if sr_addr != 0 {
        let mut sr = read_mmio_u32(mmio, sr_addr);
        sr |= (1 << 1) | (1 << 0); // TXE + RXNE
        sr &= !(1 << 7); // BSY cleared for now
        sr &= !((1 << 6) | (1 << 5) | (1 << 4)); // clear error bits
        write_mmio_u32(mmio, sr_addr, sr);
    }
}

fn spi_mark_data_consumed(mmio: &mut HashMap<u64, u32>, sr_addr: u64) {
    if sr_addr != 0 {
        let mut sr = read_mmio_u32(mmio, sr_addr);
        sr &= !(1 << 0); // RXNE cleared on DR read
        sr |= 1 << 1; // TXE stays set
        sr &= !(1 << 7); // BSY cleared
        sr &= !((1 << 6) | (1 << 5) | (1 << 4)); // clear error bits
        write_mmio_u32(mmio, sr_addr, sr);
    }
}

fn apply_rcc_ready_flags(addr: u64, _old_val: u32, mut new_val: u32) -> u32 {
    // Aggressively set RDY bits for common RCC/PWR registers to bypass HAL init loops.
    
    if addr & !3 == 0x40023800 { // RCC_CR
        if (new_val & (1 << 0)) != 0 { new_val |= 1 << 1; } else { new_val &= !(1 << 1); }   // HSIRDY
        if (new_val & (1 << 16)) != 0 { new_val |= 1 << 17; } else { new_val &= !(1 << 17); } // HSERDY
        if (new_val & (1 << 24)) != 0 { new_val |= 1 << 25; } else { new_val &= !(1 << 25); } // PLLRDY
        if (new_val & (1 << 26)) != 0 { new_val |= 1 << 27; } else { new_val &= !(1 << 27); } // PLLI2SRDY
        if (new_val & (1 << 28)) != 0 { new_val |= 1 << 29; } else { new_val &= !(1 << 29); } // PLLSAIRDY
    } else if addr & !3 == 0x40023874 { // RCC_CSR
        if (new_val & (1 << 0)) != 0 { new_val |= 1 << 1; } else { new_val &= !(1 << 1); }   // LSIRDY
    } else if addr & !3 == 0x40023870 { // RCC_BDCR
        if (new_val & (1 << 0)) != 0 { new_val |= 1 << 1; } else { new_val &= !(1 << 1); }   // LSERDY
    } else if addr & !3 == 0x40007004 { // PWR_CSR
        new_val |= 1 << 14; // VOSRDY
    }

    new_val
}

fn apply_rcc_cfgr_flags(value: u32) -> u32 {
    let sw = value & 0x3;
    let sws = sw << 2;
    (value & !(0x3 << 2)) | sws
}

fn decode_mmio_write(
    register_meta: &HashMap<u64, RegisterMeta>,
    addr: u64,
    width: u8,
    value: u32,
) -> Option<MmioWriteEvent> {
    if let Some(meta) = register_meta.get(&addr) {
        return Some(MmioWriteEvent {
            peripheral: Arc::clone(&meta.peripheral),
            register: Arc::clone(&meta.register),
            addr,
            width,
            value,
            flags: meta.flags,
        });
    }
    None
}
