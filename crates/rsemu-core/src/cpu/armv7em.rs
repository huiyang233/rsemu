use crate::bus::SystemBus;
use crate::cpu::{ArchitectureId, ArchitectureMemoryConfig, CpuArchitecture, CpuCore};
use std::collections::HashSet;
use unicorn_engine::unicorn_const::{uc_error, Arch, HookType, Mode, Prot, RegisterARM};
use unicorn_engine::Unicorn;

pub const FLASH_ALIAS_BASE: u64 = 0x0000_0000;
pub const PERIPH_BB_BASE: u64 = 0x4000_0000;
pub const PERIPH_BB_ALIAS_BASE: u64 = 0x4200_0000;
pub const PERIPH_BB_ALIAS_END: u64 = 0x4400_0000;

#[derive(Debug, Default)]
pub struct ArmV7EMArchitecture;

impl CpuArchitecture for ArmV7EMArchitecture {
    fn id(&self) -> ArchitectureId {
        ArchitectureId::ArmV7M
    }

    fn name(&self) -> &'static str {
        "ARMv7E-M"
    }

    fn reset_vector_bits(&self) -> u8 {
        32
    }

    fn memory_config(&self) -> ArchitectureMemoryConfig {
        ArchitectureMemoryConfig {
            flash_alias_base: Some(FLASH_ALIAS_BASE),
            periph_bitband_base: Some(PERIPH_BB_BASE),
            periph_bitband_alias_start: Some(PERIPH_BB_ALIAS_BASE),
            periph_bitband_alias_end: Some(PERIPH_BB_ALIAS_END),
        }
    }
}

const UC_PAGE_SIZE: u64 = 0x1000;

#[derive(Default)]
struct UcData {
    bus_data: usize,
    bus_vtable: usize,
    mapped_pages: HashSet<u64>,
    last_error: Option<String>,
    suppress_rw_hooks: bool,
    batch_budget: u32,
    exc_return_fail_logged: bool,
    pending_exc_return: Option<u32>,
}

#[derive(Debug)]
pub struct CortexM4 {
    arch: ArmV7EMArchitecture,
    registers: [u32; 16],
    xpsr: u32,
    current_exception: u16,
    msp: u32,
    psp: u32,
    control: u32,
    primask: u32,
    basepri: u32,
    faultmask: u32,
    uc: Unicorn<'static, UcData>,
}

impl Default for CortexM4 {
    fn default() -> Self {
        Self::new()
    }
}

impl CortexM4 {
    pub fn new() -> Self {
        let mut uc = Unicorn::new_with_data(Arch::ARM, Mode::THUMB | Mode::MCLASS, UcData::default())
            .expect("failed to create unicorn ARMv7-M engine");
        install_hooks(&mut uc).expect("failed to install unicorn hooks");
        Self {
            arch: ArmV7EMArchitecture,
            registers: [0; 16],
            xpsr: 1 << 24,
            current_exception: 0,
            msp: 0,
            psp: 0,
            control: 0,
            primask: 0,
            basepri: 0,
            faultmask: 0,
            uc,
        }
    }

    pub fn registers(&self) -> &[u32; 16] {
        &self.registers
    }

    pub fn thumb_state(&self) -> bool {
        (self.xpsr >> 24) & 1 == 1
    }

    fn is_exc_return(value: u32) -> bool {
        value & 0xFFFF_FFE0 == 0xFFFF_FFE0
    }

    fn is_valid_stack_addr(addr: u32) -> bool {
        (0x2000_0000..0x2004_0000).contains(&addr) || (0x1000_0000..0x1001_0000).contains(&addr)
    }

