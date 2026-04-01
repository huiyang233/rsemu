#!/bin/bash
# 运行嵌入了 Unicorn 的 External 后端 (更稳定，支持浮点等)
RSEMU_ARM_CORE=external cargo run -p rsemu-cli --release --features rsemu-core/cpu-external-unicorn -- \
  --svd stm32-emulator/saturn/stm32f407.svd \
  --firmware stm32f4xx-hal/target/thumbv7em-none-eabihf/release/examples/screen-color \
  --target STM32F407 --fast
