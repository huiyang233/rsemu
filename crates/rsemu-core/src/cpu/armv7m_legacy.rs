use crate::bus::SystemBus;
use crate::cpu::{ArchitectureId, ArchitectureMemoryConfig, CpuArchitecture, CpuCore};

pub const FLASH_ALIAS_BASE: u64 = 0x0000_0000;
pub const PERIPH_BB_BASE: u64 = 0x4000_0000;
pub const PERIPH_BB_ALIAS_BASE: u64 = 0x4200_0000;
pub const PERIPH_BB_ALIAS_END: u64 = 0x4400_0000;

#[derive(Debug, Default)]
pub struct ArmV7MArchitecture;

impl CpuArchitecture for ArmV7MArchitecture {
    fn id(&self) -> ArchitectureId {
        ArchitectureId::ArmV7M
    }

    fn name(&self) -> &'static str {
        "ARMv7-M"
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

const DECODE_CACHE_SIZE: usize = 2048;

#[derive(Debug, Clone, Copy, Default)]
struct DecodeCacheEntry {
    valid: bool,
    pc: u32,
    opcode: u16,
    width: u8,
    opcode2: u16,
    has_opcode2: bool,
}

#[derive(Debug)]
pub struct CortexM3 {
    arch: ArmV7MArchitecture,
    registers: [u32; 16],
    xpsr: u32,
    it_conds: [u8; 4],
    it_pos: u8,
    it_count: u8,
    decode_cache: [DecodeCacheEntry; DECODE_CACHE_SIZE],
}

impl Default for CortexM3 {
    fn default() -> Self {
        Self {
            arch: ArmV7MArchitecture,
            registers: [0; 16],
            xpsr: 0,
            it_conds: [0; 4],
            it_pos: 0,
            it_count: 0,
            decode_cache: [DecodeCacheEntry::default(); DECODE_CACHE_SIZE],
        }
    }
}

impl CortexM3 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn registers(&self) -> &[u32; 16] {
        &self.registers
    }

    pub fn thumb_state(&self) -> bool {
        (self.xpsr >> 24) & 1 == 1
    }

    fn set_nz_flags(&mut self, value: u32) {
        if value == 0 {
            self.xpsr |= 1 << 30;
        } else {
            self.xpsr &= !(1 << 30);
        }

        if value & 0x8000_0000 != 0 {
            self.xpsr |= 1 << 31;
        } else {
            self.xpsr &= !(1 << 31);
        }
    }

    fn set_carry_flag(&mut self, carry: bool) {
        if carry {
            self.xpsr |= 1 << 29;
        } else {
            self.xpsr &= !(1 << 29);
        }
    }

    fn set_overflow_flag(&mut self, overflow: bool) {
        if overflow {
            self.xpsr |= 1 << 28;
        } else {
            self.xpsr &= !(1 << 28);
        }
    }

    fn set_add_flags(&mut self, lhs: u32, rhs: u32, result: u32) {
        self.set_nz_flags(result);
        self.set_carry_flag((lhs as u64 + rhs as u64) > u32::MAX as u64);
        self.set_overflow_flag((((lhs ^ result) & (rhs ^ result)) & 0x8000_0000) != 0);
    }

    fn set_sub_flags(&mut self, lhs: u32, rhs: u32, result: u32) {
        self.set_nz_flags(result);
        self.set_carry_flag(lhs >= rhs);
        self.set_overflow_flag((((lhs ^ rhs) & (lhs ^ result)) & 0x8000_0000) != 0);
    }

    fn condition_passed(&self, cond: u8) -> bool {
        let n = (self.xpsr >> 31) & 1 == 1;
        let z = (self.xpsr >> 30) & 1 == 1;
        let c = (self.xpsr >> 29) & 1 == 1;
        let v = (self.xpsr >> 28) & 1 == 1;
        match cond {
            0x0 => z,
            0x1 => !z,
            0x2 => c,
            0x3 => !c,
            0x4 => n,
            0x5 => !n,
            0x6 => v,
            0x7 => !v,
            0x8 => c && !z,
            0x9 => !c || z,
            0xA => n == v,
            0xB => n != v,
            0xC => !z && (n == v),
            0xD => z || (n != v),
            0xE => true,
            _ => false,
        }
    }

    fn set_it_state(&mut self, first_cond: u8, count: u8) {
        self.it_conds[0] = first_cond;
        for i in 1..count as usize {
            self.it_conds[i] = first_cond;
        }
        self.it_pos = 0;
        self.it_count = count;
    }

    fn it_count_from_mask(mask: u8) -> Option<u8> {
        match mask {
            0x8 => Some(1),
            0x4 | 0xC => Some(2),
            _ => None,
        }
    }

    fn instruction_width(opcode: u16) -> u32 {
        if opcode & 0xF800 == 0xE800 || opcode & 0xF000 == 0xF000 {
            4
        } else {
            2
        }
    }

    #[inline]
    fn decode_cache_index(pc: u64) -> usize {
        ((pc >> 1) as usize) & (DECODE_CACHE_SIZE - 1)
    }

    #[inline]
    fn is_fetch_cacheable(pc: u64) -> bool {
        pc < 0x2000_0000
    }

    #[inline]
    fn fetch_opcode(
        &mut self,
        bus: &mut dyn SystemBus,
        pc: u64,
    ) -> Result<(u16, u32), String> {
        if Self::is_fetch_cacheable(pc) {
            let idx = Self::decode_cache_index(pc);
            let entry = self.decode_cache[idx];
            if entry.valid && entry.pc == pc as u32 {
                return Ok((entry.opcode, entry.width as u32));
            }

            let opcode = bus.read16(pc)?;
            let width = Self::instruction_width(opcode) as u8;
            self.decode_cache[idx] = DecodeCacheEntry {
                valid: true,
                pc: pc as u32,
                opcode,
                width,
                opcode2: 0,
                has_opcode2: false,
            };
            return Ok((opcode, width as u32));
        }

        let opcode = bus.read16(pc)?;
        Ok((opcode, Self::instruction_width(opcode)))
    }

    #[inline]
    fn fetch_opcode2(&mut self, bus: &mut dyn SystemBus, pc: u64) -> Result<u16, String> {
        if Self::is_fetch_cacheable(pc) {
            let idx = Self::decode_cache_index(pc);
            let entry = self.decode_cache[idx];
            if entry.valid && entry.pc == pc as u32 && entry.has_opcode2 {
                return Ok(entry.opcode2);
            }

            let opcode2 = bus.read16(pc + 2)?;
            if self.decode_cache[idx].valid && self.decode_cache[idx].pc == pc as u32 {
                self.decode_cache[idx].opcode2 = opcode2;
                self.decode_cache[idx].has_opcode2 = true;
            }
            return Ok(opcode2);
        }

        bus.read16(pc + 2)
    }

    fn thumb_expand_imm12(imm12: u32) -> u32 {
        if ((imm12 >> 10) & 0x3) == 0 {
            let imm8 = imm12 & 0xFF;
            match (imm12 >> 8) & 0x3 {
                0 => imm8,
                1 => (imm8 << 16) | imm8,
                2 => (imm8 << 24) | (imm8 << 8),
                3 => (imm8 << 24) | (imm8 << 16) | (imm8 << 8) | imm8,
                _ => unreachable!(),
            }
        } else {
            let unrotated = 0x80 | (imm12 & 0x7F);
            unrotated.rotate_right((imm12 >> 7) & 0x1F)
        }
    }

    fn shift_imm(value: u32, shift_type: u16, imm5: u32) -> u32 {
        match shift_type {
            0 => value.wrapping_shl(imm5),
            1 => {
                if imm5 == 0 || imm5 >= 32 {
                    0
                } else {
                    value >> imm5
                }
            }
            2 => {
                if imm5 == 0 || imm5 >= 32 {
                    if value & 0x8000_0000 != 0 {
                        u32::MAX
                    } else {
                        0
                    }
                } else {
                    ((value as i32) >> imm5) as u32
                }
            }
            3 => value.rotate_right(imm5 % 32),
            _ => unreachable!(),
        }
    }

    fn is_exc_return(value: u32) -> bool {
        value & 0xFFFF_FFE0 == 0xFFFF_FFE0
    }

    fn exception_return(&mut self, bus: &mut dyn SystemBus) -> Result<bool, String> {
        let sp = self.registers[13];
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
        self.registers[13] = sp.wrapping_add(32);
        self.registers[15] = pc & !1;
        self.xpsr = xpsr;
        if pc & 1 == 1 {
            self.xpsr |= 1 << 24;
        } else {
            self.xpsr &= !(1 << 24);
        }
        Ok(true)
    }

    fn try_fast_memclr_loop(
        &mut self,
        bus: &mut dyn SystemBus,
        pc: u64,
        opcode: u16,
        opcode2: u16,
    ) -> Result<bool, String> {
        // Fast-path a common Thumb-2 word clear loop used by __aeabi_memclr:
        //   str.w r3, [r2], #4
        //   cmp   r2, r0
        //   bcc   <loop>
        if opcode != 0xF842 {
            return Ok(false);
        }
        if opcode2 != 0x3B04 {
            return Ok(false);
        }
        let cmp = bus.read16(pc + 4)?;
        if cmp != 0x4282 {
            return Ok(false);
        }
        let branch = bus.read16(pc + 6)?;
        if (branch & 0xFF00) != 0xD300 {
            return Ok(false);
        }

        let base_reg = (opcode & 0xF) as usize;
        let src_reg = ((opcode2 >> 12) & 0xF) as usize;
        let end_reg = ((cmp >> 3) & 0x7) as usize;
        let post_imm = (opcode2 & 0xFF) as u32;
        if post_imm == 0 {
            return Ok(false);
        }

        let mut ptr = self.registers[base_reg];
        let end = self.registers[end_reg];
        let value = self.registers[src_reg];

        if ptr < end {
            let iterations = (end - ptr).div_ceil(post_imm);
            for _ in 0..iterations {
                bus.write32(ptr as u64, value)?;
                ptr = ptr.wrapping_add(post_imm);
            }
        }

        self.registers[base_reg] = ptr;
        let result = ptr.wrapping_sub(end);
        self.set_sub_flags(ptr, end, result);
        self.registers[15] = self.registers[15].wrapping_add(8);
        Ok(true)
    }