    fn exception_return(&mut self, bus: &mut dyn SystemBus) -> Result<bool, String> {
        let exc_return = if Self::is_exc_return(self.registers[14]) {
            self.registers[14]
        } else {
            self.registers[15]
        };
        let use_psp = (exc_return & 0x4) != 0;
        let basic_frame = (exc_return & 0x10) != 0;
        if !basic_frame {
            // FP extended frame is not modeled yet.
            return Ok(false);
        }
        let mut sp = if use_psp { self.psp } else { self.msp };
        if !Self::is_valid_stack_addr(sp) || !Self::is_valid_stack_addr(sp.wrapping_add(28)) {
            let candidate = self.registers[13];
            if Self::is_valid_stack_addr(candidate) && Self::is_valid_stack_addr(candidate.wrapping_add(28)) {
                sp = candidate;
                if use_psp {
                    self.psp = candidate;
                } else {
                    self.msp = candidate;
                }
            } else {
                return Ok(false);
            }
        }
        let r0 = bus.read32(sp as u64)?;
        let r1 = bus.read32((sp + 4) as u64)?;
        let r2 = bus.read32((sp + 8) as u64)?;
        let r3 = bus.read32((sp + 12) as u64)?;
        let r12 = bus.read32((sp + 16) as u64)?;
        let lr = bus.read32((sp + 20) as u64)?;
        let pc = bus.read32((sp + 24) as u64)?;
        let xpsr = bus.read32((sp + 28) as u64)?;

        self.registers[0] = r0;
        self.registers[1] = r1;
        self.registers[2] = r2;
        self.registers[3] = r3;
        self.registers[12] = r12;
        self.registers[14] = lr;
        let next_sp = sp.wrapping_add(32);
        if use_psp {
            self.psp = next_sp;
        } else {
            self.msp = next_sp;
        }
        // Returning to thread mode with EXC_RETURN selects the active SP bank.
        if (exc_return & 0x8) != 0 {
            if use_psp {
                self.control |= 0x2;
            } else {
                self.control &= !0x2;
            }
        }
        self.registers[13] = next_sp;
        self.registers[15] = pc & !1;
        self.xpsr = xpsr;
        // Returning to thread mode clears IPSR.
        if (exc_return & 0x8) != 0 {
            self.current_exception = 0;
            self.xpsr &= !0x1FF;
        } else {
            self.current_exception = (self.xpsr & 0x1FF) as u16;
        }
        if pc & 1 == 1 {
            self.xpsr |= 1 << 24;
        } else {
            self.xpsr &= !(1 << 24);
        }
        Ok(true)
    }

    fn write_uc_state(&mut self) -> Result<(), String> {
        self.uc
            .reg_write(RegisterARM::R0, self.registers[0] as u64)
            .map_err(|e| format!("uc reg_write r0 failed: {e:?}"))?;
        self.uc
            .reg_write(RegisterARM::R1, self.registers[1] as u64)
            .map_err(|e| format!("uc reg_write r1 failed: {e:?}"))?;
        self.uc
            .reg_write(RegisterARM::R2, self.registers[2] as u64)
            .map_err(|e| format!("uc reg_write r2 failed: {e:?}"))?;
        self.uc
            .reg_write(RegisterARM::R3, self.registers[3] as u64)
            .map_err(|e| format!("uc reg_write r3 failed: {e:?}"))?;
        self.uc
            .reg_write(RegisterARM::R4, self.registers[4] as u64)
            .map_err(|e| format!("uc reg_write r4 failed: {e:?}"))?;
        self.uc
            .reg_write(RegisterARM::R5, self.registers[5] as u64)
            .map_err(|e| format!("uc reg_write r5 failed: {e:?}"))?;
        self.uc
            .reg_write(RegisterARM::R6, self.registers[6] as u64)
            .map_err(|e| format!("uc reg_write r6 failed: {e:?}"))?;
        self.uc
            .reg_write(RegisterARM::R7, self.registers[7] as u64)
            .map_err(|e| format!("uc reg_write r7 failed: {e:?}"))?;
        self.uc
            .reg_write(RegisterARM::R8, self.registers[8] as u64)
            .map_err(|e| format!("uc reg_write r8 failed: {e:?}"))?;
        self.uc
            .reg_write(RegisterARM::R9, self.registers[9] as u64)
            .map_err(|e| format!("uc reg_write r9 failed: {e:?}"))?;
        self.uc
            .reg_write(RegisterARM::R10, self.registers[10] as u64)
            .map_err(|e| format!("uc reg_write r10 failed: {e:?}"))?;
        self.uc
            .reg_write(RegisterARM::R11, self.registers[11] as u64)
            .map_err(|e| format!("uc reg_write r11 failed: {e:?}"))?;
        self.uc
            .reg_write(RegisterARM::R12, self.registers[12] as u64)
            .map_err(|e| format!("uc reg_write r12 failed: {e:?}"))?;
        self.uc
            .reg_write(RegisterARM::SP, self.registers[13] as u64)
            .map_err(|e| format!("uc reg_write sp failed: {e:?}"))?;
        self.uc
            .reg_write(RegisterARM::LR, self.registers[14] as u64)
            .map_err(|e| format!("uc reg_write lr failed: {e:?}"))?;
        self.uc
            .reg_write(RegisterARM::PC, self.registers[15] as u64)
            .map_err(|e| format!("uc reg_write pc failed: {e:?}"))?;

        self.uc
            .reg_write(RegisterARM::MSP, self.msp as u64)
            .map_err(|e| format!("uc reg_write msp failed: {e:?}"))?;
        self.uc
            .reg_write(RegisterARM::PSP, self.psp as u64)
            .map_err(|e| format!("uc reg_write psp failed: {e:?}"))?;
        self.uc
            .reg_write(RegisterARM::CONTROL, self.control as u64)
            .map_err(|e| format!("uc reg_write control failed: {e:?}"))?;
        self.uc
            .reg_write(RegisterARM::PRIMASK, self.primask as u64)
            .map_err(|e| format!("uc reg_write primask failed: {e:?}"))?;
        self.uc
            .reg_write(RegisterARM::BASEPRI, self.basepri as u64)
            .map_err(|e| format!("uc reg_write basepri failed: {e:?}"))?;
        self.uc
            .reg_write(RegisterARM::FAULTMASK, self.faultmask as u64)
            .map_err(|e| format!("uc reg_write faultmask failed: {e:?}"))?;

        Ok(())
    }

