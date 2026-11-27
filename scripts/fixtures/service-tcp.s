.global _start
.text
_start:
    mov (%rsp), %r15
    mov $102, %eax
    syscall
    cmp $1000, %eax
    jne fail
    mov $104, %eax
    syscall
    cmp $1001, %eax
    jne fail
    mov $115, %eax
    mov $1, %edi
    lea -8(%rsp), %rsi
    syscall
    cmp $1, %eax
    jne fail
    cmpl $42, -8(%rsp)
    jne fail

    mov $105, %eax
    xor %edi, %edi
    syscall
    cmp $-1, %rax
    jne fail

    mov $106, %eax
    xor %edi, %edi
    syscall
    cmp $-1, %rax
    jne fail

    mov $116, %eax
    xor %edi, %edi
    xor %esi, %esi
    syscall
    cmp $-1, %rax
    jne fail

    xor %eax, %eax
    xor %edi, %edi
    lea -8(%rsp), %rsi
    mov $1, %edx
    syscall
    cmp $-9, %rax
    jne fail

    mov $1, %eax
    mov $1, %edi
    syscall
    cmp $-9, %rax
    jne fail

    cmp $2, %r15
    je spinning

    mov $293, %eax
    lea -16(%rsp), %rdi
    mov $0x80800, %esi
    syscall
    test %rax, %rax
    jne fail
    mov -16(%rsp), %ebx
    mov -12(%rsp), %r14d
    mov $32, %eax
    mov %r14d, %edi
    syscall
    test %rax, %rax
    js fail
    mov %eax, %r14d
    mov $3, %eax
    syscall
    test %rax, %rax
    jne fail
    mov $292, %eax
    mov %r14d, %edi
    mov $1, %esi
    mov $0x80000, %edx
    syscall
    cmp $1, %rax
    jne fail
    mov $3, %eax
    syscall
    test %rax, %rax
    jne fail
    mov $1, %r14d
    mov $33, %eax
    mov %r14d, %edi
    mov %r14d, %esi
    syscall
    cmp $1, %rax
    jne fail
    mov $72, %eax
    mov $1, %esi
    syscall
    cmp $1, %rax
    jne fail
    mov $72, %eax
    mov %ebx, %edi
    mov $1, %esi
    syscall
    cmp $1, %rax
    jne fail
    mov $72, %eax
    mov $3, %esi
    syscall
    cmp $0x800, %rax
    jne fail
    xor %eax, %eax
    mov %ebx, %edi
    lea -8(%rsp), %rsi
    mov $1, %edx
    syscall
    cmp $-11, %rax
    jne fail
    movb $90, -8(%rsp)
    mov $1, %eax
    mov %r14d, %edi
    syscall
    cmp $1, %rax
    jne fail
    mov $3, %eax
    syscall
    test %rax, %rax
    jne fail
    movb $0, -8(%rsp)
    xor %eax, %eax
    mov %ebx, %edi
    syscall
    cmp $1, %rax
    jne fail
    cmpb $90, -8(%rsp)
    jne fail
    xor %eax, %eax
    syscall
    test %rax, %rax
    jne fail
    mov $3, %eax
    syscall
    test %rax, %rax
    jne fail

    mov $39, %eax
    syscall
    mov %rax, %r15
    mov $53, %eax
    mov $1, %edi
    mov $1, %esi
    xor %edx, %edx
    lea -16(%rsp), %r10
    syscall
    test %rax, %rax
    jne fail
    mov -16(%rsp), %ebx
    mov -12(%rsp), %r14d
    mov $33, %eax
    mov %ebx, %edi
    mov $3, %esi
    syscall
    cmp $3, %rax
    jne fail
    mov $3, %eax
    mov %ebx, %edi
    syscall
    test %rax, %rax
    jne fail
    mov $3, %ebx
    mov $41, %eax
    mov $2, %edi
    mov $1, %esi
    xor %edx, %edx
    syscall
    cmp $4, %rax
    jne fail
    mov %eax, %edi
    mov $3, %eax
    syscall
    test %rax, %rax
    jne fail
    mov $2, %eax
    lea null_path(%rip), %rdi
    mov $2, %esi
    xor %edx, %edx
    syscall
    test %rax, %rax
    js fail
    mov %eax, %r12d
    mov $33, %eax
    mov %r12d, %edi
    mov $4, %esi
    syscall
    cmp $4, %rax
    jne fail
    mov $3, %eax
    mov %r12d, %edi
    syscall
    test %rax, %rax
    jne fail
    xor %eax, %eax
    mov $4, %edi
    lea -8(%rsp), %rsi
    mov $1, %edx
    syscall
    test %rax, %rax
    jne fail
    mov $1, %eax
    syscall
    cmp $1, %rax
    jne fail
    mov $3, %eax
    syscall
    test %rax, %rax
    jne fail
    mov $2, %eax
    lea own_path(%rip), %rdi
    xor %esi, %esi
    xor %edx, %edx
    syscall
    test %rax, %rax
    js fail
    mov %eax, %r12d
    movl $-1, -20(%rsp)
    mov $56, %eax
    mov $0x01200011, %edi
    xor %esi, %esi
    xor %edx, %edx
    lea -20(%rsp), %r10
    xor %r8d, %r8d
    syscall
    test %rax, %rax
    js fail
    jz child
    cmpl $-1, -20(%rsp)
    jne fail
    mov %rax, %r13
    mov $3, %eax
    mov %r14d, %edi
    syscall
    test %rax, %rax
    jne fail
    movb $81, -8(%rsp)
    mov $1, %eax
    mov %ebx, %edi
    lea -8(%rsp), %rsi
    mov $1, %edx
    syscall
    cmp $1, %rax
    jne fail
    mov $48, %eax
    mov $1, %esi
    syscall
    test %rax, %rax
    jne fail
    mov $61, %eax
    mov %r13, %rdi
    lea -8(%rsp), %rsi
    xor %edx, %edx
    xor %r10d, %r10d
    syscall
    cmp %r13, %rax
    jne fail
    cmpl $0x2500, -8(%rsp)
    jne fail
    xor %eax, %eax
    mov %r12d, %edi
    mov $1, %edx
    syscall
    cmp $1, %rax
    jne fail
    cmpb $69, -8(%rsp)
    jne fail
    mov $3, %eax
    syscall
    test %rax, %rax
    jne fail
    xor %eax, %eax
    mov %ebx, %edi
    mov $1, %edx
    syscall
    cmp $1, %rax
    jne fail
    cmpb $86, -8(%rsp)
    jne fail
    xor %eax, %eax
    syscall
    test %rax, %rax
    jne fail
    mov $3, %eax
    syscall
    test %rax, %rax
    jne fail

    mov $41, %eax
    mov $2, %edi
    mov $1, %esi
    xor %edx, %edx
    syscall
    test %rax, %rax
    js fail
    mov %eax, %r12d
    mov $72, %eax
    mov %r12d, %edi
    mov $3, %esi
    syscall
    cmp $2, %rax
    jne fail
    mov $49, %eax
    mov %r12d, %edi
    lea endpoint(%rip), %rsi
    mov $16, %edx
    syscall
    test %rax, %rax
    jne fail
    mov $50, %eax
    mov %r12d, %edi
    mov $4, %esi
    syscall
    test %rax, %rax
    jne fail