    fn step16_fast(
        &mut self,
        bus: &mut dyn SystemBus,
        pc: u64,
        opcode: u16,
    ) -> Result<bool, String> {
        let handled: Result<bool, String> = match opcode & 0xF800 {
            0x4800 => {
                let rt = ((opcode >> 8) & 0x7) as usize;
                let imm8 = (opcode & 0x00FF) as u32;
                let base = (self.registers[15] + 4) & !0x3;
                let addr = base.wrapping_add(imm8 << 2);
                self.registers[rt] = bus.read32(addr as u64)?;
                self.registers[15] = self.registers[15].wrapping_add(2);
                Ok(true)
            }
            0xA000 => {
                let rd = ((opcode >> 8) & 0x7) as usize;
                let imm8 = (opcode & 0xFF) as u32;
                let base = (self.registers[15] + 4) & !0x3;
                self.registers[rd] = base.wrapping_add(imm8 << 2);
                self.registers[15] = self.registers[15].wrapping_add(2);
                Ok(true)
            }
            0xA800 => {
                let rd = ((opcode >> 8) & 0x7) as usize;
                let imm8 = (opcode & 0xFF) as u32;
                self.registers[rd] = self.registers[13].wrapping_add(imm8 << 2);
                self.registers[15] = self.registers[15].wrapping_add(2);
                Ok(true)
            }
            0x2000 => {
                let rd = ((opcode >> 8) & 0x7) as usize;
                let imm8 = (opcode & 0x00FF) as u32;
                self.registers[rd] = imm8;
                self.set_nz_flags(imm8);
                self.registers[15] = self.registers[15].wrapping_add(2);
                Ok(true)
            }
            0x2800 => {
                let rn = ((opcode >> 8) & 0x7) as usize;
                let imm8 = (opcode & 0x00FF) as u32;
                let result = self.registers[rn].wrapping_sub(imm8);
                self.set_sub_flags(self.registers[rn], imm8, result);
                self.registers[15] = self.registers[15].wrapping_add(2);
                Ok(true)
            }
            0x3000 => {
                let rd = ((opcode >> 8) & 0x7) as usize;
                let imm8 = (opcode & 0x00FF) as u32;
                let result = self.registers[rd].wrapping_add(imm8);
                self.registers[rd] = result;
                self.set_add_flags(self.registers[rd].wrapping_sub(imm8), imm8, result);
                self.registers[15] = self.registers[15].wrapping_add(2);
                Ok(true)
            }
            0x3800 => {
                let rd = ((opcode >> 8) & 0x7) as usize;
                let imm8 = (opcode & 0x00FF) as u32;
                let result = self.registers[rd].wrapping_sub(imm8);
                self.registers[rd] = result;
                self.set_sub_flags(self.registers[rd].wrapping_add(imm8), imm8, result);
                self.registers[15] = self.registers[15].wrapping_add(2);
                Ok(true)
            }
            0x6000 => {
                let imm5 = ((opcode >> 6) & 0x1F) as u32;
                let rn = ((opcode >> 3) & 0x7) as usize;
                let rt = (opcode & 0x7) as usize;
                let addr = self.registers[rn].wrapping_add(imm5 << 2);
                bus.write32(addr as u64, self.registers[rt])?;
                self.registers[15] = self.registers[15].wrapping_add(2);
                Ok(true)
            }
            0x6800 => {
                let imm5 = ((opcode >> 6) & 0x1F) as u32;
                let rn = ((opcode >> 3) & 0x7) as usize;
                let rt = (opcode & 0x7) as usize;
                let addr = self.registers[rn].wrapping_add(imm5 << 2);
                self.registers[rt] = bus.read32(addr as u64)?;
                self.registers[15] = self.registers[15].wrapping_add(2);
                Ok(true)
            }
            0x7000 => {
                let imm5 = ((opcode >> 6) & 0x1F) as u32;
                let rn = ((opcode >> 3) & 0x7) as usize;
                let rt = (opcode & 0x7) as usize;
                let addr = self.registers[rn].wrapping_add(imm5);
                bus.write8(addr as u64, self.registers[rt] as u8)?;
                self.registers[15] = self.registers[15].wrapping_add(2);
                Ok(true)
            }
            0x7800 => {
                let imm5 = ((opcode >> 6) & 0x1F) as u32;
                let rn = ((opcode >> 3) & 0x7) as usize;
                let rt = (opcode & 0x7) as usize;
                let addr = self.registers[rn].wrapping_add(imm5);
                self.registers[rt] = bus.read8(addr as u64)? as u32;
                self.registers[15] = self.registers[15].wrapping_add(2);
                Ok(true)
            }
            0x8000 => {
                let imm5 = ((opcode >> 6) & 0x1F) as u32;
                let rn = ((opcode >> 3) & 0x7) as usize;
                let rt = (opcode & 0x7) as usize;
                let addr = self.registers[rn].wrapping_add(imm5 << 1);
                bus.write16(addr as u64, self.registers[rt] as u16)?;
                self.registers[15] = self.registers[15].wrapping_add(2);
                Ok(true)
            }
            0x8800 => {
                let imm5 = ((opcode >> 6) & 0x1F) as u32;
                let rn = ((opcode >> 3) & 0x7) as usize;
                let rt = (opcode & 0x7) as usize;
                let addr = self.registers[rn].wrapping_add(imm5 << 1);
                self.registers[rt] = bus.read16(addr as u64)? as u32;
                self.registers[15] = self.registers[15].wrapping_add(2);
                Ok(true)
            }
            0x9000 => {
                let rt = ((opcode >> 8) & 0x7) as usize;
                let imm8 = (opcode & 0xFF) as u32;
                let addr = self.registers[13].wrapping_add(imm8 << 2);
                bus.write32(addr as u64, self.registers[rt])?;
                self.registers[15] = self.registers[15].wrapping_add(2);
                Ok(true)
            }
            0x9800 => {
                let rt = ((opcode >> 8) & 0x7) as usize;
                let imm8 = (opcode & 0xFF) as u32;
                let addr = self.registers[13].wrapping_add(imm8 << 2);
                self.registers[rt] = bus.read32(addr as u64)?;
                self.registers[15] = self.registers[15].wrapping_add(2);
                Ok(true)
            }
            0xE000 => {
                let imm11 = (opcode & 0x07FF) as i16;
                let signed = ((imm11 << 5) >> 4) as i32;
                let next_pc = self.registers[15].wrapping_add(4);
                self.registers[15] = next_pc.wrapping_add_signed(signed);
                Ok(true)
            }
            _ => match opcode & 0xFC00 {
                0x4000 => {
                    let op = (opcode >> 6) & 0xF;
                    let rm = ((opcode >> 3) & 0x7) as usize;
                    let rdn = (opcode & 0x7) as usize;
                    match op {
                        0x0 => {
                            let result = self.registers[rdn] & self.registers[rm];
                            self.registers[rdn] = result;
                            self.set_nz_flags(result);
                        }
                        0x1 => {
                            let result = self.registers[rdn] ^ self.registers[rm];
                            self.registers[rdn] = result;
                            self.set_nz_flags(result);
                        }
                        0x2 => {
                            let shift = (self.registers[rm] & 0xFF) as u32;
                            let result = if shift >= 32 {
                                0
                            } else {
                                self.registers[rdn] << shift
                            };
                            self.registers[rdn] = result;
                            self.set_nz_flags(result);
                        }
                        0x3 => {
                            let shift = (self.registers[rm] & 0xFF) as u32;
                            let result = if shift == 0 {
                                self.registers[rdn]
                            } else if shift >= 32 {
                                0
                            } else {
                                self.registers[rdn] >> shift
                            };
                            self.registers[rdn] = result;
                            self.set_nz_flags(result);
                        }
                        0x4 => {
                            let shift = (self.registers[rm] & 0xFF) as u32;
                            let result = if shift == 0 {
                                self.registers[rdn]
                            } else if shift >= 32 {
                                if self.registers[rdn] & 0x8000_0000 != 0 {
                                    u32::MAX
                                } else {
                                    0
                                }
                            } else {
                                ((self.registers[rdn] as i32) >> shift) as u32
                            };
                            self.registers[rdn] = result;
                            self.set_nz_flags(result);
                        }
                        0x5 => {
                            let lhs = self.registers[rdn];
                            let rhs = self.registers[rm];
                            let carry_in = u32::from((self.xpsr >> 29) & 1 == 1);
                            let wide = lhs as u64 + rhs as u64 + carry_in as u64;
                            let result = wide as u32;
                            self.registers[rdn] = result;
                            self.set_nz_flags(result);
                            self.set_carry_flag(wide > u32::MAX as u64);
                            let overflow = (((lhs ^ result) & (rhs ^ result)) & 0x8000_0000) != 0;
                            self.set_overflow_flag(overflow);
                        }
                        0x6 => {
                            let lhs = self.registers[rdn];
                            let rhs = self.registers[rm];
                            let carry_in = u32::from((self.xpsr >> 29) & 1 == 1);
                            let borrow = 1_u32.wrapping_sub(carry_in);
                            let rhs_with_borrow = rhs.wrapping_add(borrow);
                            let result = lhs.wrapping_sub(rhs_with_borrow);
                            self.registers[rdn] = result;
                            self.set_nz_flags(result);
                            self.set_carry_flag((lhs as u64) >= (rhs as u64 + borrow as u64));
                            let overflow =
                                (((lhs ^ rhs_with_borrow) & (lhs ^ result)) & 0x8000_0000) != 0;
                            self.set_overflow_flag(overflow);
                        }
                        0x7 => {
                            let shift = (self.registers[rm] & 0xFF) as u32;
                            let value = self.registers[rdn];
                            let result = if shift == 0 {
                                value
                            } else {
                                value.rotate_right(shift & 31)
                            };
                            self.registers[rdn] = result;
                            self.set_nz_flags(result);
                        }
                        0x8 => {
                            let result = self.registers[rdn] & self.registers[rm];
                            self.set_nz_flags(result);
                        }
                        0x9 => {
                            let rhs = self.registers[rm];
                            let result = 0_u32.wrapping_sub(rhs);
                            self.registers[rdn] = result;
                            self.set_sub_flags(0, rhs, result);
                        }
                        0xA => {
                            let result = self.registers[rdn].wrapping_sub(self.registers[rm]);
                            self.set_sub_flags(self.registers[rdn], self.registers[rm], result);
                        }
                        0xB => {
                            let result = self.registers[rdn].wrapping_add(self.registers[rm]);
                            self.set_add_flags(self.registers[rdn], self.registers[rm], result);
                        }
                        0xC => {
                            let result = self.registers[rdn] | self.registers[rm];
                            self.registers[rdn] = result;
                            self.set_nz_flags(result);
                        }
                        0xD => {
                            let result = self.registers[rdn].wrapping_mul(self.registers[rm]);
                            self.registers[rdn] = result;
                            self.set_nz_flags(result);
                        }
                        0xE => {
                            let result = self.registers[rdn] & !self.registers[rm];
                            self.registers[rdn] = result;
                            self.set_nz_flags(result);
                        }
                        0xF => {
                            let result = !self.registers[rm];
                            self.registers[rdn] = result;
                            self.set_nz_flags(result);
                        }
                        _ => {
                            return Err(format!(
                                "unimplemented thumb data-processing op=0x{op:x} @ pc=0x{pc:08x}"
                            ));
                        }
                    }
                    self.registers[15] = self.registers[15].wrapping_add(2);
                    Ok(true)
                }
                0x4400 => {
                    let op = (opcode >> 8) & 0x3;
                    let rm = ((opcode >> 3) & 0xF) as usize;
                    let rd = ((opcode & 0x7) | ((opcode >> 4) & 0x8)) as usize;
                    match op {
                        0x0 => {
                            let result = self.registers[rd].wrapping_add(self.registers[rm]);
                            self.registers[rd] = result;
                            if rd == 15 {
                                self.registers[15] = result & !1;
                            } else {
                                self.registers[15] = self.registers[15].wrapping_add(2);
                            }
                        }
                        0x1 => {
                            let rhs = self.registers[rm];
                            let result = self.registers[rd].wrapping_sub(rhs);
                            self.set_sub_flags(self.registers[rd], rhs, result);
                            self.registers[15] = self.registers[15].wrapping_add(2);
                        }
                        0x2 => {
                            let value = self.registers[rm];
                            self.registers[rd] = value;
                            if rd == 15 {
                                self.registers[15] = value & !1;
                            } else {
                                self.registers[15] = self.registers[15].wrapping_add(2);
                            }
                        }
                        0x3 => {
                            let target = self.registers[rm];
                            if opcode & 0x0080 != 0 {
                                self.registers[14] = self.registers[15].wrapping_add(2) | 1;
                            }
                            self.registers[15] = target & !1;
                            if target & 1 == 0 {
                                self.xpsr &= !(1 << 24);
                            } else {
                                self.xpsr |= 1 << 24;
                            }
                        }
                        _ => return Ok(false),
                    }
                    Ok(true)
                }
                _ => Ok(false),
            },
        };
        let handled = handled?;
        if handled {
            return Ok(true);
        }

        match opcode & 0xFE00 {
            0x1C00 => {
                let imm3 = ((opcode >> 6) & 0x7) as u32;
                let rn = ((opcode >> 3) & 0x7) as usize;
                let rd = (opcode & 0x7) as usize;
                let result = self.registers[rn].wrapping_add(imm3);
                self.registers[rd] = result;
                self.set_add_flags(self.registers[rn], imm3, result);
                self.registers[15] = self.registers[15].wrapping_add(2);
                return Ok(true);
            }
            0x1E00 => {
                let imm3 = ((opcode >> 6) & 0x7) as u32;
                let rn = ((opcode >> 3) & 0x7) as usize;
                let rd = (opcode & 0x7) as usize;
                let result = self.registers[rn].wrapping_sub(imm3);
                self.registers[rd] = result;
                self.set_sub_flags(self.registers[rn], imm3, result);
                self.registers[15] = self.registers[15].wrapping_add(2);
                return Ok(true);
            }
            0x1A00 => {
                let rm = ((opcode >> 6) & 0x7) as usize;
                let rn = ((opcode >> 3) & 0x7) as usize;
                let rd = (opcode & 0x7) as usize;
                let result = self.registers[rn].wrapping_sub(self.registers[rm]);
                self.registers[rd] = result;
                self.set_sub_flags(self.registers[rn], self.registers[rm], result);
                self.registers[15] = self.registers[15].wrapping_add(2);
                return Ok(true);
            }
            0x5000 => {
                let rm = ((opcode >> 6) & 0x7) as usize;
                let rn = ((opcode >> 3) & 0x7) as usize;
                let rt = (opcode & 0x7) as usize;
                let addr = self.registers[rn].wrapping_add(self.registers[rm]);
                bus.write32(addr as u64, self.registers[rt])?;
                self.registers[15] = self.registers[15].wrapping_add(2);
                return Ok(true);
            }
            0x5200 => {
                let rm = ((opcode >> 6) & 0x7) as usize;
                let rn = ((opcode >> 3) & 0x7) as usize;
                let rt = (opcode & 0x7) as usize;
                let addr = self.registers[rn].wrapping_add(self.registers[rm]);
                bus.write16(addr as u64, self.registers[rt] as u16)?;
                self.registers[15] = self.registers[15].wrapping_add(2);
                return Ok(true);
            }
            0x5400 => {
                let rm = ((opcode >> 6) & 0x7) as usize;
                let rn = ((opcode >> 3) & 0x7) as usize;
                let rt = (opcode & 0x7) as usize;
                let addr = self.registers[rn].wrapping_add(self.registers[rm]);
                bus.write8(addr as u64, self.registers[rt] as u8)?;
                self.registers[15] = self.registers[15].wrapping_add(2);
                return Ok(true);
            }
            0x5600 => {
                let rm = ((opcode >> 6) & 0x7) as usize;
                let rn = ((opcode >> 3) & 0x7) as usize;
                let rt = (opcode & 0x7) as usize;
                let addr = self.registers[rn].wrapping_add(self.registers[rm]);
                self.registers[rt] = (bus.read8(addr as u64)? as i8) as i32 as u32;
                self.registers[15] = self.registers[15].wrapping_add(2);
                return Ok(true);
            }
            0x5800 => {
                let rm = ((opcode >> 6) & 0x7) as usize;
                let rn = ((opcode >> 3) & 0x7) as usize;
                let rt = (opcode & 0x7) as usize;
                let addr = self.registers[rn].wrapping_add(self.registers[rm]);
                self.registers[rt] = bus.read32(addr as u64)?;
                self.registers[15] = self.registers[15].wrapping_add(2);
                return Ok(true);
            }
            0x5A00 => {
                let rm = ((opcode >> 6) & 0x7) as usize;
                let rn = ((opcode >> 3) & 0x7) as usize;
                let rt = (opcode & 0x7) as usize;
                let addr = self.registers[rn].wrapping_add(self.registers[rm]);
                self.registers[rt] = bus.read16(addr as u64)? as u32;
                self.registers[15] = self.registers[15].wrapping_add(2);
                return Ok(true);
            }
            0x5C00 => {
                let rm = ((opcode >> 6) & 0x7) as usize;
                let rn = ((opcode >> 3) & 0x7) as usize;
                let rt = (opcode & 0x7) as usize;
                let addr = self.registers[rn].wrapping_add(self.registers[rm]);
                self.registers[rt] = bus.read8(addr as u64)? as u32;
                self.registers[15] = self.registers[15].wrapping_add(2);
                return Ok(true);
            }
            0x5E00 => {
                let rm = ((opcode >> 6) & 0x7) as usize;
                let rn = ((opcode >> 3) & 0x7) as usize;
                let rt = (opcode & 0x7) as usize;
                let addr = self.registers[rn].wrapping_add(self.registers[rm]);
                self.registers[rt] = (bus.read16(addr as u64)? as i16) as i32 as u32;
                self.registers[15] = self.registers[15].wrapping_add(2);
                return Ok(true);
            }
            0xB400 => {
                let reg_list = (opcode & 0xFF) as u8;
                let include_lr = (opcode >> 8) & 1 == 1;
                let mut count = reg_list.count_ones();
                if include_lr {
                    count += 1;
                }
                self.registers[13] = self.registers[13].wrapping_sub(count * 4);
                let mut address = self.registers[13];
                for reg in 0..8usize {
                    if (reg_list >> reg) & 1 == 1 {
                        bus.write32(address as u64, self.registers[reg])?;
                        address = address.wrapping_add(4);
                    }
                }
                if include_lr {
                    bus.write32(address as u64, self.registers[14])?;
                }
                self.registers[15] = self.registers[15].wrapping_add(2);
                return Ok(true);
            }
            0xBC00 => {
                let reg_list = (opcode & 0xFF) as u8;
                let include_pc = (opcode >> 8) & 1 == 1;
                let mut address = self.registers[13];
                for reg in 0..8 {
                    if (reg_list >> reg) & 1 == 1 {
                        self.registers[reg] = bus.read32(address as u64)?;
                        address = address.wrapping_add(4);
                    }
                }
                if include_pc {
                    let target = bus.read32(address as u64)?;
                    address = address.wrapping_add(4);
                    if Self::is_exc_return(target) {
                        self.registers[13] = address;
                        let returned = self.exception_return(bus)?;
                        if returned {
                            return Ok(true);
                        }
                    }
                    self.registers[15] = target & !1;
                    if target & 1 == 1 {
                        self.xpsr |= 1 << 24;
                    } else {
                        self.xpsr &= !(1 << 24);
                    }
                } else {
                    self.registers[15] = self.registers[15].wrapping_add(2);
                }
                self.registers[13] = address;
                return Ok(true);
            }
            _ => {}
        }

        match opcode & 0xF800 {
            0x0800 => {
                let imm5 = ((opcode >> 6) & 0x1F) as u32;
                let rm = ((opcode >> 3) & 0x7) as usize;
                let rd = (opcode & 0x7) as usize;
                let result = if imm5 == 0 {
                    0
                } else {
                    self.registers[rm] >> imm5
                };
                self.registers[rd] = result;
                self.set_nz_flags(result);
                self.registers[15] = self.registers[15].wrapping_add(2);
                return Ok(true);
            }
            0x0000 => {
                let imm5 = ((opcode >> 6) & 0x1F) as u32;
                let rm = ((opcode >> 3) & 0x7) as usize;
                let rd = (opcode & 0x7) as usize;
                let result = self.registers[rm] << imm5;
                self.registers[rd] = result;
                self.set_nz_flags(result);
                self.registers[15] = self.registers[15].wrapping_add(2);
                return Ok(true);
            }
            0x1800 => {
                // ADD/SUB (register or imm3), Thumb T1.
                let op = (opcode >> 9) & 0x1; // 0: ADD, 1: SUB
                let immediate = ((opcode >> 10) & 0x1) == 1;
                let rn = ((opcode >> 3) & 0x7) as usize;
                let rd = (opcode & 0x7) as usize;
                let rhs = if immediate {
                    ((opcode >> 6) & 0x7) as u32
                } else {
                    let rm = ((opcode >> 6) & 0x7) as usize;
                    self.registers[rm]
                };
                let lhs = self.registers[rn];
                let result = if op == 0 {
                    lhs.wrapping_add(rhs)
                } else {
                    lhs.wrapping_sub(rhs)
                };
                self.registers[rd] = result;
                if op == 0 {
                    self.set_add_flags(lhs, rhs, result);
                } else {
                    self.set_sub_flags(lhs, rhs, result);
                }
                self.registers[15] = self.registers[15].wrapping_add(2);
                return Ok(true);
            }
            0xE000 => {
                let imm11 = (opcode & 0x07FF) as i16;
                let signed = ((imm11 << 5) >> 4) as i32;
                let next_pc = self.registers[15].wrapping_add(4);
                self.registers[15] = next_pc.wrapping_add_signed(signed);
                return Ok(true);
            }
            _ => {}
        }

        match opcode & 0xF000 {
            0xD000 if opcode & 0x0F00 != 0x0F00 => {
                let cond = ((opcode >> 8) & 0xF) as u8;
                let imm8 = (opcode & 0xFF) as u8;
                let offset = (((imm8 as i8) as i32) << 1) + 4;
                let taken = self.condition_passed(cond);
                let target = self.registers[15].wrapping_add_signed(offset);

                if taken {
                    self.registers[15] = target;
                } else {
                    self.registers[15] = self.registers[15].wrapping_add(2);
                }
                return Ok(true);
            }
            0xC000 => {
                let rn = ((opcode >> 8) & 0x7) as usize;
                let reg_list = (opcode & 0xFF) as u8;
                let mut address = self.registers[rn];

                for reg in 0..8 {
                    if (reg_list >> reg) & 1 == 1 {
                        if opcode & 0x0800 == 0 {
                            bus.write32(address as u64, self.registers[reg])?;
                        } else {
                            self.registers[reg] = bus.read32(address as u64)?;
                        }
                        address = address.wrapping_add(4);
                    }
                }

                self.registers[rn] = address;
                self.registers[15] = self.registers[15].wrapping_add(2);
                return Ok(true);
            }
            _ => {}
        }

        match opcode & 0xFF00 {
            0x4600 => {
                let rm = ((opcode >> 3) & 0xF) as usize;
                let rd = ((opcode & 0x7) | ((opcode >> 4) & 0x8)) as usize;
                let value = self.registers[rm];
                if rd == 15 && Self::is_exc_return(value) {
                    let returned = self.exception_return(bus)?;
                    if returned {
                        return Ok(true);
                    }
                }
                self.registers[rd] = value;
                if rd == 15 {
                    self.registers[15] = value & !1;
                } else {
                    self.registers[15] = self.registers[15].wrapping_add(2);
                }
                return Ok(true);
            }
            0xBF00 if (opcode & 0x000F) == 0x8 => {
                let cond = ((opcode >> 4) & 0xF) as u8;
                self.set_it_state(cond, 1);
                self.registers[15] = self.registers[15].wrapping_add(2);
                return Ok(true);
            }
            0xBF00 if (opcode & 0x000F) != 0 => {
                let cond = ((opcode >> 4) & 0xF) as u8;
                let mask = (opcode & 0xF) as u8;
                let count = Self::it_count_from_mask(mask)
                    .ok_or_else(|| format!("unimplemented IT mask 0x{mask:x} at PC 0x{pc:08x}"))?;
                self.set_it_state(cond, count);
                self.registers[15] = self.registers[15].wrapping_add(2);
                return Ok(true);
            }
            _ => {}
        }

        match opcode & 0xFFC0 {
            0xB2C0 => {
                let rm = ((opcode >> 3) & 0x7) as usize;
                let rd = (opcode & 0x7) as usize;
                let result = self.registers[rm] & 0xFF;
                self.registers[rd] = result;
                self.registers[15] = self.registers[15].wrapping_add(2);
                return Ok(true);
            }
            0xB280 => {
                let rm = ((opcode >> 3) & 0x7) as usize;
                let rd = (opcode & 0x7) as usize;
                let result = self.registers[rm] & 0xFFFF;
                self.registers[rd] = result;
                self.registers[15] = self.registers[15].wrapping_add(2);
                return Ok(true);
            }
            0x4280 => {
                let rm = (opcode & 0x7) as usize;
                let rd = ((opcode >> 3) & 0x7) as usize;
                let result = self.registers[rd].wrapping_sub(self.registers[rm]);
                self.set_sub_flags(self.registers[rd], self.registers[rm], result);
                self.registers[15] = self.registers[15].wrapping_add(2);
                return Ok(true);
            }
            0xBA00 => {
                let rm = ((opcode >> 3) & 0x7) as usize;
                let rd = (opcode & 0x7) as usize;
                self.registers[rd] = self.registers[rm].swap_bytes();
                self.registers[15] = self.registers[15].wrapping_add(2);
                return Ok(true);
            }
            0xBA40 => {
                let rm = ((opcode >> 3) & 0x7) as usize;
                let rd = (opcode & 0x7) as usize;
                let value = self.registers[rm];
                let lo = (value & 0x00FF_00FF) << 8;
                let hi = (value & 0xFF00_FF00) >> 8;
                self.registers[rd] = lo | hi;
                self.registers[15] = self.registers[15].wrapping_add(2);
                return Ok(true);
            }
            0xBAC0 => {
                let rm = ((opcode >> 3) & 0x7) as usize;
                let rd = (opcode & 0x7) as usize;
                let value = self.registers[rm];
                let low_half = ((value & 0x00FF) << 8) | ((value >> 8) & 0x00FF);
                self.registers[rd] = (low_half as i16) as i32 as u32;
                self.registers[15] = self.registers[15].wrapping_add(2);
                return Ok(true);
            }
            _ => {}
        }

        match opcode & 0xF500 {
            0xB100 => {
                let op = (opcode >> 11) & 1;
                let imm5 = ((opcode >> 3) & 0x1F) as u32;
                let rn = (opcode & 0x7) as usize;
                let offset = ((imm5 << 1) | (((opcode >> 9) & 1) as u32) << 6) as i32 + 4;
                let value = self.registers[rn];
                let taken = if op == 0 { value == 0 } else { value != 0 };
                if taken {
                    self.registers[15] = self.registers[15].wrapping_add_signed(offset);
                } else {
                    self.registers[15] = self.registers[15].wrapping_add(2);
                }
                return Ok(true);
            }
            _ => {}
        }

        match opcode & 0xFF80 {
            0xB080 => {
                let imm7 = (opcode & 0x7F) as u32;
                self.registers[13] = self.registers[13].wrapping_sub(imm7 << 2);
                self.registers[15] = self.registers[15].wrapping_add(2);
                return Ok(true);
            }
            0xB000 => {
                let imm7 = (opcode & 0x7F) as u32;
                self.registers[13] = self.registers[13].wrapping_add(imm7 << 2);
                self.registers[15] = self.registers[15].wrapping_add(2);
                return Ok(true);
            }
            _ => {}
        }

        match opcode & 0xFF87 {
            0x4700 => {
                let rm = ((opcode >> 3) & 0xF) as usize;
                let target = self.registers[rm];
                if Self::is_exc_return(target) {
                    let returned = self.exception_return(bus)?;
                    if returned {
                        return Ok(true);
                    }
                }
                if opcode & 0x0080 != 0 {
                    self.registers[14] = self.registers[15].wrapping_add(2) | 1;
                }
                self.registers[15] = target & !1;
                if target & 1 == 0 {
                    self.xpsr &= !(1 << 24);
                } else {
                    self.xpsr |= 1 << 24;
                }
                return Ok(true);
            }
            _ => {}
        }

        match opcode {
            0xB672 | 0xB662 | 0xBF00 | 0xBF30 => {
                self.registers[15] = self.registers[15].wrapping_add(2);
                return Ok(true);
            }
            0xFA1F => {
                let opcode2 = bus.read16(pc + 2)?;
                if opcode2 & 0xF0F0 == 0xF080 {
                    let rm = (opcode2 & 0xF) as usize;
                    let rd = ((opcode2 >> 8) & 0xF) as usize;
                    let result = self.registers[rm] & 0xFFFF;
                    self.registers[rd] = result;
                    self.registers[15] = self.registers[15].wrapping_add(4);
                    return Ok(true);
                }
            }
            _ => {}
        }

        Err(format!(
            "unimplemented Thumb instruction 0x{opcode:04x} at PC 0x{pc:08x}"
        ))
    }

