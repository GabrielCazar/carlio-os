#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use carlio_os::errors::{ErrorLog, KernelError};
#[cfg(target_os = "none")]
use carlio_os::pci::PciFunction;
use carlio_os::{parse_command, Command};
use core::arch::asm;
#[cfg(target_os = "none")]
use core::fmt::{self, Write as FmtWrite};
#[cfg(target_os = "none")]
use core::panic::PanicInfo;

const COM1: u16 = 0x3f8;
const INPUT_LIMIT: usize = 128;
const SERIAL_POLL_LIMIT: usize = 1_000_000;
const ERROR_LOG_CAPACITY: usize = 16;

static mut ERROR_LOG: ErrorLog<ERROR_LOG_CAPACITY> = ErrorLog::new();
static mut HHDM_OFFSET: u64 = 0;
static mut KERNEL_PHYSICAL_BASE: u64 = 0;
static mut KERNEL_VIRTUAL_BASE: u64 = 0;

#[repr(C)]
struct LimineKernelAddressResponse {
    revision: u64,
    physical_base: u64,
    virtual_base: u64,
}

#[repr(C)]
struct LimineKernelAddressRequest {
    id: [u64; 4],
    revision: u64,
    response: *mut LimineKernelAddressResponse,
}

#[repr(C)]
struct LimineHhdmResponse {
    revision: u64,
    offset: u64,
}

#[repr(C)]
struct LimineHhdmRequest {
    id: [u64; 4],
    revision: u64,
    response: *mut LimineHhdmResponse,
}

#[repr(C)]
struct LimineMemmapEntry {
    base: u64,
    length: u64,
    kind: u64,
}

#[repr(C)]
struct LimineMemmapResponse {
    revision: u64,
    entry_count: u64,
    entries: *mut *mut LimineMemmapEntry,
}

#[repr(C)]
struct LimineMemmapRequest {
    id: [u64; 4],
    revision: u64,
    response: *mut LimineMemmapResponse,
}

#[used]
#[link_section = ".limine_requests"]
static mut HHDM_REQUEST: LimineHhdmRequest = LimineHhdmRequest {
    id: [
        0xc7b1_dd30_df4c_8b88,
        0x0a82_e883_a194_f07b,
        0x48dc_f1cb_8ad2_b852,
        0x6398_4e95_9a98_244b,
    ],
    revision: 0,
    response: core::ptr::null_mut(),
};

#[used]
#[link_section = ".limine_requests"]
static mut MEMMAP_REQUEST: LimineMemmapRequest = LimineMemmapRequest {
    id: [
        0xc7b1_dd30_df4c_8b88,
        0x0a82_e883_a194_f07b,
        0x67cf_3d9d_378a_806f,
        0xe304_acdf_c50c_3c62,
    ],
    revision: 0,
    response: core::ptr::null_mut(),
};

#[used]
#[link_section = ".limine_requests"]
static mut KERNEL_ADDRESS_REQUEST: LimineKernelAddressRequest = LimineKernelAddressRequest {
    id: [
        0xc7b1_dd30_df4c_8b88,
        0x0a82_e883_a194_f07b,
        0x71ba_7686_3cc5_5f63,
        0xb264_4a48_c516_a487,
    ],
    revision: 0,
    response: core::ptr::null_mut(),
};

#[cfg(target_os = "none")]
#[path = "arch/x86_64/ahci.rs"]
mod ahci;
#[cfg(target_os = "none")]
#[path = "arch/x86_64/idt.rs"]
mod idt;
#[cfg(target_os = "none")]
#[path = "arch/x86_64/pci.rs"]
mod pci_device;

#[used]
#[link_section = ".limine_requests_start"]
static REQUESTS_START: [u64; 2] = [0xf6b8_f4b3_9de7_d1ae, 0xfab9_1a69_40fc_b9cf];

#[used]
#[link_section = ".limine_requests"]
static BASE_REVISION: [u64; 3] = [0xf956_2b2d_5c95_a6c8, 0x6a7b_3849_4453_6bdc, 3];

#[used]
#[link_section = ".limine_requests_end"]
static REQUESTS_END: [u64; 2] = [0xadc0_e053_1bb1_0d03, 0x9572_709f_3176_4c62];

#[no_mangle]
#[cfg(target_os = "none")]
pub extern "C" fn _start() -> ! {
    serial_init();
    unsafe { idt::install() };
    startup_screen();
    write("IDT: exceções 0-31 registradas; IRQs externos permanecem desativados.\r\n");
    write("Aviso: terminal de diagnóstico em ring 0; não é uma fronteira de segurança.\r\n");
    report_memory_map();
    enumerate_pci();
    unsafe { idt::start_watchdog() };
    write("Watchdog PIT ativo: prazo de 5 s sem progresso.\r\n");
    write("Digite 'ajuda' para ver os comandos.\r\n\r\n");
    shell()
}

