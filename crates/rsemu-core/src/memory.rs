#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareSegment {
    pub load_address: u64,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareImage {
    segments: Vec<FirmwareSegment>,
}

impl FirmwareImage {
    pub fn from_bin(load_address: u64, bytes: &[u8]) -> Self {
        Self {
            segments: vec![FirmwareSegment {
                load_address,
                bytes: bytes.to_vec(),
            }],
        }
    }

    pub fn from_segments(segments: Vec<FirmwareSegment>) -> Self {
        Self { segments }
    }

    pub fn segments(&self) -> &[FirmwareSegment] {
        &self.segments
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryBlock {
    base: u64,
    data: Vec<u8>,
    writable: bool,
}

impl MemoryBlock {
    pub fn new(base: u64, len: usize, writable: bool) -> Self {
        Self {
            base,
            data: vec![0; len],
            writable,
        }
    }

    pub fn base(&self) -> u64 {
        self.base
    }

    pub fn end(&self) -> u64 {
        self.base + self.data.len() as u64
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn writable(&self) -> bool {
        self.writable
    }

    pub fn contains(&self, addr: u64) -> bool {
        addr >= self.base && addr < self.end()
    }

    pub fn read8(&self, addr: u64) -> Option<u8> {
        if !self.contains(addr) {
            return None;
        }

        let offset = (addr - self.base) as usize;
        self.data.get(offset).copied()
    }

    pub fn write8(&mut self, addr: u64, value: u8) -> Result<(), String> {
        if !self.writable {
            return Err(format!("write to read-only memory 0x{addr:08x}"));
        }

        let offset = self.offset_of(addr)?;
        self.data[offset] = value;
        Ok(())
    }

    pub fn load_bytes(&mut self, addr: u64, bytes: &[u8]) -> Result<(), String> {
        let start = self.offset_of(addr)?;
        let end = start + bytes.len();
        if end > self.data.len() {
            return Err(format!(
                "segment end 0x{:08x} exceeds memory block 0x{:08x}..0x{:08x}",
                addr + bytes.len() as u64,
                self.base,
                self.end()
            ));
        }

        self.data[start..end].copy_from_slice(bytes);
        Ok(())
    }

    fn offset_of(&self, addr: u64) -> Result<usize, String> {
        if !self.contains(addr) {
            return Err(format!("address 0x{addr:08x} is outside memory block"));
        }
        Ok((addr - self.base) as usize)
    }
}
