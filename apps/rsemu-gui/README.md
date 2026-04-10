# rsemu-gui

rsemu 的图形化调试界面，基于 Tauri 2 + React + TypeScript 构建。面向不熟悉命令行的用户，提供可视化的板子配置和实时仿真预览。

---

## 功能概览

| 功能 | 说明 |
|------|------|
| 板子选择 | 支持 STM32F103 (Cortex-M3) 和 STM32F407 (Cortex-M4) |
| 可视化布局 | 拖拽外设到画布，点击配置引脚（Wokwi 风格） |
| 固件加载 | 原生文件对话框选择 `.bin` 固件 |
| 一键运行 | 后台线程启动模拟器，实时推送事件到前端 |
| LED 显示 | GPIO ODR/BSRR 变化实时反映到 LED 图标亮灭 |
| ST7789 屏幕 | SPI 帧解码后渲染到 Canvas 元素 |
| SSD1306 屏幕 | I2C 帧解码后渲染到 Canvas 元素 |
| UART 终端 | 收发串口数据，内嵌终端面板 |
| Button 输入 | 点击按下/松开，注入 GPIO IDR 电平 |

---

## 技术栈

```
前端                          后端 (Rust)
─────────────────────         ──────────────────────
React 18 + TypeScript         Tauri 2
Vite 5 (打包/热更新)          rsemu-core  (CPU 模拟)
Tailwind CSS 3 (样式)         rsemu-peripherals (外设)
Zustand 5 (状态管理)          rsemu-targets (STM32 目标)
dnd-kit (拖拽)                tauri-plugin-dialog (文件对话框)
@tauri-apps/api 2             base64 (帧数据编码)
```

---

## 目录结构

```
apps/rsemu-gui/
├── src-tauri/                    # Rust 后端
│   ├── Cargo.toml
│   ├── tauri.conf.json           # 窗口、构建配置
│   ├── capabilities/default.json # Tauri v2 权限声明
│   ├── icons/                    # 应用图标
│   └── src/
│       ├── main.rs               # 程序入口
│       ├── lib.rs                # Tauri builder，注册命令和插件
│       ├── state.rs              # SimState、SimConfig、ControlMsg 定义
│       ├── emulator.rs           # 后台线程：运行模拟器，发送 Tauri 事件
│       └── commands/
│           ├── mod.rs
│           ├── board.rs          # get_boards 命令
│           └── simulation.rs     # start/stop/inject_gpio/send_uart/open_dialog
│
├── src/                          # React 前端
│   ├── main.tsx                  # 应用入口
│   ├── App.tsx                   # 根组件，全局事件订阅
│   ├── styles/globals.css        # Tailwind 基础样式（暗色主题）
│   │
│   ├── types/                    # TypeScript 类型定义
│   │   ├── board.ts              # BoardInfo, BusPeripheralInfo
│   │   ├── peripheral.ts         # PeripheralConfig, CanvasItem, PinMapping
│   │   └── simulation.ts         # 事件 payload 类型
│   │
│   ├── store/
│   │   └── appStore.ts           # Zustand store：页面状态、画布、运行时数据
│   │
│   ├── lib/
│   │   ├── tauri.ts              # 所有 Tauri 命令和事件监听的封装
│   │   └── peripheralDefs.ts     # 外设类型元数据（标签、颜色、引脚定义）
│   │
│   ├── hooks/
│   │   └── useEmulatorEvents.ts  # 订阅后端事件，更新 store
│   │
│   ├── components/
│   │   ├── ui/                   # 基础 UI 组件
│   │   │   ├── Button.tsx
│   │   │   ├── Select.tsx
│   │   │   └── Badge.tsx
│   │   ├── canvas/               # 配置页画布相关组件
│   │   │   ├── SimCanvas.tsx     # 主画布（DnD 容器）
│   │   │   ├── PeripheralPalette.tsx  # 左侧外设拖拽面板
│   │   │   ├── PeripheralNode.tsx    # 画布上的外设卡片
│   │   │   └── PinConfigPanel.tsx   # 引脚配置弹窗
│   │   └── peripherals/          # 仿真页外设 widget
│   │       ├── LedWidget.tsx     # LED 亮灭指示
│   │       ├── DisplayWidget.tsx # ST7789 屏幕渲染
│   │       ├── Ssd1306Widget.tsx # SSD1306(I2C) 屏幕渲染
│   │       ├── UartTerminal.tsx  # 串口终端
│   │       └── ButtonWidget.tsx  # 可交互按钮
│   │
│   └── pages/
│       ├── SetupPage/            # 配置页（选板子 → 添加外设 → 选固件）
│       │   ├── index.tsx
│       │   ├── BoardSelector.tsx
│       │   └── FirmwarePicker.tsx
│       └── SimulationPage/       # 仿真页（实时显示）
│           ├── index.tsx
│           └── ControlBar.tsx
│
├── index.html
├── package.json
├── vite.config.ts
├── tailwind.config.ts
├── tsconfig.json
└── postcss.config.js
```

---

## SVD 文件

GUI 已在编译时将 STM32F103 和 STM32F407 的 SVD 文件嵌入二进制（位于 `src-tauri/svd/`），**用户无需手动提供 SVD 文件**。

如需更新 SVD（例如使用更精确的寄存器定义），替换对应文件后重新编译即可：

```
src-tauri/svd/stm32f103.svd   ← 替换此文件
src-tauri/svd/stm32f407.svd   ← 替换此文件
```

