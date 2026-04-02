#!/bin/bash
# 运行基于 Unicorn 的高性能模拟器
cargo run -p rsemu-cli --release -- \
  --svd examples/stm32f407.svd \
  --firmware stm32f4xx-hal/target/thumbv7em-none-eabihf/release/examples/screen-color \
  --target STM32F407 --fast "$@"