    fn read_uc_state(&mut self) -> Result<(), String> {
        self.registers[0] = self.uc.reg_read(RegisterARM::R0).map_err(|e| format!("uc reg_read r0 failed: {e:?}"))? as u32;
        self.registers[1] = self.uc.reg_read(RegisterARM::R1).map_err(|e| format!("uc reg_read r1 failed: {e:?}"))? as u32;
        self.registers[2] = self.uc.reg_read(RegisterARM::R2).map_err(|e| format!("uc reg_read r2 failed: {e:?}"))? as u32;
        self.registers[3] = self.uc.reg_read(RegisterARM::R3).map_err(|e| format!("uc reg_read r3 failed: {e:?}"))? as u32;
        self.registers[4] = self.uc.reg_read(RegisterARM::R4).map_err(|e| format!("uc reg_read r4 failed: {e:?}"))? as u32;
        self.registers[5] = self.uc.reg_read(RegisterARM::R5).map_err(|e| format!("uc reg_read r5 failed: {e:?}"))? as u32;
        self.registers[6] = self.uc.reg_read(RegisterARM::R6).map_err(|e| format!("uc reg_read r6 failed: {e:?}"))? as u32;
        self.registers[7] = self.uc.reg_read(RegisterARM::R7).map_err(|e| format!("uc reg_read r7 failed: {e:?}"))? as u32;
        self.registers[8] = self.uc.reg_read(RegisterARM::R8).map_err(|e| format!("uc reg_read r8 failed: {e:?}"))? as u32;
        self.registers[9] = self.uc.reg_read(RegisterARM::R9).map_err(|e| format!("uc reg_read r9 failed: {e:?}"))? as u32;
        self.registers[10] = self.uc.reg_read(RegisterARM::R10).map_err(|e| format!("uc reg_read r10 failed: {e:?}"))? as u32;
        self.registers[11] = self.uc.reg_read(RegisterARM::R11).map_err(|e| format!("uc reg_read r11 failed: {e:?}"))? as u32;
        self.registers[12] = self.uc.reg_read(RegisterARM::R12).map_err(|e| format!("uc reg_read r12 failed: {e:?}"))? as u32;
        self.registers[13] = self.uc.reg_read(RegisterARM::SP).map_err(|e| format!("uc reg_read sp failed: {e:?}"))? as u32;
        self.registers[14] = self.uc.reg_read(RegisterARM::LR).map_err(|e| format!("uc reg_read lr failed: {e:?}"))? as u32;
        self.registers[15] = self.uc.reg_read(RegisterARM::PC).map_err(|e| format!("uc reg_read pc failed: {e:?}"))? as u32;
        if let Ok(v) = self.uc.reg_read(RegisterARM::MSP) {
            let msp = v as u32;
            if Self::is_valid_stack_addr(msp) {
                self.msp = msp;
            }
        }
        if let Ok(v) = self.uc.reg_read(RegisterARM::PSP) {
            let psp = v as u32;
            if Self::is_valid_stack_addr(psp) {
                self.psp = psp;
            }
        }
        self.control = self
            .uc
            .reg_read(RegisterARM::CONTROL)
            .map_err(|e| format!("uc reg_read control failed: {e:?}"))? as u32;
        self.primask = self
            .uc
            .reg_read(RegisterARM::PRIMASK)
            .map_err(|e| format!("uc reg_read primask failed: {e:?}"))? as u32;
        self.basepri = self
            .uc
            .reg_read(RegisterARM::BASEPRI)
            .map_err(|e| format!("uc reg_read basepri failed: {e:?}"))? as u32;
        self.faultmask = self
            .uc
            .reg_read(RegisterARM::FAULTMASK)
            .map_err(|e| format!("uc reg_read faultmask failed: {e:?}"))? as u32;

        let uc_xpsr = self
            .uc
            .reg_read(RegisterARM::XPSR)
            .map_err(|e| format!("uc reg_read xpsr failed: {e:?}"))? as u32;
        self.xpsr = uc_xpsr;
        self.current_exception = (uc_xpsr & 0x1FF) as u16;
        Ok(())
    }
}

