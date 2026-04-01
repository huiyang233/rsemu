#!/bin/bash
# 运行自己写的 Legacy 后端 (ARMv7-M 解释器)
RSEMU_ARM_CORE=legacy cargo run -p rsemu-cli --release -- \
  --svd stm32-emulator/saturn/stm32f407.svd \
  --firmware stm32f4xx-hal/target/thumbv7em-none-eabihf/release/examples/screen-color \
  --target STM32F407 --fast
