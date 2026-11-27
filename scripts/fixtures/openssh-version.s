.global _start
.text
_start:
    xor %edx, %edx
    push %rdx
.if CONFIG_MODE
    lea config(%rip), %rax
    push %rax
    lea config_flag(%rip), %rax
    push %rax
    lea stderr_flag(%rip), %rax
    push %rax
.endif
    lea mode(%rip), %rax
    push %rax
    lea daemon(%rip), %rdi
    push %rdi
    mov %rsp, %rsi
    mov $59, %eax
    syscall
    neg %eax
    mov %eax, %edi
    mov $60, %eax
    syscall
    ud2
.section .rodata
daemon:
    .asciz "/usr/bin/sshd"
mode:
.if CONFIG_MODE
.if CONFIG_MODE == 2
    .asciz "-D"
.else
    .asciz "-T"
.endif
config:
    .asciz "/etc/ssh/probe_config"
config_flag:
    .asciz "-f"
stderr_flag:
    .asciz "-e"
.else
    .asciz "-V"
.endif