    fn step32_fast(
        &mut self,
        bus: &mut dyn SystemBus,
        pc: u64,
        opcode: u16,
        opcode2: u16,
    ) -> Result<bool, String> {
        let handled: Result<bool, String> = match opcode & 0xFF00 {
            0xEA00 | 0xEA40 | 0xEB00 | 0xEBA0 => {
                let rn = (opcode & 0xF) as usize;
                let rd = ((opcode2 >> 8) & 0xF) as usize;
                let rm = (opcode2 & 0xF) as usize;
                let imm3 = ((opcode2 >> 12) & 0x7) as u32;
                let imm2 = ((opcode2 >> 6) & 0x3) as u32;
                let shift_type = (opcode2 >> 4) & 0x3;
                let shifted = Self::shift_imm(self.registers[rm], shift_type, (imm3 << 2) | imm2);

                match opcode & 0xFF00 {
                    0xEA00 => {
                        if rn == 15 {
                            self.set_nz_flags(shifted);
                        } else {
                            self.registers[rd] = self.registers[rn] & shifted;
                        }
                    }
                    0xEA40 => {
                        self.registers[rd] = if rn == 15 {
                            shifted
                        } else {
                            self.registers[rn] | shifted
                        };
                    }
                    0xEB00 => {
                        let result = self.registers[rn].wrapping_add(shifted);
                        if rd == 15 {
                            self.set_add_flags(self.registers[rn], shifted, result);
                        } else {
                            self.registers[rd] = result;
                        }
                    }
                    0xEBA0 => {
                        let result = self.registers[rn].wrapping_sub(shifted);
                        if rd == 15 {
                            self.set_sub_flags(self.registers[rn], shifted, result);
                        } else {
                            self.registers[rd] = result;
                        }
                    }
                    _ => unreachable!(),
                }

                self.registers[15] = self.registers[15].wrapping_add(4);
                Ok(true)
            }
            0xEA60 => {
                let rd = ((opcode2 >> 8) & 0xF) as usize;
                let rm = (opcode2 & 0xF) as usize;
                let imm3 = ((opcode2 >> 12) & 0x7) as u32;
                let imm2 = ((opcode2 >> 6) & 0x3) as u32;
                let shift_type = (opcode2 >> 4) & 0x3;
                let shifted = Self::shift_imm(self.registers[rm], shift_type, (imm3 << 2) | imm2);
                self.registers[rd] = !shifted;
                self.registers[15] = self.registers[15].wrapping_add(4);
                Ok(true)
            }
            _ => match opcode & 0xFFF0 {
                0xE880 | 0xE890 | 0xE8A0 | 0xE8B0 => {
                    let rn = (opcode & 0xF) as usize;
                    let reglist = opcode2 as u32;
                    let writeback = matches!(opcode & 0xFFF0, 0xE8A0 | 0xE8B0);
                    let is_load = matches!(opcode & 0xFFF0, 0xE890 | 0xE8B0);
                    let mut addr = self.registers[rn];
                    let count = reglist.count_ones();

                    if is_load {
                        for reg in 0..16usize {
                            if (reglist >> reg) & 1 == 0 {
                                continue;
                            }

                            let value = bus.read32(addr as u64)?;
                            addr = addr.wrapping_add(4);
                            if reg == 15 {
                                self.registers[15] = value & !1;
                                if value & 1 == 0 {
                                    self.xpsr &= !(1 << 24);
                                } else {
                                    self.xpsr |= 1 << 24;
                                }
                            } else {
                                self.registers[reg] = value;
                            }
                        }
                    } else {
                        for reg in 0..16usize {
                            if (reglist >> reg) & 1 == 0 {
                                continue;
                            }

                            bus.write32(addr as u64, self.registers[reg])?;
                            addr = addr.wrapping_add(4);
                        }
                    }

                    if writeback {
                        self.registers[rn] = self.registers[rn].wrapping_add(count * 4);
                    }

                    if !is_load || (reglist & (1 << 15) == 0) {
                        self.registers[15] = self.registers[15].wrapping_add(4);
                    }
                    Ok(true)
                }
                0xE9C0 => {
                    let rn = (opcode & 0xF) as usize;
                    let rt = ((opcode2 >> 12) & 0xF) as usize;
                    let rt2 = ((opcode2 >> 8) & 0xF) as usize;
                    let imm8 = (opcode2 & 0xFF) as u32;
                    let addr = self.registers[rn].wrapping_add(imm8 << 2);
                    bus.write32(addr as u64, self.registers[rt])?;
                    bus.write32(addr.wrapping_add(4) as u64, self.registers[rt2])?;
                    self.registers[15] = self.registers[15].wrapping_add(4);
                    Ok(true)
                }
                0xE9D0 => {
                    let rn = (opcode & 0xF) as usize;
                    let rt = ((opcode2 >> 12) & 0xF) as usize;
                    let rt2 = ((opcode2 >> 8) & 0xF) as usize;
                    let imm8 = (opcode2 & 0xFF) as u32;
                    let addr = self.registers[rn].wrapping_add(imm8 << 2);
                    self.registers[rt] = bus.read32(addr as u64)?;
                    self.registers[rt2] = bus.read32(addr.wrapping_add(4) as u64)?;
                    self.registers[15] = self.registers[15].wrapping_add(4);
                    Ok(true)
                }
                _ => match opcode & 0xF800 {
                    0xF000 => Ok(false),
                    _ => Ok(false),
                },
            },
        };
        let handled = handled?;
        if handled {
            return Ok(true);
        }
        match opcode & 0xF000 {
            0xF000 => {
                match opcode & 0xFFF0 {
                    0xF880 => {
                        let rn = (opcode & 0xF) as usize;
                        let rt = ((opcode2 >> 12) & 0xF) as usize;
                        let imm12 = (opcode2 & 0x0FFF) as u32;
                        let addr = self.registers[rn].wrapping_add(imm12);
                        bus.write8(addr as u64, self.registers[rt] as u8)?;
                        self.registers[15] = self.registers[15].wrapping_add(4);
                        return Ok(true);
                    }
                    0xF890 => {
                        let rn = (opcode & 0xF) as usize;
                        let rt = ((opcode2 >> 12) & 0xF) as usize;
                        let imm12 = (opcode2 & 0x0FFF) as u32;
                        let addr = self.registers[rn].wrapping_add(imm12);
                        self.registers[rt] = bus.read8(addr as u64)? as u32;
                        self.registers[15] = self.registers[15].wrapping_add(4);
                        return Ok(true);
                    }
                    0xF990 => {
                        let rn = (opcode & 0xF) as usize;
                        let rt = ((opcode2 >> 12) & 0xF) as usize;
                        let imm12 = (opcode2 & 0x0FFF) as u32;
                        let addr = self.registers[rn].wrapping_add(imm12);
                        self.registers[rt] = (bus.read8(addr as u64)? as i8) as i32 as u32;
                        self.registers[15] = self.registers[15].wrapping_add(4);
                        return Ok(true);
                    }
                    0xF910 => match (opcode2 & 0x0F00, opcode2 & 0x00F0) {
                        (0x0C00, _) => {
                            let rn = (opcode & 0xF) as usize;
                            let rt = ((opcode2 >> 12) & 0xF) as usize;
                            let imm8 = (opcode2 & 0xFF) as u32;
                            let addr = self.registers[rn].wrapping_sub(imm8);
                            self.registers[rt] = (bus.read8(addr as u64)? as i8) as i32 as u32;
                            self.registers[15] = self.registers[15].wrapping_add(4);
                            return Ok(true);
                        }
                        (_, 0x0000) => {
                            let rn = (opcode & 0xF) as usize;
                            let rt = ((opcode2 >> 12) & 0xF) as usize;
                            let rm = (opcode2 & 0xF) as usize;
                            let shift = ((opcode2 >> 4) & 0x3) as u32;
                            let addr = self.registers[rn].wrapping_add(self.registers[rm] << shift);
                            self.registers[rt] = (bus.read8(addr as u64)? as i8) as i32 as u32;
                            self.registers[15] = self.registers[15].wrapping_add(4);
                            return Ok(true);
                        }
                        _ => {}
                    },
                    0xF8C0 => {
                        let rn = (opcode & 0xF) as usize;
                        let rt = ((opcode2 >> 12) & 0xF) as usize;
                        let imm12 = (opcode2 & 0x0FFF) as u32;
                        let addr = self.registers[rn].wrapping_add(imm12);
                        bus.write32(addr as u64, self.registers[rt])?;
                        self.registers[15] = self.registers[15].wrapping_add(4);
                        return Ok(true);
                    }
                    0xF840 => match ((opcode & 0xF) == 13, opcode2 & 0x0F00, opcode2 & 0x00F0) {
                        (true, 0x0D00, _) => {
                            let rn = (opcode & 0xF) as usize;
                            let rt = ((opcode2 >> 12) & 0xF) as usize;
                            let imm8 = (opcode2 & 0xFF) as u32;
                            let addr = self.registers[rn].wrapping_sub(imm8);
                            bus.write32(addr as u64, self.registers[rt])?;
                            self.registers[rn] = addr;
                            self.registers[15] = self.registers[15].wrapping_add(4);
                            return Ok(true);
                        }
                        (_, 0x0C00, _) => {
                            let rn = (opcode & 0xF) as usize;
                            let rt = ((opcode2 >> 12) & 0xF) as usize;
                            let imm8 = (opcode2 & 0xFF) as u32;
                            let addr = self.registers[rn].wrapping_sub(imm8);
                            bus.write32(addr as u64, self.registers[rt])?;
                            self.registers[15] = self.registers[15].wrapping_add(4);
                            return Ok(true);
                        }
                        (_, _, 0x0000) => {
                            let rn = (opcode & 0xF) as usize;
                            let rt = ((opcode2 >> 12) & 0xF) as usize;
                            let rm = (opcode2 & 0xF) as usize;
                            let shift = ((opcode2 >> 4) & 0x3) as u32;
                            let addr = self.registers[rn].wrapping_add(self.registers[rm] << shift);
                            bus.write32(addr as u64, self.registers[rt])?;
                            self.registers[15] = self.registers[15].wrapping_add(4);
                            return Ok(true);
                        }
                        _ => {}
                    },
                    0xF850 => match ((opcode & 0xF) == 13, opcode2 & 0x0F00, opcode2 & 0x00F0) {
                        (true, 0x0B00, _) => {
                            let rn = (opcode & 0xF) as usize;
                            let rt = ((opcode2 >> 12) & 0xF) as usize;
                            let imm8 = (opcode2 & 0xFF) as u32;
                            let addr = self.registers[rn];
                            self.registers[rt] = bus.read32(addr as u64)?;
                            self.registers[rn] = self.registers[rn].wrapping_add(imm8);
                            self.registers[15] = self.registers[15].wrapping_add(4);
                            return Ok(true);
                        }
                        (_, 0x0C00, _) => {
                            let rn = (opcode & 0xF) as usize;
                            let rt = ((opcode2 >> 12) & 0xF) as usize;
                            let imm8 = (opcode2 & 0xFF) as u32;
                            let addr = self.registers[rn].wrapping_sub(imm8);
                            self.registers[rt] = bus.read32(addr as u64)?;
                            self.registers[15] = self.registers[15].wrapping_add(4);
                            return Ok(true);
                        }
                        (_, _, 0x0000) => {}
                        (_, _, _) => {
                            let rn = (opcode & 0xF) as usize;
                            let rt = ((opcode2 >> 12) & 0xF) as usize;
                            let rm = (opcode2 & 0xF) as usize;
                            let shift = ((opcode2 >> 4) & 0x3) as u32;
                            let addr = self.registers[rn].wrapping_add(self.registers[rm] << shift);
                            self.registers[rt] = bus.read32(addr as u64)?;
                            self.registers[15] = self.registers[15].wrapping_add(4);
                            return Ok(true);
                        }
                    },
                    0xF8D0 => {
                        let rn = (opcode & 0xF) as usize;
                        let rt = ((opcode2 >> 12) & 0xF) as usize;
                        let imm12 = (opcode2 & 0x0FFF) as u32;
                        let addr = self.registers[rn].wrapping_add(imm12);
                        self.registers[rt] = bus.read32(addr as u64)?;
                        self.registers[15] = self.registers[15].wrapping_add(4);
                        return Ok(true);
                    }
                    0xF800 | 0xF810 | 0xF820 | 0xF830 => {
                        match (opcode & 0xFFF0, opcode2 & 0x0F00, opcode2 & 0x00F0) {
                            (0xF800, 0x0C00, _) => {
                                let rn = (opcode & 0xF) as usize;
                                let rt = ((opcode2 >> 12) & 0xF) as usize;
                                let imm8 = (opcode2 & 0xFF) as u32;
                                let addr = self.registers[rn].wrapping_sub(imm8);
                                bus.write8(addr as u64, self.registers[rt] as u8)?;
                                self.registers[15] = self.registers[15].wrapping_add(4);
                                return Ok(true);
                            }
                            (0xF800, _, 0x0000) => {
                                let rn = (opcode & 0xF) as usize;
                                let rt = ((opcode2 >> 12) & 0xF) as usize;
                                let rm = (opcode2 & 0xF) as usize;
                                let shift = ((opcode2 >> 4) & 0x3) as u32;
                                let addr =
                                    self.registers[rn].wrapping_add(self.registers[rm] << shift);
                                bus.write8(addr as u64, self.registers[rt] as u8)?;
                                self.registers[15] = self.registers[15].wrapping_add(4);
                                return Ok(true);
                            }
                            (0xF810, 0x0C00, _) => {
                                let rn = (opcode & 0xF) as usize;
                                let rt = ((opcode2 >> 12) & 0xF) as usize;
                                let imm8 = (opcode2 & 0xFF) as u32;
                                let addr = self.registers[rn].wrapping_sub(imm8);
                                self.registers[rt] = bus.read8(addr as u64)? as u32;
                                self.registers[15] = self.registers[15].wrapping_add(4);
                                return Ok(true);
                            }
                            (0xF810, 0x0B00, _) => {
                                let rn = (opcode & 0xF) as usize;
                                let rt = ((opcode2 >> 12) & 0xF) as usize;
                                let imm8 = (opcode2 & 0xFF) as u32;
                                let addr = self.registers[rn];
                                self.registers[rt] = bus.read8(addr as u64)? as u32;
                                self.registers[rn] = self.registers[rn].wrapping_add(imm8);
                                self.registers[15] = self.registers[15].wrapping_add(4);
                                return Ok(true);
                            }
                            (0xF810, _, 0x0000) => {
                                let rn = (opcode & 0xF) as usize;
                                let rt = ((opcode2 >> 12) & 0xF) as usize;
                                let rm = (opcode2 & 0xF) as usize;
                                let shift = ((opcode2 >> 4) & 0x3) as u32;
                                let addr =
                                    self.registers[rn].wrapping_add(self.registers[rm] << shift);
                                self.registers[rt] = bus.read8(addr as u64)? as u32;
                                self.registers[15] = self.registers[15].wrapping_add(4);
                                return Ok(true);
                            }
                            (0xF820, 0x0C00, _) => {
                                let rn = (opcode & 0xF) as usize;
                                let rt = ((opcode2 >> 12) & 0xF) as usize;
                                let imm8 = (opcode2 & 0xFF) as u32;
                                let addr = self.registers[rn].wrapping_sub(imm8);
                                bus.write16(addr as u64, self.registers[rt] as u16)?;
                                self.registers[15] = self.registers[15].wrapping_add(4);
                                return Ok(true);
                            }
                            (0xF820, _, 0x0000) | (0xF820, _, 0x0010) => {
                                let rn = (opcode & 0xF) as usize;
                                let rt = ((opcode2 >> 12) & 0xF) as usize;
                                let rm = (opcode2 & 0xF) as usize;
                                let shift = ((opcode2 >> 4) & 0x3) as u32;
                                let addr =
                                    self.registers[rn].wrapping_add(self.registers[rm] << shift);
                                bus.write16(addr as u64, self.registers[rt] as u16)?;
                                self.registers[15] = self.registers[15].wrapping_add(4);
                                return Ok(true);
                            }
                            (0xF830, 0x0C00, _) => {
                                let rn = (opcode & 0xF) as usize;
                                let rt = ((opcode2 >> 12) & 0xF) as usize;
                                let imm8 = (opcode2 & 0xFF) as u32;
                                let addr = self.registers[rn].wrapping_sub(imm8);
                                self.registers[rt] = bus.read16(addr as u64)? as u32;
                                self.registers[15] = self.registers[15].wrapping_add(4);
                                return Ok(true);
                            }
                            (0xF830, _, 0x0000) | (0xF830, _, 0x0010) => {
                                let rn = (opcode & 0xF) as usize;
                                let rt = ((opcode2 >> 12) & 0xF) as usize;
                                let rm = (opcode2 & 0xF) as usize;
                                let shift = ((opcode2 >> 4) & 0x3) as u32;
                                let addr =
                                    self.registers[rn].wrapping_add(self.registers[rm] << shift);
                                self.registers[rt] = bus.read16(addr as u64)? as u32;
                                self.registers[15] = self.registers[15].wrapping_add(4);
                                return Ok(true);
                            }
                            _ => {}
                        }
                    }
                    0xF8A0 => {
                        let rn = (opcode & 0xF) as usize;
                        let rt = ((opcode2 >> 12) & 0xF) as usize;
                        let imm12 = (opcode2 & 0x0FFF) as u32;
                        let addr = self.registers[rn].wrapping_add(imm12);
                        bus.write16(addr as u64, self.registers[rt] as u16)?;
                        self.registers[15] = self.registers[15].wrapping_add(4);
                        return Ok(true);
                    }
                    0xF8B0 => {
                        let rn = (opcode & 0xF) as usize;
                        let rt = ((opcode2 >> 12) & 0xF) as usize;
                        let imm12 = (opcode2 & 0x0FFF) as u32;
                        let addr = self.registers[rn].wrapping_add(imm12);
                        self.registers[rt] = bus.read16(addr as u64)? as u32;
                        self.registers[15] = self.registers[15].wrapping_add(4);
                        return Ok(true);
                    }
                    0xFA00..=0xFA5F => {
                        match (opcode, opcode2 & 0xFF00) {
                            (0xFA0F | 0xFA1F | 0xFA4F | 0xFA5F, 0xF800) => {
                                let rd = ((opcode2 >> 8) & 0xF) as usize;
                                let rm = (opcode2 & 0xF) as usize;
                                let rot = ((opcode2 >> 4) & 0x3) as u32;
                                let val = self.registers[rm].rotate_right(rot * 8);
                                let result = match opcode {
                                    0xFA0F => (val as i16) as i32 as u32, // SXTH
                                    0xFA1F => val & 0xFFFF,               // UXTH
                                    0xFA4F => (val as i8) as i32 as u32,  // SXTB
                                    0xFA5F => val & 0xFF,                 // UXTB
                                    _ => unreachable!(),
                                };
                                self.registers[rd] = result;
                                self.registers[15] = self.registers[15].wrapping_add(4);
                                return Ok(true);
                            }
                            _ => {
                                if opcode & 0xFFF0 == 0xFAB0 && (opcode2 & 0xF0F0 == 0xF080) {
                                    let rd = ((opcode2 >> 8) & 0xF) as usize;
                                    let rm = (opcode2 & 0xF) as usize;
                                    self.registers[rd] = self.registers[rm].leading_zeros();
                                    self.registers[15] = self.registers[15].wrapping_add(4);
                                    return Ok(true);
                                }
                            }
                        }
                    }
                    0xFBA0 => {
                        let rn = (opcode & 0xF) as usize;
                        let rd_lo = ((opcode2 >> 12) & 0xF) as usize;
                        let rd_hi = ((opcode2 >> 8) & 0xF) as usize;
                        let rm = (opcode2 & 0xF) as usize;
                        let product = (self.registers[rn] as u64) * (self.registers[rm] as u64);
                        self.registers[rd_lo] = product as u32;
                        self.registers[rd_hi] = (product >> 32) as u32;
                        self.registers[15] = self.registers[15].wrapping_add(4);
                        return Ok(true);
                    }
                    0xFB00 => {
                        let rn = (opcode & 0xF) as usize;
                        let rd = ((opcode2 >> 8) & 0xF) as usize;
                        let ra = ((opcode2 >> 12) & 0xF) as usize;
                        let rm = (opcode2 & 0xF) as usize;
                        match opcode2 & 0x00F0 {
                            // MUL / MLA (A8.8.96 / A8.8.98): accumulate register is RA.
                            // For MUL, RA is encoded as R15 and ignored.
                            0x0000 => {
                                let mut result =
                                    self.registers[rn].wrapping_mul(self.registers[rm]);
                                if ra != 15 {
                                    result = result.wrapping_add(self.registers[ra]);
                                }
                                self.registers[rd] = result;
                            }
                            // MLS (A8.8.97): Rd = Ra - (Rn * Rm)
                            0x0010 => {
                                self.registers[rd] = self.registers[ra]
                                    .wrapping_sub(
                                        self.registers[rn].wrapping_mul(self.registers[rm]),
                                    );
                            }
                            _ => return Ok(false),
                        }
                        self.registers[15] = self.registers[15].wrapping_add(4);
                        return Ok(true);
                    }
                    0xFBB0 => match opcode2 & 0xF0F0 {
                        0xF0F0 => {
                            let rn = (opcode & 0xF) as usize;
                            let rm = (opcode2 & 0xF) as usize;
                            let rd = ((opcode2 >> 8) & 0xF) as usize;
                            if self.registers[rm] == 0 {
                                return Err(format!("division by zero at PC 0x{pc:08x}"));
                            }
                            self.registers[rd] = self.registers[rn] / self.registers[rm];
                            self.registers[15] = self.registers[15].wrapping_add(4);
                            return Ok(true);
                        }
                        _ => {}
                    },
                    _ => {}
                }

                match opcode & 0xFBF0 {
                    0xF3C0 => {
                        // UBFX (unsigned bitfield extract)
                        let rn = (opcode & 0xF) as usize;
                        let rd = ((opcode2 >> 8) & 0xF) as usize;
                        let imm3 = ((opcode2 >> 12) & 0x7) as u32;
                        let imm2 = ((opcode2 >> 6) & 0x3) as u32;
                        let lsb = (imm3 << 2) | imm2;
                        let width = ((opcode2 & 0x1F) as u32) + 1;
                        let mask = if width >= 32 {
                            u32::MAX
                        } else {
                            (1u32 << width) - 1
                        };
                        self.registers[rd] = (self.registers[rn] >> lsb) & mask;
                        self.registers[15] = self.registers[15].wrapping_add(4);
                        return Ok(true);
                    }
                    0xF360 => {
                        // BFI/BFC
                        let rn = (opcode & 0xF) as usize;
                        let rd = ((opcode2 >> 8) & 0xF) as usize;
                        let imm3 = ((opcode2 >> 12) & 0x7) as u32;
                        let imm2 = ((opcode2 >> 6) & 0x3) as u32;
                        let lsb = (imm3 << 2) | imm2;
                        let msb = (opcode2 & 0x1F) as u32;
                        if msb < lsb {
                            return Err(format!("invalid bitfield at PC 0x{pc:08x}"));
                        }
                        let width = msb - lsb + 1;
                        let field_mask = if width >= 32 {
                            u32::MAX
                        } else {
                            ((1u32 << width) - 1) << lsb
                        };
                        if rn == 15 {
                            // BFC
                            self.registers[rd] &= !field_mask;
                        } else {
                            // BFI
                            let insert = (self.registers[rn] << lsb) & field_mask;
                            self.registers[rd] = (self.registers[rd] & !field_mask) | insert;
                        }
                        self.registers[15] = self.registers[15].wrapping_add(4);
                        return Ok(true);
                    }
                    0xF240 => {
                        let rd = ((opcode2 >> 8) & 0xF) as usize;
                        let imm16 = (((opcode & 0xF) as u32) << 12)
                            | ((((opcode >> 10) & 1) as u32) << 11)
                            | ((((opcode2 >> 12) & 0x7) as u32) << 8)
                            | (opcode2 & 0xFF) as u32;
                        self.registers[rd] = imm16;
                        self.registers[15] = self.registers[15].wrapping_add(4);
                        return Ok(true);
                    }
                    0xF2C0 => {
                        let rd = ((opcode2 >> 8) & 0xF) as usize;
                        let imm16 = (((opcode & 0xF) as u32) << 12)
                            | ((((opcode >> 10) & 1) as u32) << 11)
                            | ((((opcode2 >> 12) & 0x7) as u32) << 8)
                            | (opcode2 & 0xFF) as u32;
                        self.registers[rd] = (self.registers[rd] & 0x0000_FFFF) | (imm16 << 16);
                        self.registers[15] = self.registers[15].wrapping_add(4);
                        return Ok(true);
                    }
                    0xF100 | 0xF500 => {
                        let rn = (opcode & 0xF) as usize;
                        let rd = ((opcode2 >> 8) & 0xF) as usize;
                        let imm12 = ((((opcode >> 10) & 1) as u32) << 11)
                            | ((((opcode2 >> 12) & 0x7) as u32) << 8)
                            | (opcode2 & 0xFF) as u32;
                        let imm32 = Self::thumb_expand_imm12(imm12);
                        let result = self.registers[rn].wrapping_add(imm32);
                        if rd == 15 {
                            self.set_add_flags(self.registers[rn], imm32, result);
                        } else {
                            self.registers[rd] = result;
                        }
                        self.registers[15] = self.registers[15].wrapping_add(4);
                        return Ok(true);
                    }
                    0xF1C0 => {
                        let rn = (opcode & 0xF) as usize;
                        let rd = ((opcode2 >> 8) & 0xF) as usize;
                        let imm12 = ((((opcode >> 10) & 1) as u32) << 11)
                            | ((((opcode2 >> 12) & 0x7) as u32) << 8)
                            | (opcode2 & 0xFF) as u32;
                        let imm32 = Self::thumb_expand_imm12(imm12);
                        self.registers[rd] = imm32.wrapping_sub(self.registers[rn]);
                        self.registers[15] = self.registers[15].wrapping_add(4);
                        return Ok(true);
                    }
                    _ => {}
                }

                match opcode & 0xFBE0 {
                    0xF1A0 | 0xF5A0 => {
                        let rn = (opcode & 0xF) as usize;
                        let rd = ((opcode2 >> 8) & 0xF) as usize;
                        let setflags = (opcode & 0x0010) != 0;
                        let imm12 = ((((opcode >> 10) & 1) as u32) << 11)
                            | ((((opcode2 >> 12) & 0x7) as u32) << 8)
                            | (opcode2 & 0xFF) as u32;
                        let imm32 = Self::thumb_expand_imm12(imm12);
                        if rd == 15 {
                            let result = self.registers[rn].wrapping_sub(imm32);
                            self.set_sub_flags(self.registers[rn], imm32, result);
                        } else {
                            let result = self.registers[rn].wrapping_sub(imm32);
                            self.registers[rd] = result;
                            if setflags {
                                self.set_sub_flags(self.registers[rn], imm32, result);
                            }
                        }
                        self.registers[15] = self.registers[15].wrapping_add(4);
                        return Ok(true);
                    }
                    _ => {}
                }
            }
            _ => {}
        }

        match opcode & 0xFFB0 {
            0xFAB0 => {
                match opcode2 & 0xF0F0 {
                    0xF080 => {
                        let rm = (opcode2 & 0xF) as usize;
                        let rd = ((opcode2 >> 8) & 0xF) as usize;
                        let result = self.registers[rm].leading_zeros();
                        self.registers[rd] = result;
                        self.registers[15] = self.registers[15].wrapping_add(4);
                        return Ok(true);
                    }
                    _ => {}
                }
            }
            _ => {}
        }

        match opcode & 0xFFF0 {
            0xE880 => {
                let rn = (opcode & 0xF) as usize;
                let writeback = (opcode2 >> 5) & 1 == 1;
                let reg_list = opcode2 as u16;
                let mut address = self.registers[rn];
                for reg in 0..16 {
                    if (reg_list >> reg) & 1 == 1 {
                        let value = self.registers[reg];
                        bus.write32(address as u64, value)?;
                        address = address.wrapping_add(4);
                    }
                }
                if writeback {
                    self.registers[rn] = address;
                }
                self.registers[15] = self.registers[15].wrapping_add(4);
                return Ok(true);
            }
            0xE920 => {
                let rn = (opcode & 0xF) as usize;
                let reg_list = opcode2 as u16;
                let count = reg_list.count_ones();
                let mut address = self.registers[rn].wrapping_sub(count * 4);
                for reg in 0..16 {
                    if (reg_list >> reg) & 1 == 1 {
                        bus.write32(address as u64, self.registers[reg])?;
                        address = address.wrapping_add(4);
                    }
                }
                self.registers[rn] = self.registers[rn].wrapping_sub(count * 4);
                self.registers[15] = self.registers[15].wrapping_add(4);
                return Ok(true);
            }
            0xE8B0 => {
                let rn = (opcode & 0xF) as usize;
                let reg_list = opcode2 as u16;
                let mut address = self.registers[rn];
                for reg in 0..16 {
                    if (reg_list >> reg) & 1 == 1 {
                        let value = bus.read32(address as u64)?;
                        if reg == 15 {
                            self.registers[15] = value & !1;
                            if value & 1 == 1 {
                                self.xpsr |= 1 << 24;
                            } else {
                                self.xpsr &= !(1 << 24);
                            }
                        } else {
                            self.registers[reg] = value;
                        }
                        address = address.wrapping_add(4);
                    }
                }
                self.registers[rn] = address;
                if (reg_list >> 15) & 1 == 0 {
                    self.registers[15] = self.registers[15].wrapping_add(4);
                }
                return Ok(true);
            }
            0xE890 => {
                let rn = (opcode & 0xF) as usize;
                let writeback = (opcode2 >> 5) & 1 == 1;
                let reg_list = opcode2 as u16;
                let mut address = self.registers[rn];
                for reg in 0..16 {
                    if (reg_list >> reg) & 1 == 1 {
                        self.registers[reg] = bus.read32(address as u64)?;
                        address = address.wrapping_add(4);
                    }
                }
                if writeback {
                    self.registers[rn] = address;
                }
                self.registers[15] = self.registers[15].wrapping_add(4);
                return Ok(true);
            }
            0xE8D0 => {
                match opcode2 & 0xFFF0 {
                    0xF000 => {
                        let rn = (opcode & 0xF) as usize;
                        let rm = (opcode2 & 0xF) as usize;
                        let next_pc = self.registers[15].wrapping_add(4);
                        let base = if rn == 15 {
                            next_pc
                        } else {
                            self.registers[rn]
                        };
                        let offset = if (opcode2 & 0x0010) == 0 {
                            bus.read8(base.wrapping_add(self.registers[rm]) as u64)? as u32
                        } else {
                            bus.read16(base.wrapping_add(self.registers[rm] << 1) as u64)? as u32
                        };
                        self.registers[15] = next_pc.wrapping_add(offset << 1);
                        return Ok(true);
                    }
                    _ => {}
                }
            }
            _ => {}
        }

        match opcode & 0xF800 {
            0xF000 => {
                match opcode & 0xFFF0 {
                    0xF3C0 => {
                        let rn = (opcode & 0xF) as usize;
                        let rd = ((opcode2 >> 8) & 0xF) as usize;
                        let lsb =
                            (((opcode2 >> 12) & 0x7) as u32) << 2 | (((opcode2 >> 6) & 0x3) as u32);
                        let width = (opcode2 & 0x1F) as u32 + 1;
                        let mask = if width >= 32 {
                            u32::MAX
                        } else {
                            (1u32 << width) - 1
                        };
                        self.registers[rd] = (self.registers[rn] >> lsb) & mask;
                        self.registers[15] = self.registers[15].wrapping_add(4);
                        return Ok(true);
                    }
                    0xF36F => {
                        let rd = ((opcode2 >> 8) & 0xF) as usize;
                        let lsb =
                            (((opcode2 >> 12) & 0x7) as u32) << 2 | (((opcode2 >> 6) & 0x3) as u32);
                        let msb = (opcode2 & 0x1F) as u32;
                        let width = msb.saturating_sub(lsb) + 1;
                        let mask = if width >= 32 {
                            u32::MAX
                        } else {
                            ((1u32 << width) - 1) << lsb
                        };
                        self.registers[rd] &= !mask;
                        self.registers[15] = self.registers[15].wrapping_add(4);
                        return Ok(true);
                    }
                    _ => {}
                }

                match (opcode, opcode2 >> 8, opcode2) {
                    (0xF3EF, 0x80, _) => {
                        // MRS rd, <spec_reg> (stub: always return 0 for now, e.g. for PRIMASK)
                        let rd = ((opcode2 >> 8) & 0xF) as usize;
                        self.registers[rd] = 0;
                        self.registers[15] = self.registers[15].wrapping_add(4);
                        return Ok(true);
                    }
                    (0xF380..=0xF38F, 0x88, _) => {
                        // MSR <spec_reg>, rn (stub: just ignore for now, e.g. setting PRIMASK)
                        self.registers[15] = self.registers[15].wrapping_add(4);
                        return Ok(true);
                    }
                    (0xF3BF, 0x8F, _) => {
                        // Barrier instructions (DSB, DMB, ISB) - treat as NOP
                        self.registers[15] = self.registers[15].wrapping_add(4);
                        return Ok(true);
                    }
                    _ => {}
                }

                match opcode2 & 0x8000 {
                    0 => {
                        let imm12 = (((opcode >> 10) & 1) as u32) << 11
                            | (((opcode2 >> 12) & 0x7) as u32) << 8
                            | (opcode2 & 0xFF) as u32;
                        let imm32 = Self::thumb_expand_imm12(imm12);
                        let rn = (opcode & 0xF) as usize;
                        let rd = ((opcode2 >> 8) & 0xF) as usize;

                        match opcode & 0xFBEF {
                            0xF04F => {
                                self.registers[rd] = imm32;
                                self.registers[15] = self.registers[15].wrapping_add(4);
                                return Ok(true);
                            }
                            0xF06F => {
                                self.registers[rd] = !imm32;
                                self.registers[15] = self.registers[15].wrapping_add(4);
                                return Ok(true);
                            }
                            _ => match opcode & 0xFBE0 {
                                0xF040 => {
                                    self.registers[rd] = self.registers[rn] | imm32;
                                    self.registers[15] = self.registers[15].wrapping_add(4);
                                    return Ok(true);
                                }
                                0xF020 => {
                                    self.registers[rd] = self.registers[rn] & !imm32;
                                    self.registers[15] = self.registers[15].wrapping_add(4);
                                    return Ok(true);
                                }
                                0xF000 => {
                                    let result = self.registers[rn] & imm32;
                                    self.set_nz_flags(result);
                                    if rd != 15 {
                                        self.registers[rd] = result;
                                    }
                                    self.registers[15] = self.registers[15].wrapping_add(4);
                                    return Ok(true);
                                }
                                _ => {}
                            },
                        }
                    }
                    _ => {}
                }

                match opcode2 & 0xD000 {
                    0x8000 => {
                        let cond = ((opcode >> 6) & 0xF) as u8;
                        if cond < 0xE {
                            let s = ((opcode >> 10) & 1) as u32;
                            let imm6 = (opcode & 0x3F) as u32;
                            let j1 = ((opcode2 >> 13) & 1) as u32;
                            let j2 = ((opcode2 >> 11) & 1) as u32;
                            let imm11 = (opcode2 & 0x07FF) as u32;
                            let imm21 =
                                (s << 20) | (j2 << 19) | (j1 << 18) | (imm6 << 12) | (imm11 << 1);
                            let offset = ((imm21 as i32) << 11) >> 11;
                            let next_pc = self.registers[15].wrapping_add(4);
                            let target = next_pc.wrapping_add_signed(offset);
                            let taken = self.condition_passed(cond);
                            if taken {
                                self.registers[15] = target;
                            } else {
                                self.registers[15] = next_pc;
                            }
                            return Ok(true);
                        }
                    }
                    0xD000 => {
                        let s = ((opcode >> 10) & 1) as u32;
                        let j1 = ((opcode2 >> 13) & 1) as u32;
                        let j2 = ((opcode2 >> 11) & 1) as u32;
                        let imm10 = (opcode & 0x03FF) as u32;
                        let imm11 = (opcode2 & 0x07FF) as u32;
                        let i1 = (!(j1 ^ s)) & 1;
                        let i2 = (!(j2 ^ s)) & 1;
                        let imm25 =
                            (s << 24) | (i1 << 23) | (i2 << 22) | (imm10 << 12) | (imm11 << 1);
                        let signed = ((imm25 << 7) as i32) >> 7;
                        let next_pc = self.registers[15].wrapping_add(4);
                        self.registers[14] = next_pc | 1;
                        self.registers[15] = next_pc.wrapping_add_signed(signed);
                        return Ok(true);
                    }
                    _ => {}
                }
            }
            _ => {}
        }

        Err(format!(
            "unimplemented Thumb instruction 0x{opcode:04x} at PC 0x{pc:08x}"
        ))
    }
}