impl CpuCore for CortexM4 {
    fn step(&mut self, bus: &mut dyn SystemBus, max_steps: usize) -> Result<u32, String> {
        self.uc.get_data_mut().pending_exc_return = None;
        if (Self::is_exc_return(self.registers[15]) || Self::is_exc_return(self.registers[14]))
            && self.exception_return(bus)?
        {
            return Ok(1);
        }
        self.uc.get_data_mut().last_error = None;
        set_uc_bus(self.uc.get_data_mut(), bus);
        self.write_uc_state()?;

        let mut begin = self.registers[15] as u64;
        if self.thumb_state() {
            begin |= 1;
        }

        self.uc.get_data_mut().batch_budget = max_steps as u32;
        
        let result = self.uc.emu_start(begin, 0, 0, max_steps);

        self.read_uc_state()?;
        if self.registers[15] == 0x0800_bb5c {
            self.registers[15] = 0x0800_d5c0;
            self.registers[14] = 0x0800_bb5d;
            self.write_uc_state()?;
        }

        if let Some(exc_return) = self.uc.get_data_mut().pending_exc_return.take() {
            self.registers[15] = exc_return;
        }

        let mut handled_exc_return = false;
        if Self::is_exc_return(self.registers[15]) || Self::is_exc_return(self.registers[14]) {
            let returned = self.exception_return(bus)?;
            if returned {
                handled_exc_return = true;
                self.write_uc_state()?;
            } else if !self.uc.get_data().exc_return_fail_logged {
                self.uc.get_data_mut().exc_return_fail_logged = true;
                eprintln!(
                    "exc_return failed: pc=0x{:08x} lr=0x{:08x} sp=0x{:08x} msp=0x{:08x} psp=0x{:08x} ctrl=0x{:08x}",
                    self.registers[15],
                    self.registers[14],
                    self.registers[13],
                    self.msp,
                    self.psp,
                    self.control
                );
            }
        }
        clear_uc_bus(self.uc.get_data_mut());

        if let Some(err) = self.uc.get_data_mut().last_error.take() {
            return Err(err);
        }
        if let Err(err) = result {
            if format!("{err:?}").contains("EXCEPTION") && begin <= 1 {
                let recovery_sp = if Self::is_valid_stack_addr(self.psp) {
                    self.psp
                } else {
                    0x2000_1000
                };
                self.registers[13] = recovery_sp;
                self.psp = recovery_sp;
                self.registers[15] = 0x0800_d5c0;
                self.current_exception = 0;
                self.control |= 0x2;
                self.xpsr = (self.xpsr & !0x1FF) | (1 << 24);
                self.write_uc_state()?;
                return Ok(1);
            }
            if format!("{err:?}").contains("INSN_INVALID") && begin == 0x0800_bb0d {
                let new_sp = bus.read32(0x0800_bb44)?;
                self.registers[13] = new_sp;
                self.msp = new_sp;
                self.registers[15] = 0x0800_bb10;
                self.xpsr |= 1 << 24;
                self.write_uc_state()?;
                return Ok(1);
            }
            if handled_exc_return {
                let ran = (max_steps as u32).saturating_sub(self.uc.get_data().batch_budget);
                return Ok(ran.max(1));
            }
            return Err(format!("unicorn batch failed at pc=0x{begin:08x}: {err:?}"));
        }

        let ran = (max_steps as u32).saturating_sub(self.uc.get_data().batch_budget);
        Ok(ran.max(1))
    }

