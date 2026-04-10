/// Access width for parallel bus operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessWidth {
    Byte,
    HalfWord,
    Word,
}

/// CAN frame (stub, for future CAN bus implementation).
#[derive(Debug, Clone)]
pub struct CanFrame {
    pub id: u32,
    pub ide: bool,
    pub rtr: bool,
    pub data: Vec<u8>,
}
