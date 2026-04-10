# Bus-Device 架构重构规划

> 目标：将总线协议（SPI/I2C/UART/FSMC/...）和外部设备（ST7789/SSD1306/...）彻底解耦，
> 使添加新总线或新设备时不需要修改已有代码。

---

## 一、整体架构

```
┌──────────────────────────────────────────────────────────────┐
│  Machine（机器核心）                                           │
│                                                              │
│  ┌─────────────┐   ┌─────────────┐   ┌─────────────┐        │
│  │  CPU (Unicorn) │  │  Memory     │  │  NVIC       │        │
│  └──────┬──────┘   └──────┬──────┘   └──────▲──────┘        │
│         │                 │                  │                │
│         ▼                 ▼                  │ irq_request    │
│  ┌──────────────────────────────────────┐    │                │
│  │  Bus Layer（总线层）                   │    │                │
│  │                                      │    │                │
│  │  SPI Bus ──→ SpiSlave               │────┘                │
│  │  I2C Bus ──→ I2cSlave               │                     │
│  │  UART Bus ──→ UartDevice            │                     │
│  │  FSMC Bus ──→ ParallelDevice        │                     │
│  │  GPIO     ──→ GpioListener           │                     │
│  │  CAN Bus  ──→ CanNode (壳子)          │                     │
│  │  USB Bus  ──→ UsbDevice (壳子)        │                     │
│  │  ADC      ──→ AnalogSource           │                     │
│  │  DAC      ──→ AnalogSink             │                     │
│  └──────────────────────────────────────┘                     │
│         │                                                    │
│         ▼                                                    │
│  ┌──────────────────────────────────────┐                     │
│  │  Device Layer（设备层）                │                     │
│  │                                      │                     │
│  │  ST7789 (显示屏，可走 SPI 或 FSMC)     │                     │
│  │  SSD1306 (OLED，走 I2C 或 SPI)        │                     │
│  │  SPI Flash (走 SPI)                   │                     │
│  │  虚拟终端 (走 UART)                    │                     │
│  │  温度传感器 (走 I2C)                   │                     │
│  │  LED (走 GPIO)                        │                     │
│  │  ...                                  │                     │
│  └──────────────────────────────────────┘                     │
│                                                              │
│  ┌──────────────────────────────────────┐                     │
│  │  DMA Controller（内部，不属于总线模型）  │                     │
│  │  在 memory ↔ peripheral 之间搬运数据   │                     │
│  └──────────────────────────────────────┘                     │
└──────────────────────────────────────────────────────────────┘
```

### 配置绑定示例（board.toml）

```toml
[[peripheral]]
device = "ST7789"
bus = "SPI1"
cs_pin = { port = "A", pin = 4 }
dc_pin = { port = "A", pin = 5 }

# 同一个 ST7789 也可以走 FSMC，不需要改 ST7789 代码
# [[peripheral]]
# device = "ST7789"
# bus = "FSMC"
# dc_pin = { port = "D", pin = 13 }
```

---

## 二、核心 Trait 定义

### 2.1 SPI 从设备（独立 trait，不复用通用字节接口）

```rust
/// SPI 从设备 — 独立接口，体现 SPI 全双工时序
pub trait SpiSlave: Send {
    /// SPI transfer：主机发一个字节，从机同步回一个字节（MISO）
    /// 返回值是本次 transfer 的 MISO 数据
    /// SPI 总线在写 DR 时调用，返回值锁存到 DR，固件后续读 DR 时只消费锁存值
    fn transfer(&mut self, mosi: u8) -> u8;

    /// CS 片选变化
    fn chip_select(&mut self, active: bool);

    /// 设备复位
    fn reset(&mut self);
}
```

> **设计决策**：SPI 没有拆成 write_byte/read_byte 两个方法，因为 SPI 是全双工的——
> 每个 clock edge 同时发和收。`transfer(mosi) -> miso` 精确反映这个时序。

### 2.2 I2C 从设备（独立 trait，体现 START/ADDR/STOP 时序）

