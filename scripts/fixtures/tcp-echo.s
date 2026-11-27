.global _start
.text
_start:
    mov $41, %eax
    mov $2, %edi
.ifdef BLOCKING
    mov $1, %esi
.else
    mov $0x801, %esi
.endif
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

.ifdef BLOCKING
    call monotonic_ns
    mov %rax, %r15
    mov $7, %eax
    xor %edi, %edi
    xor %esi, %esi
    mov $50, %edx
    syscall
    test %rax, %rax
    jnz fail
    call monotonic_ns
    sub %r15, %rax
    cmp $50000000, %rax
    jb fail
.endif
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
.ifdef BLOCKING
    xor %r10d, %r10d
    int $0x80
.else
    mov $0x80800, %r10d
    syscall
    cmp $-11, %rax
    je accept
.endif
    test %rax, %rax
    js fail
    mov %eax, %r13d

    xor %r14d, %r14d
receive:
.ifdef BLOCKING
    xor %eax, %eax
.else
    mov $45, %eax
.endif
    mov %r13d, %edi
    lea buffer(%rip), %rsi
    add %r14, %rsi
    mov $5, %edx
    sub %r14d, %edx
    xor %r10d, %r10d
    xor %r8d, %r8d
    syscall
.ifndef BLOCKING
    cmp $-11, %rax
    je receive
.endif
    test %rax, %rax
    jle fail
    add %rax, %r14
    cmp $5, %r14
    jb receive

    cmpl $0x676e6970, buffer(%rip)
    jne fail
    cmpb $33, buffer+4(%rip)
    jne fail

.ifdef BLOCKING
    mov $1, %eax
    mov $1, %edi
    lea polling(%rip), %rsi
    mov $polling_len, %edx
    syscall

    mov %r13d, pollfd(%rip)
    movw $1, pollfd+4(%rip)
    mov $7, %eax
    lea pollfd(%rip), %rdi
    mov $1, %esi
    mov $-1, %edx
    syscall
    cmp $1, %rax
    jne fail
    testw $1, pollfd+6(%rip)
    jz fail

    xor %eax, %eax
    mov %r13d, %edi
    lea buffer(%rip), %rsi
    mov $1, %edx
    syscall
    cmp $1, %rax
    jne fail
    cmpb $63, buffer(%rip)
    jne fail
    mov $1, %eax
.else
    mov $44, %eax
.endif
    mov %r13d, %edi
    lea response(%rip), %rsi
    mov $5, %edx
    mov $0x4000, %r10d
    xor %r8d, %r8d
    syscall
    cmp $5, %rax
    jne fail
.ifdef BLOCKING
    mov $1, %eax
    mov %r13d, %edi
    lea response(%rip), %rsi
    mov $5, %edx
    syscall
    cmp $5, %rax
    jne fail
.endif

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

.ifdef BLOCKING
monotonic_ns:
    mov $228, %eax
    mov $1, %edi
    lea timespec(%rip), %rsi
    syscall
    test %rax, %rax
    jnz fail
    imul $1000000000, timespec(%rip), %rax
    add timespec+8(%rip), %rax
    ret
.endif

.section .rodata
endpoint:
    .byte 2, 0, 0x4a, 0x85
    .zero 12
ready:
    .ascii "TCP CPL3 pronto\n"
    .equ ready_len, . - ready
response:
    .ascii "pong!"
.ifdef BLOCKING
polling:
    .ascii "TCP CPL3 poll\n"
    .equ polling_len, . - polling
.endif
.bss
    .balign 8
timespec:
    .skip 16
pollfd:
    .skip 8
buffer:
    .skip 5
