ENTRY(_start)
PHDRS {
    text PT_LOAD FLAGS(5);
    rodata PT_LOAD FLAGS(4);
    data PT_LOAD FLAGS(6);
}
SECTIONS {
    . = 0xffffffff80000000;
    .text ALIGN(4K) : { KEEP(*(.text._start)) *(.text .text.*) } :text
    .rodata ALIGN(4K) : { *(.rodata .rodata.*) } :rodata
    .data ALIGN(4K) : {
        KEEP(*(.limine_requests_start))
        KEEP(*(.limine_requests))
        KEEP(*(.limine_requests_end))
        *(.data .data.*) *(.got .got.*)
    } :data
    .bss ALIGN(4K) (NOLOAD) : { *(COMMON) *(.bss .bss.*) } :data
    /DISCARD/ : { *(.eh_frame .eh_frame_hdr) }
}