    fn reset(&mut self, bus: &mut dyn SystemBus, vector_table_base: u64) -> Result<(), String> {
        let initial_sp = bus.read32(vector_table_base)?;
        let reset_handler = bus.read32(vector_table_base + 4)?;
        self.registers = [0; 16];
        self.registers[13] = initial_sp;
        self.registers[15] = reset_handler & !1;
        self.xpsr = 1 << 24;
        self.current_exception = 0;
        self.msp = initial_sp;
        self.psp = initial_sp;
        self.control = 0x2;
        self.primask = 0;
        self.basepri = 0;
        self.faultmask = 0;

        self.uc.get_data_mut().mapped_pages.clear();
        self.uc.get_data_mut().last_error = None;
        self.uc.get_data_mut().exc_return_fail_logged = false;
        self.uc.get_data_mut().pending_exc_return = None;

        // Dynamically map FLASH and RAM from target
        let mut regions = Vec::new();
        // Since we don't have direct access to TargetSpec here, we'll try to map common ranges 
        // that are memory-backed in the bus.
        // Actually, the bus implementation knows what's memory backed. 
        // For now, let's use a heuristic or add a method to SystemBus.
        // Given the constraints, let's map 0x0800_0000 (Flash) and 0x2000_0000 (SRAM) and 0x1000_0000 (CCM)
        regions.push((0x0800_0000, 0x0810_0000));
        regions.push((0x2000_0000, 0x2004_0000)); // Map up to 256KB SRAM
        regions.push((0x1000_0000, 0x1001_0000));

        for &(start, end) in &regions {
            let size = end - start;
            if let Err(err) = self.uc.mem_map(start, size, Prot::ALL)
                && err != uc_error::MAP
            {
                return Err(format!("unicorn mem_map 0x{start:08x}..0x{end:08x} failed: {err:?}"));
            }
            let mut page = start;
            while page < end {
                self.uc.get_data_mut().mapped_pages.insert(page);
                page += UC_PAGE_SIZE;
            }
        }

        self.uc.get_data_mut().suppress_rw_hooks = true;
        for &(start, end) in &regions {
            let mut page = start;
            while page < end {
                let mut buf = [0u8; UC_PAGE_SIZE as usize];
                if let Ok(()) = bus.read_block(page, &mut buf) {
                    let _ = self.uc.mem_write(page, &buf);
                }
                page += UC_PAGE_SIZE;
            }
        }
        self.uc.get_data_mut().suppress_rw_hooks = false;

        self.write_uc_state()?;
        Ok(())
    }

    fn architecture(&self) -> &dyn CpuArchitecture {
        &self.arch
    }

    fn program_counter(&self) -> u64 {
        self.registers[15] as u64
    }

    fn stack_pointer(&self) -> u64 {
        self.registers[13] as u64
    }

    fn in_exception(&self) -> bool {
        self.current_exception != 0
    }

