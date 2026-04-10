use std::collections::HashMap;

/// DMA transfer direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaDirection {
    PeripheralToMemory,
    MemoryToPeripheral,
    MemoryToMemory,
}

/// DMA data width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaDataWidth {
    Byte,
    HalfWord,
    Word,
}

/// Configuration for a single DMA stream/channel.
#[derive(Debug, Clone)]
pub struct DmaStreamConfig {
    pub channel: u8,
    pub direction: DmaDirection,
    pub peripheral_addr: u64,
    pub memory_addr: u64,
    pub ndt: u16,              // Number of data items to transfer
    pub peripheral_width: DmaDataWidth,
    pub memory_width: DmaDataWidth,
    pub peripheral_increment: bool,
    pub memory_increment: bool,
    pub circular: bool,
    pub enabled: bool,
    pub transfer_complete_irq: Option<u8>,
}

impl Default for DmaStreamConfig {
    fn default() -> Self {
        Self {
            channel: 0,
            direction: DmaDirection::PeripheralToMemory,
            peripheral_addr: 0,
            memory_addr: 0,
            ndt: 0,
            peripheral_width: DmaDataWidth::Byte,
            memory_width: DmaDataWidth::Byte,
            peripheral_increment: false,
            memory_increment: true,
            circular: false,
            enabled: false,
            transfer_complete_irq: None,
        }
    }
}

/// State for a single DMA stream.
#[derive(Debug)]
struct DmaStreamState {
    config: DmaStreamConfig,
    remaining: u16,
    memory_addr_current: u64,
    peripheral_addr_current: u64,
}

/// DMA controller — manages multiple streams and performs data transfers.
///
/// The DMA controller sits at the machine level. When a peripheral signals
/// a DMA request (e.g., SPI RXNE), the app layer calls `dma_on_peripheral_request()`.
/// The DMA controller then transfers data between memory and the peripheral register.
pub struct DmaController {
    streams: HashMap<u8, DmaStreamState>,
}

impl DmaController {
    pub fn new() -> Self {
        Self {
            streams: HashMap::new(),
        }
    }

    /// Configure a DMA stream.
    pub fn configure_stream(&mut self, stream_id: u8, config: DmaStreamConfig) {
        let remaining = config.ndt;
        let mem_addr = config.memory_addr;
        let peri_addr = config.peripheral_addr;
        self.streams.insert(
            stream_id,
            DmaStreamState {
                config,
                remaining,
                memory_addr_current: mem_addr,
                peripheral_addr_current: peri_addr,
            },
        );
    }

    /// Enable/disable a DMA stream.
    pub fn set_stream_enabled(&mut self, stream_id: u8, enabled: bool) {
        if let Some(stream) = self.streams.get_mut(&stream_id) {
            stream.config.enabled = enabled;
            if enabled {
                stream.remaining = stream.config.ndt;
                stream.memory_addr_current = stream.config.memory_addr;
                stream.peripheral_addr_current = stream.config.peripheral_addr;
            }
        }
    }

    /// Called when a peripheral signals a DMA request.
    /// Performs a single data transfer for the matching stream.
    /// Returns `(stream_id, transfer_complete_irq)` if a transfer happened.
    ///
    /// Note: actual memory/ register read/write is done by the caller
    /// (the app layer with access to Machine). This method just calculates
    /// what needs to happen and returns the addresses.
    pub fn on_peripheral_request(&mut self, stream_id: u8) -> Option<DmaTransfer> {
        let stream = self.streams.get_mut(&stream_id)?;
        if !stream.config.enabled || stream.remaining == 0 {
            return None;
        }

        let direction = stream.config.direction;
        let mem_addr = stream.memory_addr_current;
        let peri_addr = stream.peripheral_addr_current;
        let mem_width = stream.config.memory_width;
        let peri_width = stream.config.peripheral_width;
        let byte_count = match peri_width {
            DmaDataWidth::Byte => 1,
            DmaDataWidth::HalfWord => 2,
            DmaDataWidth::Word => 4,
        };

        stream.remaining = stream.remaining.saturating_sub(1);

        if stream.config.memory_increment {
            let inc = match mem_width {
                DmaDataWidth::Byte => 1,
                DmaDataWidth::HalfWord => 2,
                DmaDataWidth::Word => 4,
            };
            stream.memory_addr_current = stream.memory_addr_current.wrapping_add(inc);
        }
        if stream.config.peripheral_increment {
            stream.peripheral_addr_current = stream.peripheral_addr_current.wrapping_add(byte_count);
        }

        let transfer_complete = stream.remaining == 0;
        let irq = if transfer_complete {
            stream.config.transfer_complete_irq
        } else {
            None
        };

        // Handle circular mode: reload NDT when transfer completes
        if transfer_complete && stream.config.circular {
            stream.remaining = stream.config.ndt;
            stream.memory_addr_current = stream.config.memory_addr;
            stream.peripheral_addr_current = stream.config.peripheral_addr;
        }

        Some(DmaTransfer {
            stream_id,
            direction,
            memory_addr: mem_addr,
            peripheral_addr: peri_addr,
            byte_count: byte_count as usize,
            transfer_complete,
            irq,
        })
    }

    /// Check if a stream is active (enabled with remaining transfers).
    pub fn is_stream_active(&self, stream_id: u8) -> bool {
        self.streams
            .get(&stream_id)
            .is_some_and(|s| s.config.enabled && s.remaining > 0)
    }

    /// Get the number of remaining data items for a stream.
    pub fn stream_remaining(&self, stream_id: u8) -> u16 {
        self.streams
            .get(&stream_id)
            .map(|s| s.remaining)
            .unwrap_or(0)
    }

    pub fn reset(&mut self) {
        self.streams.clear();
    }
}

impl std::fmt::Debug for DmaController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DmaController")
            .field("stream_count", &self.streams.len())
            .finish()
    }
}

/// Description of a single DMA transfer to be executed by the app layer.
#[derive(Debug, Clone)]
pub struct DmaTransfer {
    pub stream_id: u8,
    pub direction: DmaDirection,
    pub memory_addr: u64,
    pub peripheral_addr: u64,
    pub byte_count: usize,
    pub transfer_complete: bool,
    pub irq: Option<u8>,
}