accept:
    mov $43, %eax
    mov %r12d, %edi
    movl $16, -40(%rsp)
    lea -32(%rsp), %rsi
    lea -40(%rsp), %rdx
    int $0x80
    test %rax, %rax
    js fail
    mov %eax, %r13d
    mov $52, %eax
    mov %r13d, %edi
    movl $16, -44(%rsp)
    lea -64(%rsp), %rsi
    lea -44(%rsp), %rdx
    syscall
    test %rax, %rax
    jne fail
    cmpl $16, -44(%rsp)
    jne fail
    mov -32(%rsp), %rax
    cmp -64(%rsp), %rax
    jne fail
    mov $57, %eax
    syscall
    test %rax, %rax
    js fail
    jz tcp_child
    mov %rax, %r14
    mov $61, %eax
    mov %r14, %rdi
    lea -8(%rsp), %rsi
    xor %edx, %edx
    xor %r10d, %r10d
    syscall
    cmp %r14, %rax
    jne fail
    cmpl $0, -8(%rsp)
    jne fail
read:
    xor %eax, %eax
    mov %r13d, %edi
    lea -8(%rsp), %rsi
    mov $1, %edx
    syscall
    cmp $1, %rax
    jne close
    mov $1, %eax
    syscall
    cmp $1, %rax
    je read
