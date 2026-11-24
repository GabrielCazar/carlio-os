.globl _start
.text
_start:
    movq %xmm7, %rax
    test %rax, %rax                # execve clears the previous image state
    jne failed
    cmpq $0, (%rsp)
    jne failed
    cmpq $0, 8(%rsp)
    jne failed
    cmpq $0, 16(%rsp)
    jne failed
    lea 24(%rsp), %rsi             # auxv follows empty argv and envp
    xor %r8d, %r8d                 # AT_BASE
    xor %r9d, %r9d                 # AT_ENTRY
    mov $6, %edx
next_aux:
    cmpq $7, (%rsi)
    jne check_entry
    mov 8(%rsi), %r8
check_entry:
    cmpq $9, (%rsi)
    jne advance
    mov 8(%rsi), %r9
advance:
    add $16, %rsi
    dec %edx
    jne next_aux
    lea __ehdr_start(%rip), %rax
    cmp %rax, %r8
    jne failed
    test %r9, %r9
    je failed

    mov $1, %eax
    mov $1, %edi
    lea message(%rip), %rsi
    mov $message_len, %edx
    syscall
    cmp $message_len, %rax
    jne failed
    jmp *%r9                       # entry of the main executable

failed:
    mov $60, %eax
    mov $1, %edi
    syscall
    ud2

message: .ascii "execve: PT_INTERP e auxv verificados em CPL3\n"
.set message_len, . - message
.section .note.GNU-stack,"",@progbits
