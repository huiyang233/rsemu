.syntax unified
.thumb
.cpu cortex-m3

.equ STACK_TOP, 0x20005000

.section .vector_table, "a", %progbits
.word STACK_TOP
.word reset_handler + 1

.section .text.reset_handler, "ax", %progbits
.thumb_func
.global reset_handler
reset_handler:
    nop
loop:
    b loop
