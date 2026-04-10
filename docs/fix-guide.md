# rsemu 修复指南

本文档列出审查发现的所有问题，按优先级排序，每条给出问题描述、定位和推荐修法。

---

## P0 — 崩溃 / 死锁（立即修）

### F-01 持有 Mutex 期间调用 `handle.join()` → 死锁

**文件：** `apps/rsemu-gui/src-tauri/src/commands/simulation.rs:16-36`

**问题：** `start_simulation` 在持有 `Mutex<SimState>` 锁的同时调用 `handle.join()`。若模拟器线程 shutdown 缓慢，所有并发的 `stop_simulation` / `inject_gpio` 调用都会死锁。

**修法：**
```rust
// 先在锁内取出值，释放锁，再 join
let (tx_opt, handle_opt) = {
    let mut g = state.lock().map_err(|e| e.to_string())?;
    (g.control_tx.take(), g.thread.take())
};
if let Some(tx) = tx_opt {
    let _ = tx.send(ControlMsg::Stop);
}
if let Some(handle) = handle_opt {
    let _ = handle.join();
}
```

---

### F-02 `gpio_idr_addr` panic → 杀死模拟器线程

**文件：** `crates/rsemu-core/src/execution.rs:100,105`

**问题：** 找不到 GPIOA 或 IDR 寄存器时直接 `expect` panic。线程崩溃后前端无限等待 GPIO 注入响应，无任何错误提示。

**修法：** 改为返回 `Result<u64, String>`，调用方处理错误：
```rust
fn gpio_idr_addr(peripherals: &[PeripheralSpec]) -> Result<u64, String> {
    let gpioa = peripherals.iter().find(|p| p.name == "GPIOA")
        .ok_or_else(|| "GPIOA not found in target spec".to_string())?;
    let idr = gpioa.registers.iter().find(|r| r.name == "IDR")
        .ok_or_else(|| "IDR register not found in GPIOA".to_string())?;
    Ok(idr.address_offset as u64 + gpioa.base_address)
}
```

---

### F-03 `CortexM3::new()` / `CortexM4::new()` Unicorn 初始化失败 → silent panic

**文件：**
- `crates/rsemu-core/src/cpu/armv7m.rs:73-74`
- `crates/rsemu-core/src/cpu/armv7em.rs:76-77`

**问题：** Unicorn 引擎或 hook 安装失败时 `expect` panic，前端无任何错误事件，用户看到系统崩溃对话框。

**修法：** 改签名为 `fn new() -> Result<Self, String>`，通过 `run_machine` 的 `Result` 链传递，最终在 `emulator.rs` 里 emit 错误事件：
```rust
pub fn new() -> Result<Self, String> {
    let mut uc = Unicorn::new_with_data(...)
        .map_err(|e| format!("Unicorn init failed: {:?}", e))?;
    install_hooks(&mut uc)
        .map_err(|e| format!("Unicorn hook install failed: {e}"))?;
    Ok(Self { uc })
}
```

---

### F-04 `lib.rs` 启动时 panic → 崩溃对话框

**文件：** `apps/rsemu-gui/src-tauri/src/lib.rs:17-22`

**问题：** Tauri event loop 启动前 `expect` panic，用户看不到有意义的错误。

**修法：** 改为在 `setup` 钩子里返回 `Result`，Tauri 会捕获并显示错误：
```rust
.setup(|app| {
    let root = resolve_project_root()
        .map_err(|e| format!("cannot resolve project root: {e}"))?;
    let registry = TargetRegistry::from_dirs(&root.join("configs"), &root.join("svds"))
        .map_err(|e| format!("failed to create target registry: {e}"))?;
    app.manage(Mutex::new(registry));
    Ok(())
})
```

---

## P1 — 严重性能问题

### F-05 MMIO 使用 `HashMap<u64, u8>` 按字节存储

**文件：** `crates/rsemu-core/src/machine.rs` — `mmio` 字段，`read_mmio_u32` / `write_mmio_u32`