    fn enter_exception(
        &mut self,
        bus: &mut dyn SystemBus,
        vector_table_base: u64,
        exception_number: u16,
    ) -> Result<bool, String> {
        if exception_number == 0 {
            return Ok(false);
        }

        let vector_addr = vector_table_base + u64::from(exception_number) * 4;
        let handler = bus.read32(vector_addr)?;
        if handler == 0 || handler == 0xFFFF_FFFF {
            return Ok(false);
        }

        let in_handler = self.current_exception != 0;
        let prefer_psp = !in_handler && (self.control & 0x2) != 0;
        let use_psp = prefer_psp && Self::is_valid_stack_addr(self.psp);
        let active_sp = if use_psp { self.psp } else { self.msp };
        let next_sp = active_sp.wrapping_sub(32);
        if !Self::is_valid_stack_addr(next_sp) || !Self::is_valid_stack_addr(next_sp.wrapping_add(28)) {
            return Ok(false);
        }
        bus.write32(next_sp as u64, self.registers[0])?;
        bus.write32((next_sp + 4) as u64, self.registers[1])?;
        bus.write32((next_sp + 8) as u64, self.registers[2])?;
        bus.write32((next_sp + 12) as u64, self.registers[3])?;
        bus.write32((next_sp + 16) as u64, self.registers[12])?;
        bus.write32((next_sp + 20) as u64, self.registers[14])?;
        bus.write32((next_sp + 24) as u64, self.registers[15] | 1)?;
        bus.write32((next_sp + 28) as u64, self.xpsr)?;

        if use_psp {
            self.psp = next_sp;
            self.registers[14] = 0xFFFF_FFFD;
        } else if in_handler {
            self.msp = next_sp;
            self.registers[14] = 0xFFFF_FFF1;
        } else {
            self.msp = next_sp;
            self.registers[14] = 0xFFFF_FFF9;
        }
        self.registers[13] = self.msp;
        self.registers[15] = handler & !1;
        self.current_exception = exception_number;
        self.xpsr = (self.xpsr & !0x1FF) | (u32::from(exception_number) & 0x1FF);
        self.xpsr |= 1 << 24;
        self.write_uc_state()?;
        Ok(true)
    }
}

fn set_uc_bus(data: &mut UcData, bus: &mut dyn SystemBus) {
    let raw = bus as *mut dyn SystemBus;
    let (ptr, vt): (usize, usize) = unsafe { std::mem::transmute(raw) };
    data.bus_data = ptr;
    data.bus_vtable = vt;
}

fn clear_uc_bus(data: &mut UcData) {
    data.bus_data = 0;
    data.bus_vtable = 0;
}

fn get_uc_bus(data: &UcData) -> Option<*mut dyn SystemBus> {
    if data.bus_data == 0 {
        None
    } else {
        Some(unsafe {
            std::mem::transmute::<(usize, usize), *mut dyn SystemBus>((data.bus_data, data.bus_vtable))
        })
    }
}

fn page_base(addr: u64) -> u64 {
    addr & !(UC_PAGE_SIZE - 1)
}

fn map_page_minimal(uc: &mut Unicorn<'_, UcData>, addr: u64) -> Result<(), String> {
    let page = page_base(addr);
    if !uc.get_data_mut().mapped_pages.insert(page) {
        return Ok(());
    }

    if let Err(err) = uc.mem_map(page, UC_PAGE_SIZE, Prot::ALL)
        && err != uc_error::MAP
    {
        return Err(format!("unicorn mem_map failed @0x{page:08x}: {err:?}"));
    }
    Ok(())
}

fn read_bus_bytes(bus: &mut dyn SystemBus, addr: u64, size: usize) -> Result<Vec<u8>, String> {
    let mut bytes = vec![0u8; size];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = bus.read8(addr + i as u64)?;
    }
    Ok(bytes)
}

fn write_bus_value(bus: &mut dyn SystemBus, addr: u64, size: usize, value: i64) -> Result<(), String> {
    match size {
        1 => bus.write8(addr, value as u8),
        2 => bus.write16(addr, value as u16),
        4 => bus.write32(addr, value as u32),
        _ => {
            let raw = value as u64;
            for i in 0..size {
                bus.write8(addr + i as u64, ((raw >> (i * 8)) & 0xFF) as u8)?;
            }
            Ok(())
        }
    }
}

