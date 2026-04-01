.syntax unified
.thumb
.cpu cortex-m3

.equ STACK_TOP, 0x20005000

.section .vector_table, "a", %progbits
.word STACK_TOP
.word reset_handler + 1

.section .text.reset_handler, "ax", %progbits
.align 2
.thumb_func
.global reset_handler
reset_handler:
    b   send_char

.thumb_func
send_char:
    .hword 0x4809  // ldr r0, [pc, #36] => addr_rcc_apb2enr
    .hword 0x4909  // ldr r1, [pc, #36] => val_gpio_enable
    str r1, [r0]

    .hword 0x4809  // ldr r0, [pc, #36] => addr_rcc_apb1enr
    .hword 0x490A  // ldr r1, [pc, #40] => val_usart3_enable
    str r1, [r0]

    .hword 0x480A  // ldr r0, [pc, #40] => addr_gpiob_crh
    .hword 0x490A  // ldr r1, [pc, #40] => val_pb10_af_push_pull
    str r1, [r0]

    .hword 0x480A  // ldr r0, [pc, #40] => addr_usart3_brr
    .hword 0x490B  // ldr r1, [pc, #44] => val_usart3_brr
    str r1, [r0]

    .hword 0x480B  // ldr r0, [pc, #44] => addr_usart3_cr1
    .hword 0x490B  // ldr r1, [pc, #44] => val_usart3_cr1
    str r1, [r0]

    .hword 0x480B  // ldr r0, [pc, #44] => addr_usart3_dr
    movs r1, #72
    str r1, [r0]

loop:
    b loop

addr_rcc_apb2enr:
    .word 0x40021018
val_gpio_enable:
    .word 0x00000008
addr_rcc_apb1enr:
    .word 0x4002101C
val_usart3_enable:
    .word 0x00040000
addr_gpiob_crh:
    .word 0x40010C04
val_pb10_af_push_pull:
    .word 0x00000B00
addr_usart3_brr:
    .word 0x40004808
val_usart3_brr:
    .word 0x00000138
addr_usart3_cr1:
    .word 0x4000480C
val_usart3_cr1:
    .word 0x00002008
addr_usart3_dr:
    .word 0x40004804