#[cfg(not(target_os = "none"))]
fn main() {}

fn shell() -> ! {
    let mut line = [0u8; INPUT_LIMIT];
    loop {
        write("carlio> ");
        let mut len = 0;
        let mut overflowed = false;
        loop {
            let byte = match serial_read() {
                Ok(byte) => byte,
                Err(_) => continue,
            };
            match byte {
                b'\r' | b'\n' => {
                    write("\r\n");
                    break;
                }
                8 | 127 if len > 0 => {
                    len -= 1;
                    write("\x08 \x08");
                }
                32..=126 => {
                    if len < INPUT_LIMIT {
                        line[len] = byte;
                        len += 1;
                        serial_write(byte);
                    } else if !overflowed {
                        overflowed = true;
                        record_error(KernelError::InputTooLong);
                    }
                }
                _ => {}
            }
        }
        if overflowed {
            write("Entrada descartada por exceder o limite.\r\n");
            continue;
        }
        let command = core::str::from_utf8(&line[..len]).unwrap_or("");
        #[cfg(all(target_os = "none", feature = "storage-test"))]
        if command == "diskteste" {
            write("Teste AHCI em disco QEMU descartável...\r\n");
            match unsafe { ahci::disposable_disk_test() } {
                Ok(()) => {
                    write("AHCI: IDENTIFY, WRITE, READ e erro do controlador verificados.\r\n")
                }
                Err(error) => {
                    record_error(error);
                    write("AHCI: teste falhou: ");
                    write(error.message());
                    write("\r\n");
                }
            }
            continue;
        }
        #[cfg(all(target_os = "none", feature = "storage-test"))]
        if command == "fs-teste" {
            write("Formatando volume Hermione no disco QEMU descartavel...\r\n");
            match unsafe { ahci::disposable_filesystem_test() } {
                Ok(()) => write(
                    "Hermione: formatacao, montagem, escrita e leitura persistente passaram.\r\n",
                ),
                Err(error) => {
                    record_error(error);
                    write("Hermione: teste falhou: ");
                    write(error.message());
                    write("\r\n");
                }
            }
            continue;
        }
        #[cfg(feature = "crash-test")]
        match command {
            "panic-teste" => panic!("panic intencional para teste do capturador"),
            "fault-teste" => unsafe { asm!("ud2", options(nomem, nostack)) },
            "hang-teste" => loop {
                core::hint::spin_loop()
            },
            "doublefault-teste" => unsafe {
                asm!("mov rsp, 0", "ud2", options(nomem, nostack, noreturn))
            },
            _ => {}
        }
        match parse_command(command) {
            Command::Help => write("Comandos: ajuda, sobre, usuario, permissoes, erros, limpar, parar.\r\n"),
            Command::About => {
                write("carlIO OS | Criado por João G.R.Santos\r\n");
                write("Sistema operacional experimental x86_64 escrito em Rust.\r\n");
            }
            Command::User => write("usuário: núcleo do sistema (nível de privilégio 0)\r\n"),
            Command::Permissions => write("Política demonstrativa: convidado pode consultar o estado; alteração do sistema é negada.\r\nAviso: os processos ainda não estão isolados; o núcleo inteiro executa no nível 0.\r\n"),
            Command::Errors => report_errors(),
            Command::Clear => write("\x1b[2J\x1b[H"),
            Command::Halt => {
                write("Processador parado.\r\n");
                loop { unsafe { asm!("cli; hlt", options(nomem, nostack)); } }
            }
            Command::Unknown => write("Comando desconhecido. Digite 'ajuda'.\r\n"),
        }
    }
}

fn startup_screen() {
    write("\x1b[2J\x1b[H");
    write("+----------------------------------------------------+\r\n");
    write("| carlIO OS | Criado por João G.R.Santos             |\r\n");
    write("| Sistema operacional experimental x86_64            |\r\n");
    write("+----------------------------------------------------+\r\n");
    write("Núcleo Rust · inicialização serial\r\n\r\n");
}

fn serial_init() {
    unsafe {
        out(COM1 + 1, 0x00);
        out(COM1 + 3, 0x80);
        out(COM1, 0x03);
        out(COM1 + 1, 0x00);
        out(COM1 + 3, 0x03);
        out(COM1 + 2, 0xc7);
        out(COM1 + 4, 0x0b);
    }
}

fn serial_write(byte: u8) -> bool {
    for iteration in 0..SERIAL_POLL_LIMIT {
        if iteration & 0x0fff == 0 {
            #[cfg(target_os = "none")]
            idt::heartbeat();
        }
        if unsafe { input(COM1 + 5) } & 0x20 != 0 {
            unsafe {
                out(COM1, byte);
            }
            return true;
        }
        core::hint::spin_loop();
    }
    record_error(KernelError::SerialWriteTimeout);
    false
}

