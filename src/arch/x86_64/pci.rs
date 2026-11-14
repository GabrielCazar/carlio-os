//! Descoberta inicial PCI por Configuration Mechanism #1.
//! A rotina só lê configuração; não habilita BARs nem assume controle dos devices.

use carlio_os::pci::PciFunction;
use core::arch::asm;

const CONFIG_ADDRESS: u16 = 0x0cf8;
const CONFIG_DATA: u16 = 0x0cfc;

pub fn enumerate(mut report: impl FnMut(PciFunction)) -> usize {
    let mut found = 0;
    for bus in 0u8..=u8::MAX {
        for device in 0u8..32 {
            let id0 = read_config(bus, device, 0, 0);
            if id0 as u16 == 0xffff {
                continue;
            }
            let header = (read_config(bus, device, 0, 0x0c) >> 16) as u8;
            let functions = if header & 0x80 != 0 { 8 } else { 1 };
            for function in 0u8..functions {
                let identity = if function == 0 {
                    id0
                } else {
                    read_config(bus, device, function, 0)
                };
                let vendor_id = identity as u16;
                if vendor_id == 0xffff {
                    continue;
                }
                let class_info = read_config(bus, device, function, 0x08);
                let pci = PciFunction {
                    bus,
                    device,
                    function,
                    vendor_id,
                    device_id: (identity >> 16) as u16,
                    programming_interface: (class_info >> 8) as u8,
                    subclass: (class_info >> 16) as u8,
                    class: (class_info >> 24) as u8,
                };
                if pci.is_sata_ahci() || pci.is_video() || pci.is_network() || pci.is_usb() {
                    found += 1;
                    report(pci);
                }
            }
        }
    }
    found
}

fn read_config(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let address = 0x8000_0000u32
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xfc);
    unsafe {
        out32(CONFIG_ADDRESS, address);
        in32(CONFIG_DATA)
    }
}

unsafe fn out32(port: u16, value: u32) {
    unsafe {
        asm!("out dx, eax", in("dx") port, in("eax") value, options(nomem, nostack, preserves_flags));
    }
}

unsafe fn in32(port: u16) -> u32 {
    let value: u32;
    unsafe {
        asm!("in eax, dx", in("dx") port, out("eax") value, options(nomem, nostack, preserves_flags));
    }
    value
}
