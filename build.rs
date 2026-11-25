use std::env;

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
}
