use super::armv7m_legacy::ArmV7MArchitecture;
use crate::bus::SystemBus;
use crate::cpu::{CpuArchitecture, CpuCore};

#[cfg(feature = "cpu-external-unicorn")]
use std::collections::HashSet;

#[cfg(feature = "cpu-external-unicorn")]
use unicorn_engine::unicorn_const::{uc_error, Arch, HookType, Mode, Prot, RegisterARM};
#[cfg(feature = "cpu-external-unicorn")]
use unicorn_engine::Unicorn;

#[cfg(feature = "cpu-external-unicorn")]
const UC_PAGE_SIZE: u64 = 0x1000;

#[cfg(feature = "cpu-external-unicorn")]
const PREMAP_REGIONS: &[(u64, u64)] = &[
    (0x0000_0000, 0x0010_0000), // Flash alias (1MB)
    (0x0800_0000, 0x0810_0000), // Flash (1MB)
    (0x2000_0000, 0x2002_0000), // SRAM (128KB)
    (0x1000_0000, 0x1001_0000), // CCM RAM (64KB)
];

#[cfg(feature = "cpu-external-unicorn")]
fn is_memory_backed(addr: u64) -> bool {
    (0x0000_0000..0x0010_0000).contains(&addr)
        || (0x0800_0000..0x0810_0000).contains(&addr)
        || (0x2000_0000..0x2002_0000).contains(&addr)
        || (0x1000_0000..0x1001_0000).contains(&addr)
}

#[cfg(feature = "cpu-external-unicorn")]
#[derive(Default)]
struct UcData {
    bus_data: usize,
    bus_vtable: usize,
    mapped_pages: HashSet<u64>,
    dirty_sram_pages: HashSet<u64>,
    last_error: Option<String>,
    suppress_rw_hooks: bool,
    pending_cycles: u32,
    batch_budget: u32,
}

#[derive(Debug)]
pub struct CortexM3External {
    arch: ArmV7MArchitecture,
    registers: [u32; 16],
    xpsr: u32,
    #[cfg(feature = "cpu-external-unicorn")]
    uc: Unicorn<'static, UcData>,
    /// Whether we've done initial bulk load from bus into Unicorn
    #[cfg(feature = "cpu-external-unicorn")]
    initial_load_done: bool,
}

impl Default for CortexM3External {
    fn default() -> Self {
        Self::new()
    }
}

impl CortexM3External {
    pub fn new() -> Self {
        #[cfg(feature = "cpu-external-unicorn")]
        {
            let mut uc = Unicorn::new_with_data(Arch::ARM, Mode::THUMB | Mode::MCLASS, UcData::default())
                .expect("failed to create unicorn ARMv7-M engine");
            install_hooks(&mut uc).expect("failed to install unicorn hooks");
            return Self {
                arch: ArmV7MArchitecture,
                registers: [0; 16],
                xpsr: 1 << 24,
                uc,
                initial_load_done: false,
            };
        }

        #[cfg(not(feature = "cpu-external-unicorn"))]
        {
            Self {
                arch: ArmV7MArchitecture,
                registers: [0; 16],
                xpsr: 1 << 24,
            }
        }
    }

    pub fn registers(&self) -> &[u32; 16] {
        &self.registers
    }

    pub fn thumb_state(&self) -> bool {
        (self.xpsr >> 24) & 1 == 1
    }

    #[cfg(feature = "cpu-external-unicorn")]
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

        // Use CPSR to set Thumb bit + condition flags
        // Bit 5 (T) = Thumb mode, bits 31:28 = NZCV
        let cpsr = (self.xpsr & 0xF800_0000) | if self.thumb_state() { 1 << 5 } else { 0 };
        self.uc
            .reg_write(RegisterARM::CPSR, cpsr as u64)
            .map_err(|e| format!("uc reg_write cpsr failed: {e:?}"))?;
        Ok(())
    }

    #[cfg(feature = "cpu-external-unicorn")]
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

        let cpsr = self.uc.reg_read(RegisterARM::CPSR).map_err(|e| format!("uc reg_read cpsr failed: {e:?}"))? as u32;
        // Extract NZCV flags and Thumb bit from CPSR
        self.xpsr = (cpsr & 0xF800_0000) | if cpsr & (1 << 5) != 0 { 1 << 24 } else { 0 };
        Ok(())
    }

    /// Sync dirty SRAM pages from Unicorn back to the bus
    #[cfg(feature = "cpu-external-unicorn")]
    fn sync_dirty_sram_to_bus(&mut self, bus: &mut dyn SystemBus) -> Result<(), String> {
        let dirty: Vec<u64> = self.uc.get_data_mut().dirty_sram_pages.drain().collect();
        if dirty.is_empty() {
            return Ok(());
        }
        self.uc.get_data_mut().suppress_rw_hooks = true;
        for page in &dirty {
            let mut buf = vec![0u8; UC_PAGE_SIZE as usize];
            if let Err(e) = self.uc.mem_read(*page, &mut buf) {
                self.uc.get_data_mut().suppress_rw_hooks = false;
                return Err(format!("sync sram mem_read failed @0x{page:08x}: {e:?}"));
            }
            if (0x2000_0000..0x2002_0000).contains(page) {
                for i in 0..UC_PAGE_SIZE {
                    let addr = page + i;
                    let _ = bus.write8(addr, buf[i as usize]);
                }
            }
        }
        self.uc.get_data_mut().suppress_rw_hooks = false;
        Ok(())
    }
}

