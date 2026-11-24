.globl _start
.text
.macro syscall_nr number
    mov $\number, %eax
    syscall
.endm
.macro require expected
    cmp $\expected, %rax
    jne failed
.endm

_start:
    cmpq $0, (%rsp)
    jne failed
    sub $144, %rsp                  # Linux x86-64 struct stat
    lea directory(%rip), %rdi
    mov $0700, %esi
    syscall_nr 83                        # mkdir
    require 0

    mov $-100, %rdi
    lea path(%rip), %rsi
    mov $0xc2, %edx                 # O_CREAT | O_EXCL | O_RDWR
    mov $0640, %r10d
    syscall_nr 257                       # openat
    test %rax, %rax
    js failed
    mov %rax, %r12

    mov %r12, %rdi
    lea payload(%rip), %rsi
    mov $3, %edx
    syscall_nr 1                         # write
    require 3
    mov $0600, %esi
    syscall_nr 91                        # fchmod
    require 0
    syscall_nr 74                        # fsync
    require 0

    xor %esi, %esi
    syscall_nr 5                         # fstat with null output
    require -14                    # EFAULT
    mov %rsp, %rsi
    syscall_nr 5
    require 0
    call check_stat

    mov $-100, %rdi
    lea path(%rip), %rsi
    mov %rsp, %rdx
    xor %r10d, %r10d
    syscall_nr 262                       # newfstatat
    require 0
    call check_stat

    mov %r12, %rdi
    syscall_nr 3                         # close
    require 0
    syscall_nr 74                        # fsync of closed descriptor
    require -9                     # EBADF

    lea path(%rip), %rdi
    syscall_nr 87                        # unlink
    require 0
    mov $-100, %rdi
    lea path(%rip), %rsi
    mov %rsp, %rdx
    xor %r10d, %r10d
    syscall_nr 262
    require -2                     # ENOENT
    lea directory(%rip), %rdi
    syscall_nr 84                        # rmdir
    require 0

    mov $1, %edi
    lea message(%rip), %rsi
    mov $message_len, %edx
    syscall_nr 1
    xor %edi, %edi
    jmp finish
failed:
    mov $1, %edi
finish:
    syscall_nr 60
    ud2

check_stat:
    cmpl $0100600, 32(%rsp)         # st_mode, after return address
    jne failed
    cmpq $3, 56(%rsp)               # st_size
    jne failed
    ret

directory: .asciz "/tmp/prova-abi-vfs"
path: .asciz "/tmp/prova-abi-vfs/arquivo"
payload: .ascii "VFS"
message:
    .ascii "execve: ELF do disco executado em CPL3\n"
    .ascii "ABI Linux: stat, mutacoes e fsync verificados em CPL3\n"
.set message_len, . - message
.section .note.GNU-stack,"",@progbits