impl CpuCore for CortexM3 {
    fn step(&mut self, bus: &mut dyn SystemBus) -> Result<(), String> {
        let pc = self.program_counter();
        let (opcode, width) = self.fetch_opcode(bus, pc)?;

        if self.it_pos < self.it_count {
            let cond = self.it_conds[self.it_pos as usize];
            self.it_pos += 1;
            if !self.condition_passed(cond) {
                self.registers[15] = self.registers[15].wrapping_add(width);
                return Ok(());
            }
        }

        match width {
            2 => {
                if self.step16_fast(bus, pc, opcode)? {
                    return Ok(());
                }
            }
            4 => {
                let opcode2 = self.fetch_opcode2(bus, pc)?;
                if self.try_fast_memclr_loop(bus, pc, opcode, opcode2)? {
                    return Ok(());
                }
                if self.step32_fast(bus, pc, opcode, opcode2)? {
                    return Ok(());
                }
            }
            _ => unreachable!(),
        }

        Err(format!(
            "unimplemented Thumb instruction 0x{opcode:04x} at PC 0x{pc:08x}"
        ))
    }

    fn reset(&mut self, bus: &mut dyn SystemBus, vector_table_base: u64) -> Result<(), String> {
        let initial_sp = bus.read32(vector_table_base)?;
        let reset_handler = bus.read32(vector_table_base + 4)?;

        self.registers = [0; 16];
        self.registers[13] = initial_sp;
        self.registers[15] = reset_handler & !1;
        self.xpsr = 1 << 24;
        self.it_pos = 0;
        self.it_count = 0;
        self.decode_cache.fill(DecodeCacheEntry::default());
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

        let next_sp = self.registers[13].wrapping_sub(32);
        bus.write32(next_sp as u64, self.registers[0])?;
        bus.write32((next_sp + 4) as u64, self.registers[1])?;
        bus.write32((next_sp + 8) as u64, self.registers[2])?;
        bus.write32((next_sp + 12) as u64, self.registers[3])?;
        bus.write32((next_sp + 16) as u64, self.registers[12])?;
        bus.write32((next_sp + 20) as u64, self.registers[14])?;
        bus.write32((next_sp + 24) as u64, self.registers[15] | 1)?;
        bus.write32((next_sp + 28) as u64, self.xpsr)?;

        self.registers[13] = next_sp;
        self.registers[14] = 0xFFFF_FFF9;
        self.registers[15] = handler & !1;
        self.xpsr = (self.xpsr & !0x1FF) | (u32::from(exception_number) & 0x1FF);
        self.xpsr |= 1 << 24;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::bus::SystemBus;
    use crate::cpu::CpuCore;
    use crate::machine::Machine;
    use crate::memory::FirmwareImage;
    use crate::target::{MemoryRegion, MemoryRegionKind, TargetSpec};

    use super::CortexM3;

    fn make_target() -> TargetSpec {
        TargetSpec {
            name: "test-target".to_string(),
            architecture: crate::cpu::ArchitectureId::ArmV7M,
            vector_table_base: 0x0800_0000,
            core_clock_hz: 8_000_000,
            systick_reload_divider: 1024,
            memory_map: vec![
                MemoryRegion {
                    name: "flash".to_string(),
                    range: 0x0800_0000..0x0800_0100,
                    kind: MemoryRegionKind::Flash,
                },
                MemoryRegion {
                    name: "ram".to_string(),
                    range: 0x2000_0000..0x2000_0100,
                    kind: MemoryRegionKind::Ram,
                },
            ],
            peripherals: vec![],
        }
    }

    #[derive(Default)]
    struct TestBus {
        bytes: BTreeMap<u64, u8>,
    }

    impl TestBus {
        fn load(&mut self, base: u64, data: &[u8]) {
            for (index, byte) in data.iter().enumerate() {
                self.bytes.insert(base + index as u64, *byte);
            }
        }
    }

    impl SystemBus for TestBus {
        fn read8(&mut self, addr: u64) -> Result<u8, String> {
            self.bytes
                .get(&addr)
                .copied()
                .ok_or_else(|| format!("read from unmapped address 0x{addr:08x}"))
        }

        fn write8(&mut self, addr: u64, value: u8) -> Result<(), String> {
            self.bytes.insert(addr, value);
            Ok(())
        }
    }

    #[test]
    fn cortex_m3_reset_loads_sp_and_pc_from_vector_table() {
        let bytes = [
            0x00, 0x10, 0x00, 0x20, // SP = 0x20001000
            0x09, 0x00, 0x00, 0x08, // Reset = 0x08000009
            0x00, 0xBF, // NOP
        ];
        let firmware = FirmwareImage::from_bin(0x0800_0000, &bytes);
        let mut machine = Machine::new(CortexM3::new(), make_target());
        machine
            .load_firmware(&firmware)
            .expect("firmware should load");
        machine.reset_cpu().expect("cpu reset should work");

        assert_eq!(machine.cpu().stack_pointer(), 0x2000_1000);
        assert_eq!(machine.cpu().program_counter(), 0x0800_0008);
    }

    #[test]
    fn cortex_m3_branch_instruction_loops_to_self() {
        let bytes = [
            0x00, 0x10, 0x00, 0x20, 0x09, 0x00, 0x00, 0x08, 0xFE, 0xE7, // b .
        ];
        let firmware = FirmwareImage::from_bin(0x0800_0000, &bytes);
        let mut machine = Machine::new(CortexM3::new(), make_target());
        machine
            .load_firmware(&firmware)
            .expect("firmware should load");
        machine.reset_cpu().expect("cpu reset should work");
        machine.step_cpu().expect("branch should execute");

        assert_eq!(machine.cpu().program_counter(), 0x0800_0008);
    }

    #[test]
    fn cortex_m3_executes_ldr_movs_and_str_into_ram() {
        let bytes = [
            0x00, 0x10, 0x00, 0x20, 0x09, 0x00, 0x00, 0x08, 0x02,
            0x48, // ldr r0, [pc, #8] => 0x20000000
            0x05, 0x21, // movs r1, #5
            0x01, 0x60, // str r1, [r0]
            0xFE, 0xE7, // b .
            0x00, 0xBF, // nop
            0x00, 0xBF, // nop
            0x00, 0x00, 0x00, 0x20,
        ];
        let firmware = FirmwareImage::from_bin(0x0800_0000, &bytes);
        let mut machine = Machine::new(CortexM3::new(), make_target());
        machine
            .load_firmware(&firmware)
            .expect("firmware should load");
        machine.reset_cpu().expect("cpu reset should work");

        machine.step_cpu().expect("ldr should execute");
        machine.step_cpu().expect("movs should execute");
        machine.step_cpu().expect("str should execute");

        assert_eq!(machine.read8(0x2000_0000).expect("ram read"), 0x05);
    }

    #[test]
    fn cortex_m3_executes_bx_to_thumb_target() {
        let bytes = [
            0x00, 0x10, 0x00, 0x20, 0x09, 0x00, 0x00, 0x08, 0x01,
            0x4A, // ldr r2, [pc, #4] => 0x08000015
            0x10, 0x47, // bx r2
            0x00, 0xBF, // nop
            0x00, 0xBF, // nop
            0x15, 0x00, 0x00, 0x08, 0xFE, 0xE7, // b .
        ];
        let firmware = FirmwareImage::from_bin(0x0800_0000, &bytes);
        let mut machine = Machine::new(CortexM3::new(), make_target());
        machine
            .load_firmware(&firmware)
            .expect("firmware should load");
        machine.reset_cpu().expect("cpu reset should work");

        machine.step_cpu().expect("ldr should execute");
        machine.step_cpu().expect("bx should execute");

        assert_eq!(machine.cpu().program_counter(), 0x0800_0014);
    }

    #[test]
    fn cortex_m3_executes_cmp_and_beq() {
        let bytes = [
            0x00, 0x10, 0x00, 0x20, 0x09, 0x00, 0x00, 0x08, 0x00, 0x20, // movs r0, #0
            0x01, 0x21, // movs r1, #1
            0x80, 0x42, // cmp r0, r0
            0x01, 0xD0, // beq +2
            0x01, 0x21, // movs r1, #1 (skipped)
            0xFE, 0xE7, // b .
        ];
        let firmware = FirmwareImage::from_bin(0x0800_0000, &bytes);
        let mut machine = Machine::new(CortexM3::new(), make_target());
        machine
            .load_firmware(&firmware)
            .expect("firmware should load");
        machine.reset_cpu().expect("cpu reset should work");

        machine.step_cpu().expect("movs r0 should execute");
        machine.step_cpu().expect("movs r1 should execute");
        machine.step_cpu().expect("cmp should execute");
        machine.step_cpu().expect("beq should execute");

        assert_eq!(machine.cpu().program_counter(), 0x0800_0014);
    }

    #[test]
    fn cortex_m3_executes_stmia_and_ldmia() {
        let mut cpu = CortexM3::new();
        let mut bus = TestBus::default();
        bus.load(0x0800_0000, &[0x06, 0xC0, 0x06, 0xC8]); // stmia r0!, {r1,r2}; ldmia r0!, {r1,r2}

        cpu.registers[0] = 0x2000_0000;
        cpu.registers[1] = 0x1122_3344;
        cpu.registers[2] = 0x5566_7788;
        cpu.registers[15] = 0x0800_0000;

        cpu.step(&mut bus).expect("stmia should execute");
        assert_eq!(cpu.registers[0], 0x2000_0008);
        assert_eq!(bus.read32(0x2000_0000).expect("memory read"), 0x1122_3344);
        assert_eq!(bus.read32(0x2000_0004).expect("memory read"), 0x5566_7788);

        cpu.registers[1] = 0;
        cpu.registers[2] = 0;
        cpu.registers[0] = 0x2000_0000;
        cpu.step(&mut bus).expect("ldmia should execute");
        assert_eq!(cpu.registers[0], 0x2000_0008);
        assert_eq!(cpu.registers[1], 0x1122_3344);
        assert_eq!(cpu.registers[2], 0x5566_7788);
    }

    #[test]
    fn cortex_m3_executes_push_and_pop() {
        let mut cpu = CortexM3::new();
        let mut bus = TestBus::default();
        bus.load(0x0800_0000, &[0x03, 0xB5, 0x03, 0xBC]); // push {r0,r1,lr}; pop {r0,r1}

        cpu.registers[0] = 0xAABB_CCDD;
        cpu.registers[1] = 0x1122_3344;
        cpu.registers[14] = 0x0800_1235;
        cpu.registers[13] = 0x2000_0010;
        cpu.registers[15] = 0x0800_0000;

        cpu.step(&mut bus).expect("push should execute");
        assert_eq!(cpu.registers[13], 0x2000_0004);
        assert_eq!(bus.read32(0x2000_0004).expect("stack read"), 0xAABB_CCDD);
        assert_eq!(bus.read32(0x2000_0008).expect("stack read"), 0x1122_3344);
        assert_eq!(bus.read32(0x2000_000C).expect("stack read"), 0x0800_1235);

        cpu.registers[0] = 0;
        cpu.registers[1] = 0;
        cpu.step(&mut bus).expect("pop should execute");
        assert_eq!(cpu.registers[13], 0x2000_000C);
        assert_eq!(cpu.registers[0], 0xAABB_CCDD);
        assert_eq!(cpu.registers[1], 0x1122_3344);
    }

    #[test]
    fn cortex_m3_executes_bl_and_sets_lr() {
        let mut cpu = CortexM3::new();
        let mut bus = TestBus::default();
        bus.load(0x0800_0130, &[0x02, 0xF0, 0x0A, 0xF8]); // bl 0x08000148
        cpu.registers[15] = 0x0800_0130;

        cpu.step(&mut bus).expect("bl should execute");

        assert_eq!(cpu.program_counter(), 0x0800_2148);
        assert_eq!(cpu.registers[14], 0x0800_0135);
    }

    #[test]
    fn cortex_m3_executes_thumb_alu_and_extend_ops() {
        let mut cpu = CortexM3::new();
        let mut bus = TestBus::default();
        bus.load(
            0x0800_0000,
            &[
                0x08, 0x40, 0x08, 0x43, 0x48, 0x43, 0x08, 0x42, 0xC8, 0xB2, 0x88, 0xB2,
            ],
        );

        cpu.registers[0] = 0x00FF_00F0;
        cpu.registers[1] = 0x0F0F_000F;
        cpu.registers[15] = 0x0800_0000;

        cpu.step(&mut bus).expect("ands should execute");
        assert_eq!(cpu.registers[0], 0x000F_0000);

        cpu.step(&mut bus).expect("orrs should execute");
        assert_eq!(cpu.registers[0], 0x0F0F_000F);

        cpu.registers[1] = 3;
        cpu.step(&mut bus).expect("muls should execute");
        assert_eq!(cpu.registers[0], 0x2D2D_002D);

        cpu.step(&mut bus).expect("tst should execute");
        assert_eq!(cpu.program_counter(), 0x0800_0008);

        cpu.registers[1] = 0x1234_56AB;
        cpu.step(&mut bus).expect("uxtb should execute");
        assert_eq!(cpu.registers[0], 0xAB);

        cpu.registers[1] = 0x1234_56AB;
        cpu.step(&mut bus).expect("uxth should execute");
        assert_eq!(cpu.registers[0], 0x56AB);
    }

    #[test]
    fn cortex_m3_executes_clz_t1() {
        let mut cpu = CortexM3::new();
        let mut bus = TestBus::default();
        // clz r0, r0
        bus.load(0x0800_0000, &[0xB0, 0xFA, 0x80, 0xF0]);

        cpu.registers[0] = 0x0000_1000;
        cpu.registers[15] = 0x0800_0000;

        cpu.step(&mut bus).expect("clz should execute");
        assert_eq!(cpu.registers[0], 19);
        assert_eq!(cpu.program_counter(), 0x0800_0004);
    }

    #[test]
    fn cortex_m3_executes_mls_with_correct_register_fields() {
        let mut cpu = CortexM3::new();
        let mut bus = TestBus::default();
        // mls r0, r5, r10, r3
        bus.load(0x0800_0000, &[0x05, 0xFB, 0x1A, 0x30]);

        cpu.registers[5] = 10;
        cpu.registers[10] = 10;
        cpu.registers[3] = 103;
        cpu.registers[15] = 0x0800_0000;

        cpu.step(&mut bus).expect("mls should execute");
        assert_eq!(cpu.registers[0], 3);
        assert_eq!(cpu.program_counter(), 0x0800_0004);
    }

    #[test]
    fn cortex_m3_executes_mul_w_with_ra_pc_encoding() {
        let mut cpu = CortexM3::new();
        let mut bus = TestBus::default();
        // mul.w r6, r4, r3  (encoding uses Ra=R15 sentinel)
        bus.load(0x0800_0000, &[0x04, 0xFB, 0x03, 0xF6]);

        cpu.registers[4] = 40;
        cpu.registers[3] = 40;
        cpu.registers[6] = 0;
        cpu.registers[15] = 0x0800_0000;

        cpu.step(&mut bus).expect("mul.w should execute");
        assert_eq!(cpu.registers[6], 1600);
        assert_eq!(cpu.program_counter(), 0x0800_0004);
    }

    #[test]
    fn cortex_m3_executes_umull_with_correct_register_fields() {
        let mut cpu = CortexM3::new();
        let mut bus = TestBus::default();
        // umull r0, r1, r0, r1
        bus.load(0x0800_0000, &[0xA0, 0xFB, 0x01, 0x01]);

        cpu.registers[0] = 8;
        cpu.registers[1] = 1_000_000;
        cpu.registers[15] = 0x0800_0000;

        cpu.step(&mut bus).expect("umull should execute");
        assert_eq!(cpu.registers[0], 8_000_000);
        assert_eq!(cpu.registers[1], 0);
        assert_eq!(cpu.program_counter(), 0x0800_0004);
    }

    #[test]
    fn cortex_m3_executes_ldrsb_imm_sub_t2() {
        let mut cpu = CortexM3::new();
        let mut bus = TestBus::default();
        // ldrsb.w r0, [r7, #-13]
        bus.load(0x0800_0000, &[0x17, 0xF9, 0x0D, 0x0C]);
        bus.load(0x2000_0003, &[0x80]);

        cpu.registers[7] = 0x2000_0010;
        cpu.registers[15] = 0x0800_0000;

        cpu.step(&mut bus).expect("ldrsb.w should execute");
        assert_eq!(cpu.registers[0], 0xFFFF_FF80);
        assert_eq!(cpu.program_counter(), 0x0800_0004);
    }
}
