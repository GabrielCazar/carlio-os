//! Valida a pilha de Registro através de uma VM QEMU real.
#![cfg(target_os = "linux")]

use carlio_os::userland::windows::{
    registry_client::{RegistryAccess, RegistrySession, Win32Error},
    registry_vm::RegistryVm,
};
use std::path::Path;

#[test]
#[ignore = "requer QEMU e imagens de convidado preparadas"]
fn registry_roundtrip_in_qemu() {
    let kernel = std::env::var_os("CARLIO_REGISTRY_KERNEL").unwrap();
    let initramfs = std::env::var_os("CARLIO_REGISTRY_INITRAMFS").unwrap();
    let boot = || {
        RegistryVm::boot(
            Path::new("qemu-system-x86_64"),
            Path::new(&kernel),
            Path::new(&initramfs),
        )
        .unwrap()
    };
    let mut session = RegistrySession::new(boot(), 1000, RegistryAccess::ReadWrite);
    let key: Vec<_> = "Software\\Carlio".encode_utf16().collect();
    let name: Vec<_> = "Enabled".encode_utf16().collect();
    let mut size = 0;
    assert_eq!(
        session.query_value(1000, &key, &name, None, &mut size),
        Err(Win32Error::FileNotFound)
    );
    assert_eq!(
        session.set_value(1001, &key, &name, 4, &[42, 0, 0, 0]),
        Err(Win32Error::AccessDenied)
    );
    session
        .set_value(1000, &key, &name, 4, &[42, 0, 0, 0])
        .unwrap();
    assert_eq!(
        session.query_value(1000, &key, &name, None, &mut size),
        Ok(4)
    );
    assert_eq!(size, 4);
    let mut output = [0xEE; 4];
    size = 1;
    assert_eq!(
        session.query_value(1000, &key, &name, Some(&mut output), &mut size),
        Err(Win32Error::MoreData)
    );
    assert_eq!(output, [0xEE; 4]);
    assert_eq!(
        session.query_value(1000, &key, &name, Some(&mut output), &mut size),
        Ok(4)
    );
    assert_eq!(output, [42, 0, 0, 0]);

    let mut other = RegistrySession::new(boot(), 1001, RegistryAccess::QueryOnly);
    assert_eq!(
        other.query_value(1001, &key, &name, None, &mut size),
        Err(Win32Error::FileNotFound)
    );
    assert_eq!(
        other.delete_value(1001, &key, &name),
        Err(Win32Error::AccessDenied)
    );
    session.delete_value(1000, &key, &name).unwrap();
    assert_eq!(
        session.query_value(1000, &key, &name, None, &mut size),
        Err(Win32Error::FileNotFound)
    );
}