close:
    mov $3, %eax
    mov %r13d, %edi
    syscall
    jmp accept

tcp_child:
    mov $3, %eax
    mov %r12d, %edi
    syscall
    test %rax, %rax
    jne fail
    mov $33, %eax
    mov %r13d, %edi
    xor %esi, %esi
    syscall
    test %rax, %rax
    jne fail
    mov $33, %eax
    xor %edi, %edi
    mov $1, %esi
    syscall
    cmp $1, %rax
    jne fail
    mov $32, %eax
    xor %edi, %edi
    syscall
    cmp $2, %rax
    jne fail
    mov $72, %eax
    mov $2, %edi
    mov $2, %esi
    mov $1, %edx
    syscall
    test %rax, %rax
    jne fail
    mov $3, %eax
    syscall
    test %rax, %rax
    jne fail
    mov $3, %eax
    mov %r13d, %edi
    syscall
    test %rax, %rax
    jne fail
    xor %eax, %eax
    xor %edi, %edi
    lea -8(%rsp), %rsi
    mov $1, %edx
    syscall
    cmp $1, %rax
    jne fail
    mov $1, %eax
    mov $1, %edi
    syscall
    cmp $1, %rax
    jne fail
    cmpb $33, -8(%rsp)
    jne tcp_child_close
    mov $48, %eax
    mov $1, %esi
    syscall
    test %rax, %rax
    jne fail
    xor %eax, %eax
    xor %edi, %edi
    lea -8(%rsp), %rsi
    syscall
    cmp $1, %rax
    jne fail
    cmpb $63, -8(%rsp)
    jne fail
tcp_child_close:
    mov $3, %eax
    xor %edi, %edi
    syscall
    test %rax, %rax
    jne fail
    mov $3, %eax
    mov $1, %edi
    syscall
    test %rax, %rax
    jne fail
    mov $60, %eax
    xor %edi, %edi
    syscall
    ud2

child:
    mov $186, %eax
    syscall
    cmp -20(%rsp), %eax
    jne fail
    mov $110, %eax
    syscall
    cmp %r15, %rax
    jne fail
    xor %eax, %eax
    mov %r12d, %edi
    lea -8(%rsp), %rsi
    mov $1, %edx
    syscall
    cmp $1, %rax
    jne fail
    cmpb $127, -8(%rsp)
    jne fail
    mov $3, %eax
    mov %ebx, %edi
    syscall
    test %rax, %rax
    jne fail
    mov $45, %eax
    mov %r14d, %edi
    lea -8(%rsp), %rsi
    mov $1, %edx
    xor %r10d, %r10d
    xor %r8d, %r8d
    xor %r9d, %r9d
    syscall
    cmp $1, %rax
    jne fail
    cmpb $81, -8(%rsp)
    jne fail
    mov $45, %eax
    syscall
    test %rax, %rax
    jne fail
    movb $86, -8(%rsp)
    mov $44, %eax
    mov %r14d, %edi
    lea -8(%rsp), %rsi
    mov $1, %edx
    syscall
    cmp $1, %rax
    jne fail
    mov $60, %eax
    mov $37, %edi
    syscall
    ud2

spinning:
    pause
    jmp spinning
fail:
    mov $60, %eax
    mov $1, %edi
    syscall
    ud2

.section .rodata
endpoint:
    .byte 2, 0, 74, 133
    .zero 12
own_path:
    .asciz "/service-tcp"

null_path:
    .asciz "/dev/null"
