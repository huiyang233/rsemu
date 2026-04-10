use crate::bus::traits::{AnalogSink, AnalogSource};

/// ADC peripheral — multi-channel analog-to-digital converter.
///
/// Supports up to 16 channels. Each channel can be backed by an `AnalogSource`
/// that provides sample values. If no source is registered for a channel,
/// a configurable default value is returned.
pub struct AdcPeripheral {
    sources: Vec<Option<Box<dyn AnalogSource>>>,
    defaults: Vec<u16>,
    channel_count: u8,
}

impl AdcPeripheral {
    pub fn new(channel_count: u8) -> Self {
        let count = channel_count.min(16) as usize;
        Self {
            sources: (0..count).map(|_| None).collect(),
            defaults: (0..count).map(|_| 0).collect(),
            channel_count: count as u8,
        }
    }

    /// Register an analog signal source for a channel.
    pub fn set_source(&mut self, channel: u8, source: Box<dyn AnalogSource>) {
        if (channel as usize) < self.sources.len() {
            self.sources[channel as usize] = Some(source);
        }
    }

    /// Set the default value for a channel (used when no source is registered).
    pub fn set_default(&mut self, channel: u8, value: u16) {
        if (channel as usize) < self.defaults.len() {
            self.defaults[channel as usize] = value.min(0x0FFF);
        }
    }

    /// Sample a channel. Returns a 12-bit value (0-4095).
    /// If a source is registered, calls `source.sample()`.
    /// Otherwise returns the default value.
    pub fn sample(&mut self, channel: u8) -> u16 {
        if channel as usize >= self.sources.len() {
            return 0;
        }
        if let Some(source) = &mut self.sources[channel as usize] {
            source.sample(channel).min(0x0FFF)
        } else {
            self.defaults[channel as usize]
        }
    }

    pub fn channel_count(&self) -> u8 {
        self.channel_count
    }

    pub fn reset(&mut self) {
        // Keep sources and defaults; just clear state if any
    }
}

impl std::fmt::Debug for AdcPeripheral {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdcPeripheral")
            .field("channel_count", &self.channel_count)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// DAC peripheral — dual-channel digital-to-analog converter
// ---------------------------------------------------------------------------

/// DAC peripheral — outputs analog values from digital writes.
///
/// Supports 2 channels. Each channel can be connected to an `AnalogSink`
/// that receives output values.
pub struct DacPeripheral {
    sinks: [Option<Box<dyn AnalogSink>>; 2],
    values: [u16; 2],
}

impl DacPeripheral {
    pub fn new() -> Self {
        Self {
            sinks: [None, None],
            values: [0; 2],
        }
    }

    /// Register an analog sink for a channel (0 or 1).
    pub fn set_sink(&mut self, channel: u8, sink: Box<dyn AnalogSink>) {
        if let Some(slot) = self.sinks.get_mut(channel as usize) {
            *slot = Some(sink);
        }
    }

    /// Write a value to a DAC channel (12-bit, 0-4095).
    /// If a sink is registered, forwards the value.
    pub fn write(&mut self, channel: u8, value: u16) {
        if channel as usize >= 2 {
            return;
        }
        let value = value.min(0x0FFF);
        self.values[channel as usize] = value;
        if let Some(sink) = &mut self.sinks[channel as usize] {
            sink.output(channel, value);
        }
    }

    /// Read the current value of a DAC channel.
    pub fn read(&self, channel: u8) -> u16 {
        self.values.get(channel as usize).copied().unwrap_or(0)
    }

    pub fn reset(&mut self) {
        self.values = [0; 2];
    }
}

impl std::fmt::Debug for DacPeripheral {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DacPeripheral")
            .field("values", &self.values)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Built-in analog sources
// ---------------------------------------------------------------------------

/// A fixed-value analog source (always returns the same value).
pub struct FixedAnalogSource {
    value: u16,
}

impl FixedAnalogSource {
    pub fn new(value: u16) -> Self {
        Self {
            value: value.min(0x0FFF),
        }
    }
}

impl AnalogSource for FixedAnalogSource {
    fn sample(&mut self, _channel: u8) -> u16 {
        self.value
    }
}

/// A sine-wave analog source.
pub struct SineAnalogSource {
    amplitude: u16,
    offset: u16,
    phase: f64,
    phase_step: f64,
}

impl SineAnalogSource {
    /// Create a sine wave source.
    /// `amplitude` — peak deviation (0-2047)
    /// `offset` — center value (0-4095)
    /// `frequency_hz` — oscillation frequency
    /// `sample_rate_hz` — how often `sample()` is called per second
    pub fn new(amplitude: u16, offset: u16, frequency_hz: f64, sample_rate_hz: f64) -> Self {
        let amplitude = amplitude.min(2047);
        let offset = offset.min(0x0FFF);
        let phase_step = if sample_rate_hz > 0.0 {
            2.0 * std::f64::consts::PI * frequency_hz / sample_rate_hz
        } else {
            0.0
        };
        Self {
            amplitude,
            offset,
            phase: 0.0,
            phase_step,
        }
    }
}

impl AnalogSource for SineAnalogSource {
    fn sample(&mut self, _channel: u8) -> u16 {
        let value = self.offset as f64
            + self.amplitude as f64 * (self.phase).sin();
        self.phase += self.phase_step;
        if self.phase > 2.0 * std::f64::consts::PI {
            self.phase -= 2.0 * std::f64::consts::PI;
        }
        (value.round() as u16).min(0x0FFF)
    }
}

/// A noise analog source (pseudo-random values).
pub struct NoiseAnalogSource {
    state: u32,
    mask: u16,
    offset: u16,
}

impl NoiseAnalogSource {
    pub fn new(mask: u16, offset: u16) -> Self {
        Self {
            state: 12345,
            mask: mask.min(0x0FFF),
            offset: offset.min(0x0FFF),
        }
    }

    fn next_random(&mut self) -> u16 {
        // Simple xorshift32
        self.state ^= self.state << 13;
        self.state ^= self.state >> 17;
        self.state ^= self.state << 5;
        (self.state as u16) & self.mask
    }
}

impl AnalogSource for NoiseAnalogSource {
    fn sample(&mut self, _channel: u8) -> u16 {
        (self.next_random() as u32 + self.offset as u32).min(0x0FFF) as u16
    }
}