**问题：** 每次 32 位读需要 4 次独立 hash 查找，每次 32 位写需要 4 次 insert。MMIO 操作处于仿真热路径，每秒调用数百万次，hash 访问极度缓存不友好。

**修法（分步）：**

1. 将 MMIO 空间按已知 peripheral region 分段，每段用 `Vec<u32>`（按 word 对齐）替代 HashMap。
2. 若寄存器确实稀疏，用 `BTreeMap<u32, u32>`（按字地址）替代 `HashMap<u64, u8>`，单次查找读写 4 字节。
3. 最终目标：`read32(addr)` → 一次数组索引，无堆操作。

简化版过渡方案：
```rust
// 将四次查找合并为一次，至少减少 hash 次数
fn read_mmio_u32(mmio: &HashMap<u64, [u8; 4]>, addr: u64) -> u32 {
    mmio.get(&(addr & !3))
        .map(|b| u32::from_le_bytes(*b))
        .unwrap_or(0)
}
```

---

### F-06 `decode_mmio_write` 每次写 clone 两个 String

**文件：** `crates/rsemu-core/src/machine.rs:1836-1837`

**问题：** `MmioWriteEvent` 持有 `peripheral: String` 和 `register: String`，每次 MMIO 写都发生堆分配。高速 SPI 每秒触发数百万次。

**修法：** 将 `RegisterMeta` 和 `MmioWriteEvent` 里的 `String` 改为 `Arc<str>`，clone 只增加引用计数：
```rust
pub struct RegisterMeta {
    pub peripheral: Arc<str>,
    pub register: Arc<str>,
    // ...
}
pub struct MmioWriteEvent {
    pub peripheral: Arc<str>,
    pub register: Arc<str>,
    // ...
}
```
初始化时 `Arc::from("USART1")` 一次，之后 clone 免费。

---

### F-07 `RegisterMeta.cloned()` 在每次 MMIO 写触发

**文件：** `crates/rsemu-core/src/machine.rs:1428, 1493`

**问题：**
```rust
if let Some(meta) = register_meta.get(&addr).cloned() {
```
`.cloned()` 克隆含两个 String 的 `RegisterMeta`，发生在每次 MMIO 写。

**修法：** 改为借用，只在真正需要时使用字段：
```rust
if let Some(meta) = register_meta.get(&addr) {
    // 用 meta.peripheral.as_str() 等，不 clone 整个结构体
}
```
若后续逻辑需要独占所有权，先判断 variant 再选择性 clone。

---

### F-08 MMIO read hook 每次分配 `Vec<u8>`

**文件：** `crates/rsemu-core/src/cpu/armv7m.rs:480` — `read_bus_bytes`

**问题：** 每次 MMIO 读（4 字节）分配一个 4 元素 Vec，处于每条内存读指令的路径上。

**修法：** 改为栈上固定大小 buffer：
```rust
fn read_bus_bytes(bus: &mut dyn SystemBus, addr: u64, size: usize) -> Result<[u8; 8], String> {
    let mut buf = [0u8; 8];
    // 填充 buf[..size]
    Ok(buf)
}
```
调用方取 `&buf[..size]` 传给 `uc.mem_write`。

---

### F-09 帧缓冲三次拷贝（Rust 侧）

**文件：**
- `crates/rsemu-peripherals/src/display.rs:139` — `preview_argb.clone()`
- `crates/rsemu-peripherals/src/traits.rs:58` — `FrameUpdate { pixels: Vec<u32> }`

**问题：** 每帧经历：① `preview_argb.clone()` 到 `latest_frame_argb`；② move 到 `FrameUpdate.pixels`；③ base64 编码。三次 300KB 数据操作。

**修法：**
1. 使用双缓冲：`preview_buf` 和 `ready_buf`，完成一帧后 `std::mem::swap`，避免 clone。
2. `FrameUpdate` 改为 `pixels: Arc<Vec<u32>>` 或 `pixels: Box<[u32]>`，向前端发送时仅 clone Arc。
3. 长期：考虑从 Rust 直接发送二进制 payload（Tauri 支持 `ArrayBuffer`），省去 base64 编码开销。

