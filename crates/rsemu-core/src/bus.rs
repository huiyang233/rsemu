pub trait SystemBus {
    fn read8(&mut self, addr: u64) -> Result<u8, String>;
    fn write8(&mut self, addr: u64, value: u8) -> Result<(), String>;

    fn read_block(&mut self, addr: u64, buf: &mut [u8]) -> Result<(), String> {
        for (i, byte) in buf.iter_mut().enumerate() {
            *byte = self.read8(addr + i as u64)?;
        }
        Ok(())
    }

    fn write_block(&mut self, addr: u64, buf: &[u8]) -> Result<(), String> {
        for (i, &byte) in buf.iter().enumerate() {
            self.write8(addr + i as u64, byte)?;
        }
        Ok(())
    }

    fn read16(&mut self, addr: u64) -> Result<u16, String> {
        let lo = self.read8(addr)? as u16;
        let hi = self.read8(addr + 1)? as u16;
        Ok(lo | (hi << 8))
    }

    fn read32(&mut self, addr: u64) -> Result<u32, String> {
        let b0 = self.read8(addr)? as u32;
        let b1 = self.read8(addr + 1)? as u32;
        let b2 = self.read8(addr + 2)? as u32;
        let b3 = self.read8(addr + 3)? as u32;
        Ok(b0 | (b1 << 8) | (b2 << 16) | (b3 << 24))
    }

    fn write16(&mut self, addr: u64, value: u16) -> Result<(), String> {
        self.write8(addr, (value & 0xFF) as u8)?;
        self.write8(addr + 1, (value >> 8) as u8)
    }

    fn write32(&mut self, addr: u64, value: u32) -> Result<(), String> {
        self.write8(addr, (value & 0xFF) as u8)?;
        self.write8(addr + 1, ((value >> 8) & 0xFF) as u8)?;
        self.write8(addr + 2, ((value >> 16) & 0xFF) as u8)?;
        self.write8(addr + 3, ((value >> 24) & 0xFF) as u8)
    }
}