impl CpuCore for CortexM3External {
    fn step(&mut self, bus: &mut dyn SystemBus) -> Result<(), String> {
        #[cfg(not(feature = "cpu-external-unicorn"))]
        {
            let _ = bus;
            return Err("external CPU backend requested but 'cpu-external-unicorn' feature is disabled".to_string());
        }

        #[cfg(feature = "cpu-external-unicorn")]
        {
            // If we have pending instructions in an optimized batch, just return
            if self.uc.get_data().pending_cycles > 0 {
                self.uc.get_data_mut().pending_cycles -= 1;
                return Ok(());
            }

            self.uc.get_data_mut().last_error = None;
            set_uc_bus(self.uc.get_data_mut(), bus);
            self.write_uc_state()?;

            let mut begin = self.registers[15] as u64;
            if self.thumb_state() {
                begin |= 1;
            }

            // Run a batch of up to 1024 instructions. 
            // Unicorn will stop automatically if it hits MMIO (hook returns false).
            let batch_size = 1024u32;
            self.uc.get_data_mut().batch_budget = batch_size;
            self.uc.get_data_mut().pending_cycles = 0;
            
            let result = self.uc.emu_start(begin, 0, 0, batch_size as usize);

            // Read back register state
            self.read_uc_state()?;

            // Sync dirty SRAM pages back to the bus
            self.sync_dirty_sram_to_bus(bus)?;

            clear_uc_bus(self.uc.get_data_mut());

            if let Some(err) = self.uc.get_data_mut().last_error.take() {
                return Err(err);
            }
            if let Err(err) = result {
                return Err(format!("unicorn batch failed at pc=0x{begin:08x}: {err:?}"));
            }

            // The batch budget was decremented by the instruction hook.
            // Remaining budget is what we didn't run.
            let ran = batch_size.saturating_sub(self.uc.get_data().batch_budget);
            if ran > 0 {
                self.uc.get_data_mut().pending_cycles = ran - 1;
            }
            Ok(())
        }
    }

    fn reset(&mut self, bus: &mut dyn SystemBus, vector_table_base: u64) -> Result<(), String> {
        let initial_sp = bus.read32(vector_table_base)?;
        let reset_handler = bus.read32(vector_table_base + 4)?;
        self.registers = [0; 16];
        self.registers[13] = initial_sp;
        self.registers[15] = reset_handler & !1;
        self.xpsr = 1 << 24;

        #[cfg(feature = "cpu-external-unicorn")]
        {
            self.uc.get_data_mut().mapped_pages.clear();
            self.uc.get_data_mut().dirty_sram_pages.clear();
            self.uc.get_data_mut().last_error = None;

            // Pre-map well-known memory regions
            for &(start, end) in PREMAP_REGIONS {
                let size = end - start;
                if let Err(err) = self.uc.mem_map(start, size, Prot::ALL) {
                    if err != uc_error::MAP {
                        return Err(format!("unicorn mem_map 0x{start:08x}..0x{end:08x} failed: {err:?}"));
                    }
                }
                let mut page = start;
                while page < end {
                    self.uc.get_data_mut().mapped_pages.insert(page);
                    page += UC_PAGE_SIZE;
                }
            }

            // Bulk-load all pre-mapped pages from bus into Unicorn
            self.uc.get_data_mut().suppress_rw_hooks = true;
            for &(start, end) in PREMAP_REGIONS {
                let mut page = start;
                while page < end {
                    let mut buf = vec![0u8; UC_PAGE_SIZE as usize];
                    for i in 0..UC_PAGE_SIZE {
                        buf[i as usize] = bus.read8(page + i).unwrap_or(0);
                    }
                    if let Err(e) = self.uc.mem_write(page, &buf) {
                        self.uc.get_data_mut().suppress_rw_hooks = false;
                        return Err(format!("reset mem_write failed @0x{page:08x}: {e:?}"));
                    }
                    page += UC_PAGE_SIZE;
                }
            }
            self.uc.get_data_mut().suppress_rw_hooks = false;

            self.write_uc_state()?;
            self.initial_load_done = true;
        }

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
        (self.xpsr & 0x1FF) != 0
    }

    fn enter_exception(
        &mut self,
        _bus: &mut dyn SystemBus,
        _vector_table_base: u64,
        _exception_number: u16,
    ) -> Result<bool, String> {
        Ok(false)
    }
}