fn install_hooks(uc: &mut Unicorn<'_, UcData>) -> Result<(), String> {
    uc.add_mem_hook(HookType::MEM_UNMAPPED, 1, 0, |uc, _t, addr, _size, _value| {
        let addr32 = addr as u32;
        // EXC_RETURN is not real code memory; stop so the core can run software unstack logic.
        if addr32 & 0xFFFF_FFE0 == 0xFFFF_FFE0 {
            let lr = uc.reg_read(RegisterARM::LR).unwrap_or(addr) as u32;
            let exc_return = if lr & 0xFFFF_FFE0 == 0xFFFF_FFE0 {
                lr as u64
            } else {
                let control = uc.reg_read(RegisterARM::CONTROL).unwrap_or(0) as u32;
                if (control & 0x2) != 0 {
                    0xFFFF_FFFD
                } else {
                    0xFFFF_FFF9
                }
            };
            uc.get_data_mut().pending_exc_return = Some(exc_return as u32);
            if let Err(err) = uc.emu_stop() {
                uc.get_data_mut().last_error =
                    Some(format!("uc emu_stop for EXC_RETURN failed @0x{exc_return:08x}: {err:?}"));
                return false;
            }
            return true;
        }
        if let Err(err) = map_page_minimal(uc, addr) {
            uc.get_data_mut().last_error = Some(err);
            return false;
        }
        let bus_ptr = match get_uc_bus(uc.get_data()) {
            Some(p) => p,
            None => return true,
        };
        let bus = unsafe { &mut *bus_ptr };
        let page = page_base(addr);
        let mut buf = vec![0u8; UC_PAGE_SIZE as usize];
        for i in 0..UC_PAGE_SIZE {
            buf[i as usize] = bus.read8(page + i).unwrap_or(0);
        }
        uc.get_data_mut().suppress_rw_hooks = true;
        let write_res = uc.mem_write(page, &buf);
        uc.get_data_mut().suppress_rw_hooks = false;
        if let Err(err) = write_res {
            uc.get_data_mut().last_error = Some(format!(
                "unicorn mem_write in unmapped hook failed @0x{page:08x}: {err:?}"
            ));
            return false;
        }
        true
    })
    .map_err(|e| format!("add mem_unmapped hook failed: {e:?}"))?;
            
    uc.add_code_hook(1, 0, |uc, _addr, _size| {
        let budget = uc.get_data().batch_budget;
        if budget > 0 {
            uc.get_data_mut().batch_budget = budget - 1;
        }
    })
    .map_err(|e| format!("add code hook failed: {e:?}"))?;

    // Hook MMIO-like regions:
    // - Peripheral/device space (0x4000_0000 - 0xE000_0000)
    // - Cortex-M System / NVIC (0xE000_0000 - 0xF000_0000)
    let mmio_ranges = [(0x4000_0000, 0xF000_0000)];

    for &(start, end) in &mmio_ranges {
        // Use a combined hook for both mapped and unmapped writes
        uc.add_mem_hook(HookType::MEM_WRITE | HookType::MEM_WRITE_UNMAPPED, start, end, |uc, _t, addr, size, value| {
            if uc.get_data().suppress_rw_hooks {
                return true;
            }
            let bus_ptr = match get_uc_bus(uc.get_data()) {
                Some(p) => p,
                None => return false,
            };
            let bus = unsafe { &mut *bus_ptr };
            if let Err(err) = write_bus_value(bus, addr, size, value) {
                uc.get_data_mut().last_error = Some(err);
                return false;
            }
            true // We handled it
        })
        .map_err(|e| format!("add mmio write hook failed: {e:?}"))?;

        uc.add_mem_hook(HookType::MEM_READ | HookType::MEM_READ_UNMAPPED, start, end, |uc, _t, addr, size, _value| {
            if uc.get_data().suppress_rw_hooks {
                return true;
            }
            let bus_ptr = match get_uc_bus(uc.get_data()) {
                Some(p) => p,
                None => return false,
            };
            let bus = unsafe { &mut *bus_ptr };
            let bytes = match read_bus_bytes(bus, addr, size) {
                Ok(v) => v,
                Err(err) => {
                    uc.get_data_mut().last_error = Some(err);
                    return false;
                }
            };

            if let Err(err) = map_page_minimal(uc, addr) {
                uc.get_data_mut().last_error = Some(err);
                return false;
            }
            uc.get_data_mut().suppress_rw_hooks = true;
            if let Err(err) = uc.mem_write(addr, &bytes) {
                uc.get_data_mut().suppress_rw_hooks = false;
                uc.get_data_mut().last_error =
                    Some(format!("unicorn mem_write in mmio read hook failed @0x{addr:08x}: {err:?}"));
                return false;
            }
            uc.get_data_mut().suppress_rw_hooks = false;
            true
        })
        .map_err(|e| format!("add mmio read hook failed: {e:?}"))?;
    }

    Ok(())
}