---

### F-10 前端帧缓冲三次拷贝（JS 侧）

**文件：**
- `apps/rsemu-gui/src/components/peripherals/st7789/Widget.tsx:21-35`
- `apps/rsemu-gui/src/components/peripherals/ssd1306/Widget.tsx:25-40`

**问题：** 每帧 `atob(data)` 产生 300KB 字符串，再 `new Uint8Array`，再 `new Uint8ClampedArray`，加上通道 swap 循环，共 3 次全 framebuffer 拷贝，60fps 时极耗 CPU。

**修法：**
```typescript
// 用 TextDecoder + ArrayBuffer 替代 atob 字节循环
const binary = Uint8Array.from(atob(data), c => c.charCodeAt(0));
// 或 Tauri 改用二进制传输后直接 new Uint8Array(arrayBuffer)

// Rust 侧改为直接输出 RGBA（而非 ARGB），省去通道 swap 循环
// 前端直接: new ImageData(rgba, width, height)
```

---

### F-11 UART batch 自我破坏 → 每字节一次 IPC

**文件：** `apps/rsemu-gui/src-tauri/src/emulator.rs:540-544`

**问题：**
```rust
if uart_batch.len() >= uart_batch_size || !uart_batch.is_empty() {
```
条件 `!uart_batch.is_empty()` 使每次 tick 立即 drain，`uart_batch_size = 64` 完全失效，每字节仍是独立 Tauri 事件。

**修法：** 只在满足 batch 大小或超时时 flush：
```rust
// 方案 A：仅按 batch size flush（tick 里不 flush）
if uart_batch.len() >= uart_batch_size {
    flush_uart_batch(&app, &mut uart_batch);
}
// 方案 B：加定时 flush（每 N 个 step tick 强制 flush 一次）
```
同时改 Tauri 事件为批量：
```rust
// 一次 emit 整个批次
app.emit("uart-output-batch", UartBatchPayload { bytes: uart_batch.drain(..).collect() }).ok();
```

---

### F-12 前端 `appendUartByte` 每字节触发一次全局 re-render

**文件：** `apps/rsemu-gui/src/store/appStore.ts:137-143`

**问题：** 每字节调用一次 Zustand `set()`，字符串无上限增长，长时间运行后字符串可能达 MB 级，每次 GC 压力极大。

**修法：**
1. 配合 F-11，改为接收批量事件，一次 `set()` 追加多字节。
2. 加环形缓冲上限（如 64KB），超出时丢弃头部：
```typescript
const MAX_UART_LEN = 65536;
appendUartBytes: (peripheral, bytes) => set(s => {
    const prev = s.uartOutput[peripheral] ?? "";
    const next = (prev + bytes).slice(-MAX_UART_LEN);
    return { uartOutput: { ...s.uartOutput, [peripheral]: next } };
}),
```

---

## P2 — 设计 / 可扩展性问题

### F-13 `PeripheralConfig` 是封闭枚举，新增外设需改三处

**文件：** `crates/rsemu-peripherals/src/lib.rs:14-60`

**问题：** 新增外设类型必须同时修改：
1. `PeripheralConfig` enum（`rsemu-peripherals`）
2. `run_machine` 里的 match arm（`emulator.rs`）
3. 前端 React 组件注册表

**可考虑的改进方向：**
- 将 `PeripheralConfig` 拆分为 `PeripheralKind`（类型标识）+ `PeripheralParams`（通用 JSON Value），外设自行反序列化参数。
- 前端已有 registry 模式，后端也应对应引入工厂函数 map：`HashMap<&str, fn(params) -> Box<dyn Peripheral>>`。
- 短期：至少在 `PeripheralConfig` 加 `Custom { type_name: String, params: serde_json::Value }` 分支，允许外部扩展。

---

### F-14 大量硬编码寄存器名 / 外设名字符串

