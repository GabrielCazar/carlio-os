//! Serviço de Registro volátil e independente do transporte, para execução no convidado.
//!
//! Cada instância atende um único hive privado. O broker deve vincular a sessão
//! ao hive antes de encaminhar a mensagem. A mensagem não transporta identidade
//! nem permite selecionar o hive.

const VERSION: u8 = 1;
const HEADER_SIZE: usize = 5;
const NAME_LIMIT: usize = 96;
const VALUE_LIMIT: usize = 128;
const CAPACITY: usize = 16;

/// Limite de bytes de uma instrução v1, incluindo cabeçalho.
pub const MAX_REQUEST: usize = HEADER_SIZE + NAME_LIMIT + VALUE_LIMIT;

/// Limite de bytes retornados em uma consulta v1.
pub const MAX_VALUE: usize = VALUE_LIMIT;

/// Tipo de valor com o mesmo código numérico usado pelo Win32.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueType {
    Binary = 3,
    Dword = 4,
}

/// Falhas do serviço. O transporte converte estes resultados para sua ABI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistryError {
    InvalidRequest,
    NotFound,
    HiveFull,
    BufferTooSmall { required: usize },
}

/// Resultado de uma operação bem-sucedida.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
    Updated,
    Value { kind: ValueType, len: usize },
}

struct Entry {
    name: [u8; NAME_LIMIT],
    name_len: usize,
    path_len: usize,
    data: [u8; VALUE_LIMIT],
    data_len: usize,
    kind: ValueType,
}

/// Estado de um hive. O chamador serializa as operações na mesma instância.
pub struct Registry {
    entries: [Option<Entry>; CAPACITY],
}

impl Registry {
    /// Cria um hive vazio em memória.
    pub const fn new() -> Self {
        Self {
            entries: [const { None }; CAPACITY],
        }
    }