fn serial_read() -> Result<u8, KernelError> {
    for iteration in 0..SERIAL_POLL_LIMIT {
        if iteration & 0x0fff == 0 {
            #[cfg(target_os = "none")]
            idt::heartbeat();
        }
        if unsafe { input(COM1 + 5) } & 1 != 0 {
            return Ok(unsafe { input(COM1) });
        }
        core::hint::spin_loop();
    }
    record_error(KernelError::SerialReadTimeout);
    Err(KernelError::SerialReadTimeout)
}

fn write(text: &str) {
    for byte in text.bytes() {
        if !serial_write(byte) {
            break;
        }
    }
}

fn record_error(error: KernelError) {
    // O kernel ainda executa somente na CPU de boot; o logger será protegido
    // por lock/atomics quando SMP e interrupções forem habilitados.
    unsafe {
        core::ptr::addr_of_mut!(ERROR_LOG)
            .as_mut()
            .unwrap()
            .record(error);
    }
}

fn report_errors() {
    let log = unsafe { core::ptr::addr_of!(ERROR_LOG).read() };
    write("Erros registrados: ");
    write_u32(log.total());
    write("\r\n");
    for index in 0..log.len() {
        if let Some(error) = log.recent(index) {
            write("- ");
            write(error.message());
            write("\r\n");
        }
    }
}

#[cfg(target_os = "none")]
fn report_memory_map() {
    let hhdm = unsafe { core::ptr::addr_of!(HHDM_REQUEST.response).read_volatile() };
    let map = unsafe { core::ptr::addr_of!(MEMMAP_REQUEST.response).read_volatile() };
    let kernel = unsafe { core::ptr::addr_of!(KERNEL_ADDRESS_REQUEST.response).read_volatile() };
    if hhdm.is_null() || map.is_null() || kernel.is_null() {
        record_error(KernelError::BootInfoMissing);
        write(
            "Aviso: mapa/HHDM/endereço do kernel indisponível; drivers DMA/MMIO desativados.\r\n",
        );
        return;
    }
    let offset = unsafe { (*hhdm).offset };
    let entry_count = unsafe { (*map).entry_count.min(4096) };
    let entries = unsafe { (*map).entries };
    if entries.is_null() {
        record_error(KernelError::BootInfoMissing);
        write("Aviso: lista do mapa de memória inválida; drivers DMA/MMIO desativados.\r\n");
        return;
    }
    let mut usable_bytes = 0u64;
    let mut usable_regions = 0u32;
    for index in 0..entry_count as usize {
        let entry = unsafe { entries.add(index).read_volatile() };
        if entry.is_null() {
            continue;
        }
        let entry = unsafe { &*entry };
        if entry.kind == 0 {
            usable_regions = usable_regions.saturating_add(1);
            usable_bytes = usable_bytes.saturating_add(entry.length);
        }
    }
    unsafe {
        core::ptr::addr_of_mut!(HHDM_OFFSET).write_volatile(offset);
        core::ptr::addr_of_mut!(KERNEL_PHYSICAL_BASE).write_volatile((*kernel).physical_base);
        core::ptr::addr_of_mut!(KERNEL_VIRTUAL_BASE).write_volatile((*kernel).virtual_base);
    }
    write("Mapa de memória Limine: regiões usáveis=");
    write_u32(usable_regions);
    write(", RAM usável MiB=");
    write_u32((usable_bytes / (1024 * 1024)).min(u32::MAX as u64) as u32);
    write("; HHDM e bases físico/virtual do kernel disponíveis.\r\n");
}

fn write_u32(mut number: u32) {
    let mut digits = [0u8; 10];
    let mut len = 0;
    loop {
        digits[len] = b'0' + (number % 10) as u8;
        len += 1;
        number /= 10;
        if number == 0 {
            break;
        }
    }
    while len > 0 {
        len -= 1;
        if !serial_write(digits[len]) {
            return;
        }
    }
}

#[cfg(target_os = "none")]
fn write_hex_u64(value: u64) {
    for shift in (0..16).rev() {
        write_hex_nibble(((value >> (shift * 4)) & 0xf) as u8);
    }
}

#[cfg(target_os = "none")]
fn write_u64(value: u64) {
    let mut digits = [0u8; 20];
    let mut len = 0;
    let mut number = value;
    loop {
        digits[len] = b'0' + (number % 10) as u8;
        len += 1;
        number /= 10;
        if number == 0 {
            break;
        }
    }
    while len > 0 {
        len -= 1;
        if !serial_write(digits[len]) {
            return;
        }
    }
}

