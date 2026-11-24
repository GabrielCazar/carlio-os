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
    movq %xmm7, %rax
    test %rax, %rax                # execve clears the previous image state
    jne failed
    cmpq $0, (%rsp)
    jne failed
    movabs $0x123456789abcdef0, %r15
    movq %r15, %xmm7
    mov $0x1000000, %ecx
1:  loop 1b                        # permit a scheduler timer tick
    movq %xmm7, %rax
    cmp %r15, %rax
    jne failed
    lea disk_executable(%rip), %rdi
    mov $5, %esi                   # R_OK | X_OK on the mounted rootfs
    syscall_nr 21
    require 0
    lea missing_file(%rip), %rdi
    xor %esi, %esi                 # F_OK on an absent file
    syscall_nr 21
    require -2                     # ENOENT

    lea restricted_file(%rip), %rdi
    xor %esi, %esi
    syscall_nr 21                  # access
    require -13
    mov $-100, %edi
    lea restricted_file(%rip), %rsi
    xor %edx, %edx
    syscall_nr 257                 # openat
    require -13
    lea restricted_file(%rip), %rdi
    xor %esi, %esi
    xor %edx, %edx
    syscall_nr 59                  # execve
    require -13

    sub $144, %rsp                  # Linux x86-64 struct stat
    lea directory(%rip), %rdi
    mov $0700, %esi
    syscall_nr 83                        # mkdir
    require 0

    mov $-100, %edi
    lea relative_path(%rip), %rsi
    mov $0x800c2, %edx              # O_CLOEXEC | O_CREAT | O_EXCL | O_RDWR
    mov $0640, %r10d
    syscall_nr 257                       # openat
    test %rax, %rax
    js failed
    mov %rax, %r12

    mov %r12, %rdi
    xor %esi, %esi
    mov $1, %edx
    syscall_nr 20                        # writev with an invalid vector pointer
    require -14
    sub $32, %rsp
    lea payload(%rip), %rax
    mov %rax, (%rsp)
    movq $1, 8(%rsp)
    inc %rax
    mov %rax, 16(%rsp)
    movq $2, 24(%rsp)
    mov %rsp, %rsi
    mov $2, %edx
    syscall_nr 20
    require 3
    add $32, %rsp
    mov %rsp, %rsi
    mov $2, %edx
    mov $1, %r10d
    syscall_nr 17                        # pread64 at offset 1
    require 2
    cmpw $0x5346, (%rsp)            # "FS"
    jne failed
    syscall_nr 0                         # read stays at offset 3
    require 0
    mov $-1, %r10
    syscall_nr 17
    require -22                    # EINVAL for a negative offset

    xor %edi, %edi
    mov $4096, %esi
    mov $3, %edx                   # PROT_READ | PROT_WRITE
    mov $2, %r10d                  # MAP_PRIVATE
    mov %r12, %r8
    mov $1, %r9d
    syscall_nr 9
    require -22                    # Unaligned file offset
    xor %r9d, %r9d
    syscall_nr 9
    test %rax, %rax
    jle failed
    mov %rax, %rdi
    mov $0x100002, %r10d           # MAP_PRIVATE | MAP_FIXED_NOREPLACE
    syscall_nr 9
    require -17                    # EEXIST, preserving the original mapping
    cmpl $0x00534656, (%rdi)       # "VFS" and zero-filled tail
    jne failed
    cmpb $0, 4095(%rdi)
    jne failed
    movb $0, (%rdi)
    mov $0x12, %r10d              # MAP_PRIVATE | MAP_FIXED
    syscall_nr 9
    cmp %rdi, %rax
    jne failed
    cmpl $0x00534656, (%rdi)       # Fresh contents replace the dirty page
    jne failed
    mov $0x32, %r10d              # MAP_PRIVATE | MAP_FIXED | MAP_ANONYMOUS
    mov $-1, %r8d                # The fd argument is a Linux int
    syscall_nr 9
    cmp %rdi, %rax
    jne failed
    cmpl $0, (%rdi)
    jne failed
    syscall_nr 11                  # munmap
    require 0
    mov $0x100002, %r10d
    mov %r12, %r8
    syscall_nr 9                   # The same address is now available
    cmp %rdi, %rax
    jne failed
    cmpl $0x00534656, (%rdi)
    jne failed
    syscall_nr 11
    require 0
    mov %r12, %rdi
    mov %rsp, %rsi
    mov $1, %edx
    xor %r10d, %r10d
    syscall_nr 17
    require 1
    cmpb $'V', (%rsp)              # Private writes never reach the file
    jne failed

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
    movq %xmm7, %rax
    cmp %r15, %rax
    jne failed
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
relative_path: .asciz "tmp/prova-abi-vfs/arquivo"
payload: .ascii "VFS"
message:
    .ascii "execve: ELF do disco executado em CPL3\n"
    .ascii "execve: SSE iniciado e preservado em CPL3\n"
    .ascii "ABI Linux: stat, mutacoes e fsync verificados em CPL3\n"
.set message_len, . - message
disk_executable: .asciz "/carlio-disk-exec"
missing_file: .asciz "/carlio-access-missing"

restricted_file: .asciz "/carlio-privado/probe"

.section .note.GNU-stack,"",@progbits
