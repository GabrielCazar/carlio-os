//! Argon2id para verificação PHC e derivação explícita de material de chave.
//! O chamador fornece o workspace para manter o uso de memória limitado em
//! ambientes `no_std` sem alocador.

use argon2::{
    password_hash::PasswordHash as ParsedHash, Algorithm, Argon2, Block, Params, Version,
};
use zeroize::Zeroize;

const MAX_PHC_BYTES: usize = 128;
const MAX_PASSES: u32 = 10;
const MAX_LANES: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyDerivationError {
    InvalidLimits,
    Argon2(argon2::Error),
}

/// Deriva bytes com Argon2id v1.3. `memory` contém blocos de 1 KiB e limita
/// diretamente o consumo; `salt` deve ser único por segredo e ter >= 8 bytes.
pub fn derive_key_into(
    password: &[u8],
    salt: &[u8],
    passes: u32,
    lanes: u32,
    memory: &mut [Block],
    output: &mut [u8],
) -> Result<(), KeyDerivationError> {
    clear_memory(memory);
    if memory.is_empty() || passes == 0 || passes > MAX_PASSES || lanes == 0 || lanes > MAX_LANES {
        return Err(KeyDerivationError::InvalidLimits);
    }
    let params = Params::new(memory.len() as u32, passes, lanes, Some(output.len()))
        .map_err(KeyDerivationError::Argon2)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let result = argon.hash_password_into_with_memory(password, salt, output, &mut *memory);
    clear_memory(memory);
    result.map_err(KeyDerivationError::Argon2)
}

/// Verificador PHC Argon2id com workspace exclusivo fornecido pelo chamador.
/// O tipo é mutável para deixar explícito que autenticações concorrentes não
/// podem compartilhar os mesmos blocos sem sincronização.
pub struct Argon2idVerifier<'a> {
    memory: &'a mut [Block],
}

impl<'a> Argon2idVerifier<'a> {
    pub fn new(memory: &'a mut [Block]) -> Self {
        Self { memory }
    }
}

impl crate::userland::accounts::PasswordHashVerifier for Argon2idVerifier<'_> {
    fn verify(&mut self, encoded_hash: &[u8], password: &[u8]) -> bool {
        verify_phc(encoded_hash, password, self.memory)
    }
}

/// Faz a verificação sem heap. Hashes com memória/passos acima dos limites ou
/// maiores que o workspace entregue são recusados antes de executar a KDF.
pub fn verify_phc(encoded_hash: &[u8], password: &[u8], memory: &mut [Block]) -> bool {
    clear_memory(memory);
    if encoded_hash.len() > MAX_PHC_BYTES || memory.is_empty() {
        return false;
    }
    let Ok(encoded) = core::str::from_utf8(encoded_hash) else {
        return false;
    };
    let Ok(parsed) = ParsedHash::new(encoded) else {
        return false;
    };
    if parsed.algorithm.as_str() != "argon2id" || parsed.version != Some(19) {
        return false;
    }
    let (Some(salt), Some(expected)) = (parsed.salt, parsed.hash) else {
        return false;
    };
    let Some(memory_kib) = parsed.params.get_decimal("m") else {
        return false;
    };
    let Some(passes) = parsed.params.get_decimal("t") else {
        return false;
    };
    let Some(lanes) = parsed.params.get_decimal("p") else {
        return false;
    };
    if memory_kib == 0
        || memory_kib as usize > memory.len()
        || passes == 0
        || passes > MAX_PASSES
        || lanes == 0
        || lanes > MAX_LANES
    {
        return false;
    }
    let Ok(params) = Params::new(memory_kib, passes, lanes, Some(expected.len())) else {
        return false;
    };
    let mut salt_bytes = [0u8; 64];
    let Ok(salt_bytes) = salt.decode_b64(&mut salt_bytes) else {
        return false;
    };
    let mut actual = [0u8; 64];
    let computed =
        (|| {
            let actual = &mut actual[..expected.len()];
            Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
                .hash_password_into_with_memory(password, salt_bytes, actual, &mut *memory)?;
            Ok::<_, argon2::Error>(actual)
        })();
    clear_memory(memory);
    let Ok(actual) = computed else {
        return false;
    };
    let Ok(actual) = argon2::password_hash::Output::new(actual) else {
        return false;
    };
    actual == expected
}

fn clear_memory(memory: &mut [Block]) {
    for block in memory {
        block.zeroize();
    }
}
