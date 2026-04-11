# rsemu

`rsemu` 是一个基于 [unicorn-engine](https://github.com/unicorn-engine/unicorn) 的 STM32 MCU 仿真器，支持可插拔外设，提供 CLI 和 Tauri GUI 两种运行方式。

[English](README.md)

## 支持的目标芯片

芯片配置采用 JSON 驱动 — 新增芯片只需添加 JSON + SVD 文件，无需修改 Rust 代码。

| 目标芯片 | 内核 | HSI | 配置文件 |
|----------|------|-----|----------|
| STM32F103 | Cortex-M3 | 8 MHz | `configs/stm32f103.json` |
| STM32F401 | Cortex-M4 | 16 MHz | `configs/stm32f401.json` |
| STM32F407 | Cortex-M4 | 16 MHz | `configs/stm32f407.json` |
| STM32F411 | Cortex-M4 | 16 MHz | `configs/stm32f411.json` |
| STM32F427 | Cortex-M4 | 16 MHz | `configs/stm32f427.json` |
| STM32F429 | Cortex-M4 | 16 MHz | `configs/stm32f429.json` |

## 支持的外设

| 类型 | 总线 | 说明 |
|------|------|------|
| ST7789 (SPI) | SPI + GPIO | LCD 显示屏，240x320 等 |
| ST7789 (FSMC) | FSMC | 并口 LCD 显示屏 |
| SSD1306 (I2C) | I2C | OLED 单色显示屏 |
| LED | GPIO | 引脚状态监控，带回调通知 |
| UART | USART | 串口终端（GUI 中使用 xterm.js） |
| Button | GPIO | GPIO 输入注入 |
| Custom | — | 通用扩展，使用 `HashMap<String, String>` 参数 |

## CLI 运行

```bash
cargo run -p rsemu-cli --release -- --board examples/board.toml --no-gui --max-steps 1000000
```

参数说明：
- `--board <path>` — 板级配置文件路径（默认 `board.toml`）
- `--max-steps <n>` — 指令步数上限
- `--no-gui` — 禁用显示窗口
- `--fast` — 不做实时 pacing，全速运行
- `--dump-frames` — 将 ST7789 帧数据写入磁盘

## GUI 运行

```bash
cd apps/rsemu-gui
npm install
npm run tauri dev                # 开发模式
npm run tauri dev -- --release   # 后端 release 模式，性能更好
npm run tauri build              # 构建生产包
```

环境变量：
- `RSEMU_GUI_STEP_TRACE=1` — 开启详细单步追踪，用于问题诊断

## 板级配置

完整示例见 [`examples/board.toml`](examples/board.toml)。

```toml
target = "STM32F407"
firmware = "firmware/rtthread.bin"
load_addr = 0x08000000

[[peripherals]]
type = "st7789_spi"
width = 240
height = 320
spi_base = 0x40013000
cs = { port = "A", pin = 4 }
dc = { port = "A", pin = 3 }

[[peripherals]]
type = "led"
pin = { port = "F", pin = 12 }
active_low = true
```

## 工作区结构

```
crates/rsemu-core          — CPU、内存总线、机器运行时、总线-设备 trait
crates/rsemu-svd           — CMSIS SVD 寄存器描述解析器
crates/rsemu-targets       — 目标注册表（JSON 配置 + SVD 加载）
crates/rsemu-peripherals   — 可插拔外设实现
apps/rsemu-cli             — 板级加载 + CLI 运行循环
apps/rsemu-gui             — Tauri 2.0 GUI（Rust 后端 + React/TS 前端）
configs/                   — 目标芯片 JSON 配置（每芯片一个文件）
svds/                      — CMSIS SVD 文件
examples/                  — 板级配置示例
```

## 许可证

MIT