**文件：** `crates/rsemu-core/src/machine.rs` — `classify_meta_flags`（约 1691 行），`timer_irq_number`（约 1394 行），`apply_rcc_ready_flags`（约 1787 行）

**问题：** `peripheral.starts_with("USART")`、`register.eq_ignore_ascii_case("SR")` 等字符串匹配在 STM32G0/L4 等系列上静默失效（它们使用 `ISR`/`ICR`、`RDR`/`TDR`）。

**推荐方向：**
- 在 SVD 解析阶段打 tag（`RegisterTag::UsartDr`、`RegisterTag::SpiDr` 等），后续代码按 tag 匹配，不依赖名字字符串。
- 或在 `TargetSpec`/`board.toml` 里显式声明 UART/SPI/Timer 的关键寄存器地址，加载时解析，运行时直接走地址比较。

---

### F-15 CPU 内存区间硬编码，不从 `TargetSpec` 读取

**文件：**
- `crates/rsemu-core/src/cpu/armv7m.rs:101-103` — `is_valid_stack_addr`（写死 256KB SRAM）
- `crates/rsemu-core/src/cpu/armv7m.rs:350-352` — `reset` 里的 region 列表

**问题：** F103 实际 SRAM 20KB，F407 实际 192KB，但两者都用 256KB 范围。栈地址校验错误会导致 exception entry/return 被静默跳过，引发难以排查的仿真错误。

**修法：** 将 `reset` 和 `is_valid_stack_addr` 的内存区间改为从 `TargetSpec::memory_map` 传入，或在 `Machine::new` 时注入 `MemoryLayout`：
```rust
fn is_valid_stack_addr(addr: u32, sram_regions: &[(u32, u32)]) -> bool {
    sram_regions.iter().any(|(start, end)| (*start..*end).contains(&addr))
}
```

---

### F-16 IRQ 回调是 no-op，中断驱动外设永远死等

**文件：** `crates/rsemu-core/src/machine.rs:539-550`

**问题：** `make_irq_callback` 返回空函数，`SpiBus`/`I2cBus` 存储并调用该回调，但实际上永远不触发外设 IRQ。依赖中断的 SPI/I2C 固件驱动（RXNE/TXE 中断模式）会死循环。

**修法：** 在 `Machine::new` 里把真实的 IRQ 注入函数包装成回调，传给各 bus：
```rust
// 在 Machine 里暴露 trigger_irq(irq_num: u8)
let irq_fn: IrqCallback = Box::new(move |irq| {
    // 投递到 pending_irqs 队列，下次 step 时处理
    irq_tx.send(irq).ok();
});
```

---

### F-17 `write_special_mmio` 8位/32位逻辑近乎完全重复

**文件：** `crates/rsemu-core/src/machine.rs:1419-1607`

**问题：** 8 位和 32 位版本分别处理 RCC、PWR、SysTick、USART、SPI 等，逻辑几乎相同，维护负担高，每次加外设必须改两处。

**修法：** 统一为按 word 处理，8 位写先 read-modify-write 到 word，再走同一套逻辑：
```rust
fn write_special_mmio(&mut self, addr: u64, val: u8, ...) {
    let word_addr = addr & !3;
    let shift = (addr & 3) * 8;
    let mut word = read_mmio_u32(&self.mmio, word_addr);
    word = (word & !(0xFF << shift)) | ((val as u32) << shift);
    self.write_special_mmio_u32(word_addr, word, ...);
}
```

---

### F-18 `CpuType` match 需修改 `emulator.rs` 才能新增 CPU

**文件：** `apps/rsemu-gui/src-tauri/src/emulator.rs:176-179`

**问题：** 新增 Cortex-M0/M7 需手动加 match arm。

**修法：** 在 `CpuCore` trait 上加工厂方法，或在 `TargetSpec` 里存 CPU 构造器：
```rust
// TargetSpec 里
pub fn make_cpu(&self) -> Box<dyn CpuCore> {
    match self.cpu_type {
        CpuType::CortexM4 => Box::new(CortexM4::new().unwrap()),
        CpuType::CortexM3 => Box::new(CortexM3::new().unwrap()),
    }
}
// emulator.rs 里不再需要 match
let cpu = target.make_cpu();
run_machine(cpu, ...);
```

