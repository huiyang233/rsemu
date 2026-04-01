# stm32f103-usart-hello

Build commands:

```bash
arm-none-eabi-gcc -mcpu=cortex-m3 -mthumb -nostdlib -Wl,-T,firmware/stm32f103-usart-hello/linker.ld -Wl,-Map,target/stm32f103-usart-hello.map -o target/stm32f103-usart-hello.elf firmware/stm32f103-usart-hello/startup.S
arm-none-eabi-objcopy -O binary target/stm32f103-usart-hello.elf target/stm32f103-usart-hello.bin
```
