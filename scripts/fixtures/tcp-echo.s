.global _start
.text
_start:
    mov $41, %eax
    mov $2, %edi
    mov $0x801, %esi
    xor %edx, %edx
    syscall
    test %rax, %rax
    js fail
    mov %eax, %r12d

    mov $49, %eax
    mov %r12d, %edi
    lea endpoint(%rip), %rsi
    mov $16, %edx
    syscall
    test %rax, %rax
    jnz fail

    mov $50, %eax
    mov %r12d, %edi
    mov $4, %esi
    syscall
    test %rax, %rax
    jnz fail

    mov $1, %eax
    mov $1, %edi
    lea ready(%rip), %rsi
    mov $ready_len, %edx
    syscall
accept:
    mov $288, %eax
    mov %r12d, %edi
    xor %esi, %esi
    xor %edx, %edx
    mov $0x80800, %r10d
    syscall
    cmp $-11, %rax
    je accept
    test %rax, %rax
    js fail
    mov %eax, %r13d

    xor %r14d, %r14d
receive:
    mov $45, %eax
    mov %r13d, %edi
    lea buffer(%rip), %rsi
    add %r14, %rsi
    mov $5, %edx
    sub %r14d, %edx
    xor %r10d, %r10d
    xor %r8d, %r8d
    syscall
    cmp $-11, %rax
    je receive
    test %rax, %rax
    jle fail
    add %rax, %r14
    cmp $5, %r14
    jb receive

    cmpl $0x676e6970, buffer(%rip)
    jne fail
    cmpb $33, buffer+4(%rip)
    jne fail

    mov $44, %eax
    mov %r13d, %edi
    lea response(%rip), %rsi
    mov $5, %edx
    mov $0x4000, %r10d
    xor %r8d, %r8d
    syscall
    cmp $5, %rax
    jne fail

    mov $3, %eax
    mov %r13d, %edi
    syscall
    mov $3, %eax
    mov %r12d, %edi
    syscall
    xor %edi, %edi
    jmp exit
fail:
    mov $1, %edi
exit:
    mov $60, %eax
    syscall
    ud2

.section .rodata
endpoint:
    .byte 2, 0, 0x4a, 0x85
    .zero 12
ready:
    .ascii "TCP CPL3 pronto\n"
    .equ ready_len, . - ready
response:
    .ascii "pong!"
.bss
buffer:
    .skip 5