```rust
/// I2C 从设备 — 独立接口，体现 I2C 协议时序
pub trait I2cSlave: Send {
    /// START 条件后，总线发送 7-bit 地址 + R/W 方向位
    /// 设备返回 true 表示 ACK（地址匹配）
    /// addr7 取值范围 0x00..=0x7F
    fn address(&mut self, addr7: u8, read: bool) -> bool;

    /// 主机写模式：总线收到一个字节，传给设备
    fn write_byte(&mut self, data: u8);

    /// 主机读模式：设备提供一个字节给主机
    fn read_byte(&mut self) -> u8;

    /// STOP 条件
    fn stop(&mut self);

    /// 设备复位
    fn reset(&mut self);
}
```

> **设计决策**：I2C 不是简单的字节流，它有 START → 地址匹配 → 读/写方向 → 数据 → STOP
> 的完整时序。独立的 trait 让每个方法映射到真实的 I2C 总线事件。

### 2.3 UART 设备（独立 trait，体现双向异步）

```rust
/// UART 设备 — 双向独立，异步
pub trait UartDevice: Send {
    /// MCU TX：固件写 DR 时调用，设备接收一个字节
    fn on_tx(&mut self, byte: u8);

    /// MCU RX：固件读 DR 时调用，设备从内部缓冲区提供一个字节
    /// 返回 None 表示没有数据（总线不会置 RXNE）
    fn poll_rx(&mut self) -> Option<u8>;

    /// 复位
    fn reset(&mut self);
}
```

> **设计决策**：UART 和 SPI/I2C 完全不同——没有地址、没有 START/STOP、没有全双工同步。
> TX 和 RX 是完全独立的数据流，所以用独立的 trait。

### 2.4 并行设备（FSMC）

```rust
/// FSMC / 并行总线设备
pub trait ParallelDevice: Send {
    fn write(&mut self, addr: u32, data: u32, width: AccessWidth);
    fn read(&mut self, addr: u32, width: AccessWidth) -> u32;
    fn reset(&mut self);
    fn framebuffer(&self) -> Option<&[u8]> { None }
}

pub enum AccessWidth {
    Byte,
    HalfWord,
    Word,
}
```

### 2.5 GPIO 监听

```rust
/// 监听 GPIO 引脚变化（用于 DC/RS 控制线、LED、按键等）
pub trait GpioListener: Send {
    fn pin_changed(&mut self, port: char, pin: u8, high: bool);
}
```

### 2.6 UART 设备（已合并到 2.3）

### 2.7 模拟接口

```rust
/// ADC 的信号源（模拟设备提供模拟值）
pub trait AnalogSource: Send {
    /// ADC 采样，返回 0-4095（12-bit）
    fn sample(&mut self, channel: u8) -> u16;
}

/// DAC 的输出端
pub trait AnalogSink: Send {
    /// DAC 输出一个值
    fn output(&mut self, channel: u8, value: u16);
}
```

### 2.8 CAN / USB（壳子，后续填充）

```rust
pub struct CanFrame {
    pub id: u32,
    pub ide: bool,
    pub rtr: bool,
    pub data: Vec<u8>,
}

/// CAN 节点（壳子）
pub trait CanNode: Send {
    fn receive_frame(&mut self, frame: &CanFrame);
    fn poll_tx(&mut self) -> Option<CanFrame>;
}

/// USB 设备（壳子）
pub trait UsbDevice: Send {
    fn setup_packet(&mut self, pkt: &[u8]);
    fn data_in(&mut self, endpoint: u8, data: &[u8]);
    fn poll_data_out(&mut self, endpoint: u8) -> Option<Vec<u8>>;
}
```

### 2.9 中断回调

```rust
/// 所有能产生中断的外设/总线都通过这个回调通知 NVIC
pub type IrqCallback = Box<dyn Fn(u8) + Send + Sync>;

/// 中断请求接口
pub trait IrqSource {
    /// 设置中断回调
    fn set_irq_callback(&mut self, cb: IrqCallback);
}
```

