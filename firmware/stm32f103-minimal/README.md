# stm32f103-minimal

Build commands:

```bash
arm-none-eabi-gcc -mcpu=cortex-m3 -mthumb -nostdlib -Wl,-T,firmware/stm32f103-minimal/linker.ld -Wl,-Map,target/stm32f103-minimal.map -o target/stm32f103-minimal.elf firmware/stm32f103-minimal/startup.S
arm-none-eabi-objcopy -O binary target/stm32f103-minimal.elf target/stm32f103-minimal.bin
```
