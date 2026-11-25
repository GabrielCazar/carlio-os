use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=linker.ld");
    println!("cargo:rerun-if-changed=limine.conf");
    println!("cargo:rerun-if-env-changed=CARLIO_LOCKDOWN_FLOOR");
    let floor = env::var("CARLIO_LOCKDOWN_FLOOR").unwrap_or_else(|_| "none".into());
    assert!(matches!(
        floor.as_str(),
        "none" | "integrity" | "confidentiality"
    ));
    println!("cargo:rustc-env=CARLIO_LOCKDOWN_FLOOR={floor}");
    let names = [
        "CARLIO_DIRCACHE_EXECUTABLE",
        "CARLIO_DIRCACHE_SIGNATURE",
        "CARLIO_DIRCACHE_PUBLIC_KEY",
    ];
    let inputs = names.map(|name| {
        println!("cargo:rerun-if-env-changed={name}");
        env::var_os(name).map(PathBuf::from)
    });
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let executable = output.join("dircache-backend.elf");
    match inputs {
        [None, None, None] => fs::write(executable, []).unwrap(),
        [Some(binary), Some(signature), Some(public_key)] => {
            for path in [&binary, &signature, &public_key] {
                println!("cargo:rerun-if-changed={}", path.display());
            }
            let signature_copy = output.join("dircache-backend.sig");
            let key_copy = output.join("dircache-public.pem");
            fs::copy(binary, &executable).unwrap();
            fs::copy(signature, &signature_copy).unwrap();
            fs::copy(public_key, &key_copy).unwrap();
            let status = Command::new("openssl")
                .args(["pkeyutl", "-verify", "-pubin", "-rawin", "-inkey"])
                .arg(key_copy)
                .arg("-sigfile")
                .arg(signature_copy)
                .arg("-in")
                .arg(executable)
                .status()
                .expect("openssl is required for signed dircache builds");
            assert!(status.success(), "dircache executable signature rejected");
        }
        _ => panic!("configure the executable, signature and public key together"),
    }
}