---

## 三、数据回传设计

### 为什么 SPI 用 transfer，UART 用 poll_rx？

**SPI：transfer(mosi) -> miso（同步锁存）**

SPI 全双工，每个时钟沿同时发和收。固件写 DR 时，设备必须当场返回 MISO 数据。

```
固件写 SPI_DR = 0x03 (读命令)
→ SPI 总线: miso = device.transfer(0x03)  // 返回 0xFF
→ SPI 总线把 0xFF 锁存到 rx_latch，置 RXNE=1
→ 固件读 SPI_DR → 直接取 rx_latch (0xFF)，清 RXNE
                  → 不会再调设备！只是消费锁存值
```

> 这是真实硬件的行为：SPI DR 写入触发一次 transfer，MISO 数据锁存在移位寄存器中，
> 固件读 DR 只是取走锁存值。

**UART：on_tx + poll_rx（异步缓冲区）**

GPS 模块随时可能发数据，固件什么时候读不确定。
设备自己维护一个内部 buffer，总线在 DR 被读取时从 buffer 取数据。

```
GPS 产生数据 → 设备内部 buffer.push(0x24)
固件读 UART_DR
→ UART 总线: byte = device.poll_rx()  // Some(0x24)
→ 固件得到 0x24 ✓
```

设备自己决定 buffer 的逻辑（队列、环形缓冲、丢弃策略等），总线不关心。

### ST7789 实例：走 SPI vs 走 FSMC

**核心原则：单核心 + 多适配器，状态只存一份**

```rust
struct St7789Core {
    framebuffer: Vec<u8>,
    dc: bool,
    // ... 所有状态都在这里，只有一份
}

impl St7789Core {
    fn write_command(&mut self, cmd: u8) { /* 解析命令 */ }
    fn write_data(&mut self, data: u8) { /* 写入数据 */ }
}
```

**适配器 1：SPI 接入**
```rust
use std::sync::{Arc, Mutex};

struct St7789SpiAdapter {
    core: Arc<Mutex<St7789Core>>,  // 共享核心状态（满足 Send）
}

impl SpiSlave for St7789SpiAdapter {
    fn transfer(&mut self, mosi: u8) -> u8 {
        let mut core = self.core.lock().unwrap();
        if core.dc { core.write_data(mosi) } else { core.write_command(mosi) }
        0x00  // ST7789 不回数据
    }
    fn chip_select(&mut self, _active: bool) {}
}

impl GpioListener for St7789SpiAdapter {
    fn pin_changed(&mut self, _port: char, _pin: u8, high: bool) {
        self.core.lock().unwrap().dc = high;  // DC 引脚 → 核心
    }
}
```

**适配器 2：FSMC 接入**
```rust
struct St7789FsmcAdapter {
    core: Arc<Mutex<St7789Core>>,  // 同一个核心！
}

impl ParallelDevice for St7789FsmcAdapter {
    fn write(&mut self, addr: u32, data: u32, _width: AccessWidth) {
        let dc = (addr & 1) == 1;  // RS/DC 信号
        let mut core = self.core.lock().unwrap();
        if dc { core.write_data(data as u8) } else { core.write_command(data as u8) }
    }
}
```

**为什么必须这样？** 如果 SpiSlave 和 ParallelDevice 各自持有一份 framebuffer，
同一个 ST7789 接两路输入时画面会错乱。共享核心保证状态一致。

> 配置时只能绑定一个适配器（SPI 或 FSMC），不会同时激活两个。但适配器代码都存在，
> 切换只需要改配置。

---

## 四、实施步骤（按优先级排序）

### Phase 1：定义核心接口

**优先级：最高 — 所有后续工作的基础**

- [ ] 1.1 创建 `crates/rsemu-core/src/bus/` 目录，定义所有 trait
  - `bus_traits.rs` — SpiSlave, I2cSlave, UartDevice, ParallelDevice, GpioListener, AnalogSource, AnalogSink, CanNode, UsbDevice
  - `irq.rs` — IrqCallback, IrqSource trait
  - `access.rs` — AccessWidth 枚举
  - `mod.rs` — 模块导出