    /// Consome uma única instrução v1 e copia somente o valor lido para
    /// `output`. Em uma leitura bem-sucedida, somente os primeiros `len`
    /// bytes do destino são preenchidos. Um destino vazio consulta somente os
    /// metadados, incluindo o tamanho do valor.
    ///
    /// Cabeçalho: versão, operação (1 consulta, 2 gravação, 3 remoção), tamanho
    /// do caminho, tamanho do nome e tipo. Seguem caminho, nome e dados.
    /// Consultas e remoções usam tipo zero e corpo sem dados.
    pub fn execute(&mut self, request: &[u8], output: &mut [u8]) -> Result<Reply, RegistryError> {
        let [VERSION, opcode, path_len, value_name_len, kind] = *request
            .first_chunk::<HEADER_SIZE>()
            .ok_or(RegistryError::InvalidRequest)?
        else {
            return Err(RegistryError::InvalidRequest);
        };
        let path_len = usize::from(path_len);
        let name_len = path_len + usize::from(value_name_len);
        let (name, data) = request[HEADER_SIZE..]
            .split_at_checked(name_len)
            .ok_or(RegistryError::InvalidRequest)?;
        if path_len == 0
            || name_len > NAME_LIMIT
            || name.iter().any(|byte| !(b' '..=b'~').contains(byte))
            || name[..path_len]
                .split(|&byte| byte == b'\\')
                .any(|part| part.is_empty())
        {
            return Err(RegistryError::InvalidRequest);
        }

        let existing = self.entries.iter().position(|slot| {
            slot.as_ref().is_some_and(|entry| {
                entry.path_len == path_len
                    && entry.name[..entry.name_len].eq_ignore_ascii_case(name)
            })
        });
        match opcode {
            1 if kind == 0 && data.is_empty() => {
                let entry = existing
                    .and_then(|index| self.entries[index].as_ref())
                    .ok_or(RegistryError::NotFound)?;
                if !output.is_empty() {
                    let dest =
                        output
                            .get_mut(..entry.data_len)
                            .ok_or(RegistryError::BufferTooSmall {
                                required: entry.data_len,
                            })?;
                    dest.copy_from_slice(&entry.data[..entry.data_len]);
                }
                Ok(Reply::Value {
                    kind: entry.kind,
                    len: entry.data_len,
                })
            }
            2 => {
                let kind = match kind {
                    3 if data.len() <= VALUE_LIMIT => ValueType::Binary,
                    4 if data.len() == 4 => ValueType::Dword,
                    _ => return Err(RegistryError::InvalidRequest),
                };
                let index = existing
                    .or_else(|| self.entries.iter().position(Option::is_none))
                    .ok_or(RegistryError::HiveFull)?;
                let mut entry = Entry {
                    name: [0; NAME_LIMIT],
                    name_len,
                    path_len,
                    data: [0; VALUE_LIMIT],
                    data_len: data.len(),
                    kind,
                };
                entry.name[..name_len].copy_from_slice(name);
                entry.data[..data.len()].copy_from_slice(data);
                self.entries[index] = Some(entry);
                Ok(Reply::Updated)
            }
            3 if kind == 0 && data.is_empty() => {
                let index = existing.ok_or(RegistryError::NotFound)?;
                self.entries[index] = None;
                Ok(Reply::Updated)
            }
            _ => Err(RegistryError::InvalidRequest),
        }
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_removes_values_without_case_sensitivity() {
        let mut registry = Registry::new();
        let write = b"\x01\x02\x03\x01\x04AppX\x2a\0\0\0";
        let read = b"\x01\x01\x03\x01\0aPPx";
        assert_eq!(registry.execute(write, &mut []), Ok(Reply::Updated));
        let metadata = Ok(Reply::Value {
            kind: ValueType::Dword,
            len: 4,
        });
        assert_eq!(registry.execute(read, &mut []), metadata);
        let mut short = [0xff; 3];
        assert_eq!(
            registry.execute(read, &mut short),
            Err(RegistryError::BufferTooSmall { required: 4 }),
        );
        assert_eq!(short, [0xff; 3]);
        let mut output = [0xff; 5];
        assert_eq!(registry.execute(read, &mut output), metadata);
        assert_eq!(output, [42, 0, 0, 0, 0xff]);

        let mut other_hive = Registry::new();
        assert_eq!(
            other_hive.execute(read, &mut []),
            Err(RegistryError::NotFound)
        );
        let delete = b"\x01\x03\x03\x01\0appX";
        assert_eq!(registry.execute(delete, &mut []), Ok(Reply::Updated));
        assert_eq!(
            registry.execute(read, &mut []),
            Err(RegistryError::NotFound)
        );
    }

    #[test]
    fn rejects_invalid_requests_without_altering_the_hive() {
        let mut registry = Registry::new();
        let write = b"\x01\x02\x03\x01\x03AppXoriginal";
        assert_eq!(registry.execute(write, &mut []), Ok(Reply::Updated));
        for invalid in [
            &b"\x02\x02\x03\x01\x03AppXbad"[..],
            b"\x01\x02\x03\x01\x04AppXbad",
            b"\x01\x03\x03\x01\0AppXbad",
            b"\x01\x02\x03\x01\x03App",
            b"\x01\x02\x03\x01\x03App\0bad",
        ] {
            assert_eq!(
                registry.execute(invalid, &mut []),
                Err(RegistryError::InvalidRequest)
            );
        }
        let mut output = [0; 8];
        let read = b"\x01\x01\x03\x01\0AppX";
        assert_eq!(
            registry.execute(read, &mut output),
            Ok(Reply::Value {
                kind: ValueType::Binary,
                len: 8
            }),
        );
        assert_eq!(&output, b"original");
    }

    #[test]
    fn bounds_hive_size_but_allows_replacement() {
        let mut registry = Registry::new();
        let mut write = *b"\x01\x02\x01\x01\x03KAdata";
        for index in 0..CAPACITY as u8 {
            write[6] = b'A' + index;
            assert_eq!(registry.execute(&write, &mut []), Ok(Reply::Updated));
        }
        write[6] = b'Z';
        assert_eq!(
            registry.execute(&write, &mut []),
            Err(RegistryError::HiveFull)
        );
        write[6] = b'A';
        assert_eq!(registry.execute(&write, &mut []), Ok(Reply::Updated));
    }
}
