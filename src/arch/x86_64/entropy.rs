//! Boot secrets from the x86 hardware RNG, with bounded retries on underflow.

use zeroize::Zeroizing;

pub fn swap_credentials() -> Option<(Zeroizing<[u8; 32]>, [u8; 16])> {
    let mut key = Zeroizing::new([0; 32]);
    let mut session = [0; 16];
    fill_random(&mut *key)?;
    fill_random(&mut session)?;
    Some((key, session))
}

pub fn fill_random(bytes: &mut [u8]) -> Option<()> {
    let features = core::arch::x86_64::__cpuid(1);
    if features.ecx & (1 << 30) == 0 {
        return None;
    }
    for chunk in bytes.chunks_mut(8) {
        let mut word = Zeroizing::new(0u64);
        let mut succeeded = false;
        for _ in 0..10 {
            // SAFETY: CPUID above guarantees RDRAND is supported.
            if unsafe { core::arch::x86_64::_rdrand64_step(&mut word) } == 1 {
                succeeded = true;
                break;
            }
            core::hint::spin_loop();
        }
        if !succeeded {
            return None;
        }
        chunk.copy_from_slice(&word.to_le_bytes()[..chunk.len()]);
    }
    Some(())
}