- [ ] 1.2 定义设备注册与能力查询机制（替代 DeviceFactory + downcast）
  ```rust
  bitflags::bitflags! {
      pub struct DeviceCapabilities: u32 {
          const SPI_SLAVE      = 0x01;
          const I2C_SLAVE      = 0x02;
          const UART_DEVICE    = 0x04;
          const PARALLEL       = 0x08;
          const GPIO_LISTENER  = 0x10;
          const ANALOG_SOURCE  = 0x20;
          const ANALOG_SINK    = 0x40;
          const CAN_NODE       = 0x80;
          const USB_DEVICE     = 0x100;
      }
  }

  /// 设备内部“能力入口”trait：默认不支持任何能力
  /// 具体设备只覆写自己支持的方法，避免 Any::downcast
  pub trait BusAttach: Send {
      fn as_spi_slave(&mut self) -> Option<&mut dyn SpiSlave> { None }
      fn as_i2c_slave(&mut self) -> Option<&mut dyn I2cSlave> { None }
      fn as_uart_device(&mut self) -> Option<&mut dyn UartDevice> { None }
      fn as_parallel(&mut self) -> Option<&mut dyn ParallelDevice> { None }
      fn as_gpio_listener(&mut self) -> Option<&mut dyn GpioListener> { None }
      // ...
  }

  /// 设备统一句柄 —— 通过能力接口查询设备支持什么总线
  pub struct DeviceHandle {
      /// 能力集合：设备支持哪些总线 trait，由注册时声明
      capabilities: DeviceCapabilities,
      inner: Box<dyn BusAttach>,
  }

  /// 每种能力的获取方法（mut 版本，满足 transfer/read/write 等可变接口）
  impl DeviceHandle {
      pub fn as_spi_slave(&mut self) -> Option<&mut dyn SpiSlave> {
          self.inner.as_spi_slave()
      }
      pub fn as_i2c_slave(&mut self) -> Option<&mut dyn I2cSlave> {
          self.inner.as_i2c_slave()
      }
      pub fn as_uart_device(&mut self) -> Option<&mut dyn UartDevice> {
          self.inner.as_uart_device()
      }
      pub fn as_parallel(&mut self) -> Option<&mut dyn ParallelDevice> {
          self.inner.as_parallel()
      }
      pub fn as_gpio_listener(&mut self) -> Option<&mut dyn GpioListener> {
          self.inner.as_gpio_listener()
      }
      // ...
  }

  /// 设备注册 trait —— 每种设备实现一次
  pub trait DeviceRegistry: Send + Sync {
      fn name(&self) -> &str;
      fn capabilities(&self) -> DeviceCapabilities;
      /// 创建设备实例，返回的 Handle 内部是具体类型
      fn create(&self, config: &DeviceConfig) -> DeviceHandle;
  }
  ```
  > **关键**：能力查询通过 `BusAttach` vtable 分发，不走 `Any::downcast`，也不依赖运行时类型反射。
  > 总线层只拿到需要的 `&mut dyn Trait`，设备具体类型对总线透明。
- [ ] 1.3 在 rsemu-peripherals 的 Cargo.toml 中添加对 rsemu-core 的 trait 依赖（如果需要）

### Phase 2：SPI 总线 + ST7789 重构

**优先级：最高 — 验证架构是否跑得通的关键一步**

- [ ] 2.1 创建 `SpiBus` 结构体（从 machine.rs 中提取 SPI 寄存器处理逻辑）
  - 持有 `Box<dyn SpiSlave>` 设备引用
  - 持有 `IrqCallback` 中断回调
  - 处理 SR/DR 寄存器读写
  - **DR 写入时**：调用 `device.transfer(data)`，返回值锁存到内部 rx_latch，置 RXNE=1
  - **DR 读取时**：直接消费 rx_latch 中的锁存值，清除 RXNE（不再调用设备方法）
  - SR 读取：始终报告 TXE=1, BSY=0
  ```
  SPI DR 时序（修正）：
  固件写 DR=0x03 → SpiBus.transfer(0x03) → 设备返回 0xFF
                    → rx_latch = Some(0xFF), RXNE=1
  固件读 DR        → 取出 rx_latch (0xFF)  → RXNE=0
                    → 不会再次调用设备！只是消费锁存值
  ```