---

## P3 — 次要问题

### F-19 `to_ascii_uppercase` 在热路径分配 String

**文件：** `crates/rsemu-core/src/bus/bus_context.rs:94-95`

**问题：** `dispatch_mmio` 每次调用都对 `event.peripheral` 和 `event.register` 做 `to_ascii_uppercase()`，产生堆分配。

**修法：** 在初始化时将所有 peripheral/register 名统一存为大写，运行时直接比较，无需 runtime 转换。

---

### F-20 `gpio_pin_changed` 用 `char.to_string()` 做比较

**文件：** `crates/rsemu-peripherals/src/display.rs:269,272,285,288`

**问题：**
```rust
let port_upper = port.to_ascii_uppercase();
if port_upper.to_string() == self.cs.port.to_ascii_uppercase() {
```
`char` 转 `String` 后比较，两次堆分配。

**修法：** 直接比较 `char`：
```rust
if port.to_ascii_uppercase() == self.cs.port.chars().next().unwrap().to_ascii_uppercase() {
```
或将 `cs.port` 存为 `char` 而非 `String`。

---

### F-21 `St7789Core.params` 用 `Vec<u8>` 存 4 字节

**文件：** `crates/rsemu-peripherals/src/display.rs:21`

**问题：** `params: Vec<u8>` 每次命令清空再 push，最多 4 字节，纯属无谓堆分配。

**修法：**
```rust
params: arrayvec::ArrayVec<u8, 4>,
// 或
params_buf: [u8; 4],
params_len: u8,
```

---

### F-22 Fat pointer transmute 缺少安全注释

**文件：** `crates/rsemu-core/src/cpu/armv7m.rs:441-460`（以及 `armv7em.rs` 同位置）

**问题：** 把指向栈上 `MachineBus<'a>` 的 fat pointer 拆成两个 `usize` 存进 `'static UcData`，生命周期被 unsafe transmute 擦除，正确性完全依赖 Unicorn 不会在 `emu_start` 返回后调用 hook。目前代码里没有任何注释说明这个假设。

**修法（最低要求）：** 在 `set_uc_bus` / `get_uc_bus` 以及各 hook 注册点加 `// SAFETY:` 注释，明确不变量；长期考虑改用 `Pin` + 外部生命周期管理方案。

---

### F-23 `get_boards` 里无必要的 `unwrap()`

**文件：** `apps/rsemu-gui/src-tauri/src/commands/board.rs:30`

**问题：** `registry.get_config(id).unwrap()` — 虽然逻辑上不会 None，但 panic 路径存在。

**修法：**
```rust
let config = registry.get_config(id)
    .ok_or_else(|| format!("config not found for id: {id}"))?;
```

---

## 快速参考：优先级汇总