> CLI（`rsemu-cli`）仍需通过 `board.toml` 中的 `svd` 字段手动指定路径。

---

## 快速开始

### 前置条件

- Rust 工具链（stable，推荐通过 rustup 安装）
- Node.js 18+
- 系统依赖（macOS 开箱即用；Linux 需安装 GTK3、WebKit2GTK 等，详见 [Tauri 文档](https://tauri.app/start/prerequisites/)）

### 安装依赖

```bash
cd apps/rsemu-gui
npm install
```

### 开发模式启动

```bash
npm run tauri dev
```

第一次运行会编译 Rust 依赖，耗时较长（约 2-5 分钟）。之后热更新前端代码无需重新编译 Rust。

### 生产构建

```bash
npm run tauri build
```

产物在 `src-tauri/target/release/bundle/` 下。

---

## 使用流程

```
1. 选择板子
   └─ STM32F103 或 STM32F407

2. 添加外设（画布区域）
   └─ 从左侧面板拖拽外设到画布
   └─ 点击外设卡片 → Configure pins → 填写引脚配置 → Save

3. 选择固件
   └─ 点击 "Browse .bin" 选择编译好的固件二进制文件

4. 运行仿真
   └─ 点击 ▶ Run Simulation（所有外设配置完成后才可点击）
   └─ 自动跳转到仿真页

5. 仿真页实时查看
   └─ LED 亮灭状态
   └─ ST7789 / SSD1306 屏幕渲染
   └─ UART 终端输出 / 发送数据
   └─ 点击 Button 注入 GPIO 输入

6. 停止 / 返回配置
   └─ 点击 ■ Stop 停止模拟器
   └─ 点击 ← Back to Setup 返回重新配置
```

---

## 后端架构

### 事件流

```
前端                         Rust 后端（主线程）         模拟器线程
─────                        ──────────────────          ──────────
invoke("start_simulation") ──► spawn thread ────────────► run_emulator()
                                                           │
                                                           ├─ step_cpu()
                                                           ├─ 处理 MMIO 事件
                                                           │   ├─ LED 状态变化 ──► emit("led-changed")
                                                           │   ├─ 显示新帧(ST7789/SSD1306) ──► emit("display-frame")
                                                           │   └─ UART 输出    ──► emit("uart-output")
                                                           └─ 检查控制消息
                                                               ├─ Stop
                                                               ├─ InjectGpio
                                                               └─ SendUart
```

### Tauri 命令

| 命令 | 参数 | 说明 |
|------|------|------|
| `get_boards` | — | 返回支持的板子列表 |
| `start_simulation` | `SimConfig` | 启动模拟器后台线程 |
| `stop_simulation` | — | 发送 Stop 控制消息 |
| `inject_gpio` | `port, pin, high` | 注入 GPIO IDR 电平（用于按钮） |
| `send_uart` | `peripheral, bytes` | 向 USART RX 队列推送数据 |
| `open_firmware_dialog` | — | 打开原生文件选择对话框 |

### Tauri 事件

| 事件 | Payload | 触发条件 |
|------|---------|---------|
| `sim-status` | `{ steps, running, error? }` | 模拟器启动/停止/出错 |
| `led-changed` | `{ id, on }` | GPIO ODR/BSRR 导致 LED 状态变化 |
| `display-frame` | `{ width, height, data }` | ST7789/SSD1306 产出新帧（base64 ARGB） |
| `uart-output` | `{ peripheral, byte }` | USART TX 输出一个字节 |
| `sim-steps` | `number` | 定期步数更新 |

### 显示帧格式

`display-frame.data` 是 base64 编码的原始像素数据：
- 每像素 4 字节，小端序 `[B, G, R, A]`（对应 Rust 的 `u32` ARGB）
- 前端解码后转为 `[R, G, B, A]` 写入 Canvas `ImageData`

---

## 新增外设（扩展指南）

以新增一个 **七段数码管** 外设为例：

### 1. 后端：在 `state.rs` 添加配置类型

```rust
#[serde(rename = "seg7")]
Seg7 {
    id: String,
    // ... 引脚定义
}
```

### 2. 后端：在 `emulator.rs` 处理该外设

在 `run_machine` 中根据配置创建外设实例，在 `process_events` 中读取状态并 `emit` 事件。

### 3. 前端：在 `lib/peripheralDefs.ts` 注册元数据

```typescript
{
  type: "seg7",
  label: "7-Segment Display",
  color: "bg-orange-900",
  textColor: "text-orange-200",
  icon: "🔢",
  pins: [/* ... */],
}
```

### 4. 前端：新建 `components/peripherals/Seg7Widget.tsx`

订阅对应事件，渲染 widget。

### 5. 前端：在 `SimulationPage/index.tsx` 渲染新 widget

---

## 与 CLI 的关系

| 特性 | rsemu-cli | rsemu-gui |
|------|-----------|-----------|
| 目标用户 | 开发者 | 初学者 / 可视化调试 |
| 配置方式 | `board.toml` | 图形界面 |
| 显示方式 | minifb 独立窗口 | 集成在 GUI 面板内 |
| UART | 终端 stdin/stdout | 内嵌终端面板 |
| 按钮输入 | 不支持 | 可点击 |
| 依赖 | 轻量 | 需要 Node.js + Tauri |

两者共享同一套核心库（`rsemu-core`、`rsemu-peripherals`、`rsemu-targets`），行为完全一致。