- [ ] 2.2 重构 `ST7789`（从 display.rs）— 单核心 + 多适配器模式
  - 提取 `St7789Core`：命令解析、帧缓冲、像素渲染、dc 状态（所有状态只存一份）
  - 创建 `St7789SpiAdapter`：持有 `Arc<Mutex<St7789Core>>`
    - 实现 `SpiSlave` trait：transfer() 根据 dc 状态分发命令/数据
    - 实现 `GpioListener` trait：pin_changed() 更新 dc 状态
  - 预留 `St7789FsmcAdapter`（Phase 6 实现 ParallelDevice）
  - 移除直接操作 SPI 寄存器的代码
- [ ] 2.3 验证：用现有固件测试 ST7789 通过 SPI 显示是否正常

### Phase 3：GPIO 回调系统

**优先级：高 — 多个设备需要监听 GPIO**

- [ ] 3.1 创建 `GpioBus` 或在现有 GPIO 处理中增加回调分发
  - 维护 `Vec<Box<dyn GpioListener>>` 列表，每个 listener 绑定到特定 (port, pin)
  - 当 BSRR/ODR 写入导致 pin 状态变化时，通知对应 listener
- [ ] 3.2 将 LED tracker 改为 GpioListener 实现
- [ ] 3.3 验证：LED 闪烁是否正常

### Phase 4：I2C 总线 + SSD1306 重构

**优先级：高 — 第二个总线类型，验证架构通用性**

- [ ] 4.1 创建 `I2cBus` 结构体
  - 持有 `Box<dyn I2cSlave>` 设备引用
  - 持有 `IrqCallback`
  - 维护 I2C 状态机（Idle → Start → Address → Data → Stop）
  - 监听 CR1 的 START/STOP 位
  - 地址匹配后调用 `device.address()`
  - 写数据阶段调用 `device.write_byte()`
  - 读数据阶段调用 `device.read_byte()`
- [ ] 4.2 重构 `SSD1306`
  - 实现 `I2cSlave` trait
  - 核心逻辑保留：命令解析、GDDRAM、像素渲染
  - 移除直接操作 I2C 寄存器的代码
- [ ] 4.3 验证：SSD1306 显示是否正常

### Phase 5：UART 设备化

**优先级：高 — 已有完整实现，重构风险低**

- [ ] 5.1 定义 UART 设备 trait（UartDevice）的虚拟终端实现
  - `VirtualTerminal`：实现 `UartDevice`，TX 输出到 GUI/CLI，RX 从 GUI/CLI 接收
- [ ] 5.2 重构 UART 总线
  - TX：DR 写入时调用 `device.on_tx(byte)`
  - RX：DR 读取时调用 `device.poll_rx()`，有数据就返回并清除 RXNE
  - 替换现有的 serial_output / usart_rx_queues 机制
- [ ] 5.3 验证：串口输入输出是否正常

### Phase 6：FSMC 总线 + ST7789 FSMC 支持

**优先级：中 — 新增总线类型**

- [ ] 6.1 创建 `FsmcBus` 结构体
  - 拦截 FSMC 地址范围（0x6000_0000 - 0x9FFF_FFFF）的读写
  - 持有 `Box<dyn ParallelDevice>`
  - 写入时调用 `device.write(addr, data, width)`
  - 读取时调用 `device.read(addr, width)` 并返回
- [ ] 6.2 给 ST7789 添加 `ParallelDevice` 实现
  - `write()` 中用地址最低位判断 RS/DC
  - 复用已有的命令/数据处理逻辑
- [ ] 6.3 验证：ST7789 通过 FSMC 显示是否正常