| ID | 文件 | 问题 | 优先级 |
|----|------|------|--------|
| F-01 | simulation.rs:16 | Mutex + join 死锁 | P0 |
| F-02 | execution.rs:100 | gpio_idr_addr panic | P0 |
| F-03 | armv7m.rs:73 | Unicorn init panic | P0 |
| F-04 | lib.rs:17 | 启动 panic | P0 |
| F-05 | machine.rs | HashMap<u64,u8> MMIO | P1 |
| F-06 | machine.rs:1836 | decode_mmio_write String clone | P1 |
| F-07 | machine.rs:1428 | RegisterMeta.cloned() | P1 |
| F-08 | armv7m.rs:480 | read_bus_bytes Vec 分配 | P1 |
| F-09 | display.rs:139 | 帧缓冲 Rust 侧 3 次拷贝 | P1 |
| F-10 | Widget.tsx:21 | 帧缓冲 JS 侧 3 次拷贝 | P1 |
| F-11 | emulator.rs:540 | uart_batch 失效 | P1 |
| F-12 | appStore.ts:137 | UART 每字节 re-render | P1 |
| F-13 | lib.rs:14 | PeripheralConfig 封闭枚举 | P2 |
| F-14 | machine.rs:1691 | 硬编码寄存器名 | P2 |
| F-15 | armv7m.rs:101 | 硬编码内存区间 | P2 |
| F-16 | machine.rs:539 | IRQ 回调 no-op | P2 |
| F-17 | machine.rs:1419 | write_special_mmio 重复 | P2 |
| F-18 | emulator.rs:176 | CpuType 封闭 match | P2 |
| F-19 | bus_context.rs:94 | 热路径 uppercase 分配 | P3 |
| F-20 | display.rs:269 | gpio char→String 分配 | P3 |
| F-21 | display.rs:21 | params Vec<u8> for 4 bytes | P3 |
| F-22 | armv7m.rs:441 | unsafe transmute 无注释 | P3 |
| F-23 | board.rs:30 | unwrap() in get_boards | P3 |

---

## 修复进度

| ID | 问题 | 状态 | 备注 |
|----|------|------|------|
| F-01 | Mutex + join 死锁 | ✅ 已修复 | 先取出值释放锁再 join |
| F-02 | gpio_idr_addr panic | ✅ 已修复 | 改为 Result<u64, String>，调用方处理错误 |
| F-03 | Unicorn init panic | ✅ 已修复 | CortexM3/M4::new() 返回 Result，emulator.rs emit 错误事件 |
| F-04 | 启动 panic | ✅ 已修复 | 移到 .setup() 钩子，返回 Result，Tauri 捕获并展示 |
| F-05 | MMIO HashMap<u64,u8> 性能 | ✅ 已修复 | HashMap<u64,u32> word-aligned，单次查找替代 4 次 |
| F-06 | decode_mmio_write String clone | ✅ 已修复 | RegisterMeta/MmioWriteEvent 改为 Arc<str> |
| F-07 | RegisterMeta.cloned() | ✅ 已修复 | 改为借用 register_meta.get(&addr)，不再 .cloned() |
| F-08 | read_bus_bytes Vec 分配 | ✅ 已修复 | 改为 [u8; 8] 栈上 buffer |
| F-09 | 帧缓冲 Rust 侧 3 次拷贝 | ✅ 已修复 | std::mem::replace 替代 clone()，像素格式改 RGBA |
| F-10 | 帧缓冲 JS 侧 3 次拷贝 | ✅ 已修复 | Rust 输出 RGBA，JS 直接 new ImageData 省去 swap 循环 |
| F-11 | uart_batch 失效 | ✅ 已修复 | 去掉 !is_empty() 条件，仅按 batch size flush，Stop 时 flush |
| F-12 | UART 每字节 re-render | ✅ 已修复 | 批量接收 bytes[]，64KB 环形缓冲，xterm 用 Uint8Array |
| F-13 | PeripheralConfig 封闭枚举 | ⬜ 待修复 | |
| F-14 | 硬编码寄存器名字符串 | ⬜ 待修复 | |
| F-15 | 硬编码内存区间 | ⬜ 待修复 | |
| F-16 | IRQ 回调 no-op | ⬜ 待修复 | |
| F-17 | write_special_mmio 8/32位重复 | ⬜ 待修复 | |
| F-18 | CpuType 封闭 match | ⬜ 待修复 | |
| F-19 | 热路径 uppercase 分配 | ⬜ 待修复 | |
| F-20 | gpio char→String 分配 | ✅ 已修复 | 改为 char 直接比较，去掉 .to_string() |
| F-21 | params Vec<u8> for 4 bytes | ✅ 已修复 | 改为 params_buf: [u8;4] + params_len: u8 栈上固定大小 |
| F-22 | unsafe transmute 无注释 | ⬜ 待修复 | |
| F-23 | unwrap() in get_boards | ✅ 已修复 | 改为 ok_or_else 返回 Result |