#[cfg(feature = "cpu-external-unicorn")]
fn set_uc_bus(data: &mut UcData, bus: &mut dyn SystemBus) {
    let raw = bus as *mut dyn SystemBus;
    let (ptr, vt): (usize, usize) = unsafe { std::mem::transmute(raw) };
    data.bus_data = ptr;
    data.bus_vtable = vt;
}

#[cfg(feature = "cpu-external-unicorn")]
fn clear_uc_bus(data: &mut UcData) {
    data.bus_data = 0;
    data.bus_vtable = 0;
}

#[cfg(feature = "cpu-external-unicorn")]
fn get_uc_bus(data: &UcData) -> Option<*mut dyn SystemBus> {
    if data.bus_data == 0 {
        None
    } else {
        Some(unsafe {
            std::mem::transmute::<(usize, usize), *mut dyn SystemBus>((data.bus_data, data.bus_vtable))
        })
    }
}

#[cfg(feature = "cpu-external-unicorn")]
fn page_base(addr: u64) -> u64 {
    addr & !(UC_PAGE_SIZE - 1)
}

#[cfg(feature = "cpu-external-unicorn")]
fn map_page_minimal(uc: &mut Unicorn<'_, UcData>, addr: u64) -> Result<(), String> {
    let page = page_base(addr);
    if !uc.get_data_mut().mapped_pages.insert(page) {
        return Ok(());
    }

    if let Err(err) = uc.mem_map(page, UC_PAGE_SIZE, Prot::ALL) {
        if err != uc_error::MAP {
            return Err(format!("unicorn mem_map failed @0x{page:08x}: {err:?}"));
        }
    }
    Ok(())
}

#[cfg(feature = "cpu-external-unicorn")]
fn read_bus_bytes(bus: &mut dyn SystemBus, addr: u64, size: usize) -> Result<Vec<u8>, String> {
    let mut bytes = vec![0u8; size];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = bus.read8(addr + i as u64)?;
    }
    Ok(bytes)
}

#[cfg(feature = "cpu-external-unicorn")]
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

#[cfg(feature = "cpu-external-unicorn")]
fn install_hooks(uc: &mut Unicorn<'_, UcData>) -> Result<(), String> {
    // Handle unmapped memory: map the page and populate from bus if available
    uc.add_mem_hook(HookType::MEM_UNMAPPED, 1, 0, |uc, _t, addr, _size, _value| {
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
            
    // Instruction counter hook
    uc.add_code_hook(1, 0, |uc, _addr, _size| {
        let budget = uc.get_data().batch_budget;
        if budget > 0 {
            uc.get_data_mut().batch_budget = budget - 1;
        }
    })
    .map_err(|e| format!("add code hook failed: {e:?}"))?;

    // MEM_WRITE hook: track dirty SRAM pages and forward MMIO writes to bus
    uc.add_mem_hook(HookType::MEM_WRITE, 1, 0, |uc, _t, addr, size, value| {
        if uc.get_data().suppress_rw_hooks {
            return false;
        }

        // Track dirty SRAM pages for later sync back to bus
        // Track dirty SRAM pages for later sync back to bus
        if (0x2000_0000..0x2002_0000).contains(&addr) || (0x1000_0000..0x1001_0000).contains(&addr) {
            let page = page_base(addr);
            uc.get_data_mut().dirty_sram_pages.insert(page);
            return true; // CONTINUE emulation
        }

        // For MMIO regions, forward to bus and STOP emulation to sync back to Rust
        if !is_memory_backed(addr) {
            let bus_ptr = match get_uc_bus(uc.get_data()) {
                Some(p) => p,
                None => return false,
            };
            let bus = unsafe { &mut *bus_ptr };
            if let Err(err) = write_bus_value(bus, addr, size, value) {
                uc.get_data_mut().last_error = Some(err);
                return false;
            }
            return false; // STOP emulation to keep machine in sync
        }
        true
    })
    .map_err(|e| format!("add mem_write hook failed: {e:?}"))?;

    // MEM_READ hook: for MMIO, fetch from bus and update Unicorn memory
    uc.add_mem_hook(HookType::MEM_READ, 1, 0, |uc, _t, addr, size, _value| {
        if uc.get_data().suppress_rw_hooks {
            return false;
        }
        // Only intercept MMIO reads (not memory-backed regions)
        if is_memory_backed(addr) {
            return false;
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

        uc.get_data_mut().suppress_rw_hooks = true;
        let write_res = uc.mem_write(addr, &bytes);
        uc.get_data_mut().suppress_rw_hooks = false;
        if let Err(err) = write_res {
            uc.get_data_mut().last_error = Some(format!(
                "unicorn mem_write in read hook failed @0x{addr:08x}: {err:?}"
            ));
            return false;
        }
        false
    })
    .map_err(|e| format!("add mem_read hook failed: {e:?}"))?;

    Ok(())
}