### Phase 7：中断回调统一

**优先级：中 — 让所有外设通过统一机制触发中断**

- [ ] 7.1 定义 `IrqCallback` 机制
  - 每个 SPI/I2C/UART/TIM 总线实例持有一个 `IrqCallback`
  - 初始化时注入回调，回调内部调用 NVIC 的 `set_pending(irq)`
- [ ] 7.2 将现有定时器（TIM/SysTick）的中断触发改为使用 IrqCallback
- [ ] 7.3 SPI/I2C/UART 的中断触发也改为使用 IrqCallback
- [ ] 7.4 验证：中断驱动的固件行为是否正常

### Phase 8：DMA 控制器

**优先级：中 — 不属于总线模型，在 machine 层面实现**

- [ ] 8.1 DMA 控制器结构体
  - 支持多通道/多 stream（F407 有 2 个 DMA 控制器，每控制器 8 stream）
  - 配置寄存器：源地址、目标地址、传输计数、数据宽度、模式
  - 监听外设的 DMA 请求信号（SPI RXNE、UART TXE 等）
- [ ] 8.2 DMA 请求接口
  - 总线/外设在数据就绪时发出 DMA 请求
  - DMA 控制器响应请求，自动搬运数据
  - 传输完成后触发中断
- [ ] 8.3 验证：DMA 驱动的 SPI 刷屏是否正常

### Phase 9：ADC / DAC

**优先级：中低 — 模拟接口**

- [ ] 9.1 ADC 外设
  - 多通道（16 通道）
  - 支持 `AnalogSource` 设备提供采样值
  - 支持单次/连续/扫描/间断转换模式
  - 转换完成中断
- [ ] 9.2 DAC 外设
  - 双通道
  - 支持 `AnalogSink` 设备接收输出
  - 支持定时器触发
- [ ] 9.3 内置信号源：固定值、正弦波、噪声等
- [ ] 9.4 验证：ADC 采样 + DAC 输出

### Phase 10：CAN / USB（壳子）

**优先级：低 — 先占位，后续填充**

- [ ] 10.1 CAN 总线壳子
  - 寄存器定义（从 SVD 获取）
  - CanNode trait 的空实现
  - 消息发送/接收的框架代码
  - 标记 TODO 的位置
- [ ] 10.2 USB OTG 壳子
  - 寄存器定义
  - UsbDevice trait 的空实现
  - 端点配置框架
  - 标记 TODO 的位置

---

## 五、注意事项

### 5.1 不要改 GUI 层

Phase 1-8 的重构不应影响 GUI 的 Tauri 事件机制。
GUI 监听的事件（led-changed, uart-output, framebuffer 更新等）保持不变，
只是底层从"直接操作寄存器"变成了"通过总线调用设备"。

### 5.2 配置文件兼容

现有的 board.toml 配置格式需要扩展，但应保持向后兼容。
新增的 device/bus 绑定是可选的，没有配置的设备走原来的行为。

### 5.3 每个 Phase 都要验证

每完成一个 Phase，用现有固件跑一遍回归测试，确保没有破坏已有功能。
特别是 ST7789 显示和 UART 输出这两个核心场景。

### 5.4 参考 stm32-emulator 项目

`/Users/yanghui/Documents/trae_projects/rsemu/stm32-emulator` 中的：
- `ExtDevice<A, T>` 泛型设计 → 值得参考
- GPIO 回调系统 → 值得参考
- DMA 实现 → 值得参考（但要修正 stride 错误）
- NVIC 中断 → 不要参考（作者自己都说写得很差）
- `Rc<RefCell<>>` 模式 → 不要参考（本项目 bus trait 含 `Send`，统一用 `Arc<Mutex<_>>` 或 `parking_lot::Mutex`）

### 5.5 STM32F103 vs STM32F407

总线抽象层应该芯片无关。
差异（F407 多了 FSMC、更多 DMA stream 等）通过配置和 trait 组合处理，
不需要在 trait 层面区分芯片型号。