#[cfg(target_os = "none")]
fn enumerate_pci() {
    write("Inventário PCI (somente leitura):\r\n");
    let found = pci_device::enumerate(report_pci_function);
    write("Controladores relevantes encontrados: ");
    write_u32(found as u32);
    write("\r\n");
}

#[cfg(target_os = "none")]
fn report_pci_function(function: PciFunction) {
    write("PCI ");
    write_hex_byte(function.bus);
    write(":");
    write_hex_byte(function.device);
    write(".");
    write_hex_nibble(function.function);
    write(" ");
    write_hex_word(function.vendor_id);
    write(":");
    write_hex_word(function.device_id);
    if function.is_sata_ahci() {
        write(" SATA/AHCI");
        match function.ahci_model() {
            Some(carlio_os::pci::AhciModel::IntelRstLocal) => {
                write(" (Intel RST local: reconhecido; I/O pendente)")
            }
            Some(carlio_os::pci::AhciModel::IntelNas) => {
                write(" (Intel NAS: reconhecido; I/O pendente)")
            }
            Some(carlio_os::pci::AhciModel::IntelQemu) => {
                write(" (QEMU: reconhecido; driver AHCI experimental)")
            }
            Some(carlio_os::pci::AhciModel::Unsupported) => {
                write(" (ID não suportado; sem acesso a disco)")
            }
            None => write(" (interface AHCI incompatível; sem acesso a disco)"),
        }
    } else if function.is_usb() {
        write(" USB");
    } else if function.is_network() {
        write(" rede");
    } else if function.is_video() {
        write(" vídeo");
    }
    write("\r\n");
    if function.ahci_model() == Some(carlio_os::pci::AhciModel::IntelQemu) {
        let hhdm = unsafe { core::ptr::addr_of!(HHDM_OFFSET).read_volatile() };
        let physical = unsafe { core::ptr::addr_of!(KERNEL_PHYSICAL_BASE).read_volatile() };
        let virtual_base = unsafe { core::ptr::addr_of!(KERNEL_VIRTUAL_BASE).read_volatile() };
        if hhdm != 0 && virtual_base != 0 {
            match unsafe { ahci::initialize(function, hhdm, physical, virtual_base) } {
                Ok(sectors) => {
                    write("AHCI QEMU: dispositivo identificado, setores=");
                    write_u64(sectors);
                    write(". Escrita desativada fora do teste dedicado.\r\n");
                }
                Err(error) => {
                    write("AHCI QEMU indisponível: ");
                    write(error.message());
                    write(".\r\n");
                }
            }
        } else {
            write("AHCI QEMU não iniciado: mapa DMA indisponível.\r\n");
        }
    }
}

#[cfg(target_os = "none")]
fn write_hex_byte(value: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    serial_write(HEX[(value >> 4) as usize]);
    serial_write(HEX[(value & 0x0f) as usize]);
}

#[cfg(target_os = "none")]
fn write_hex_nibble(value: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    serial_write(HEX[(value & 0x0f) as usize]);
}

#[cfg(target_os = "none")]
fn write_hex_word(value: u16) {
    write_hex_byte((value >> 8) as u8);
    write_hex_byte(value as u8);
}

unsafe fn out(port: u16, value: u8) {
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
    }
}

unsafe fn input(port: u16) -> u8 {
    let value: u8;
    unsafe {
        asm!("in al, dx", in("dx") port, out("al") value, options(nomem, nostack, preserves_flags));
    }
    value
}

#[cfg(target_os = "none")]
struct PanicSerial;

#[cfg(target_os = "none")]
impl FmtWrite for PanicSerial {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for byte in text.bytes() {
            if !serial_write(byte) {
                return Err(fmt::Error);
            }
        }
        Ok(())
    }
}

#[panic_handler]
#[cfg(target_os = "none")]
fn panic(info: &PanicInfo) -> ! {
    record_error(KernelError::KernelPanic);
    write("\r\n=== CAPTURA DE PANIC ===\r\n");
    write("Causa: ");
    let _ = PanicSerial.write_fmt(format_args!("{}\r\n", info.message()));
    if let Some(location) = info.location() {
        write("Local: ");
        write(location.file());
        write(":");
        write_u32(location.line());
        write(":");
        write_u32(location.column());
        write("\r\n");
    } else {
        write("Local: indisponível\r\n");
    }
    write("Registradores do ponto de panic indisponíveis; faults de CPU usam a captura IDT.\r\n");
    write("Captura somente serial e volátil; reiniciar perde este registro.\r\n");
    write("Modo de recuperação serial ativo; digite Enter para consultar erros.\r\n");
    loop {
        match serial_read() {
            Ok(byte) if byte == b'\r' || byte == b'\n' => {
                report_errors();
                write("Modo de recuperação ativo; núcleo sem operações de dispositivo.\r\n");
            }
            Ok(_) => {}
            Err(_) => core::hint::spin_loop(),
        }
    }
}
