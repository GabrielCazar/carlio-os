//! Broker de sessão e semântica de valores Win32 sobre um canal privado.

use super::registry::{MAX_REQUEST, MAX_VALUE};

/// Canal exclusivo de uma VM, com prazo e encerramento a cargo do transporte.
/// Uma falha pode ocorrer depois que o convidado aplicou a operação.
pub trait RegistryChannel {
    /// Envia um corpo v1 e recebe cabeçalho e conteúdo em um buffer limitado.
    fn exchange(&mut self, request: &[u8], response: &mut [u8; 4 + MAX_VALUE]) -> Option<usize>;
}

/// Direitos concedidos pelo supervisor ao abrir a sessão.
#[derive(Clone, Copy)]
pub enum RegistryAccess {
    QueryOnly,
    ReadWrite,
}

/// Códigos LSTATUS usados pela camada de compatibilidade.
#[repr(u32)]
#[derive(Debug, PartialEq, Eq)]
pub enum Win32Error {
    FileNotFound = 2,
    AccessDenied = 5,
    InvalidHandle = 6,
    InvalidParameter = 87,
    DiskFull = 112,
    MoreData = 234,
    RegistryIoFailed = 1016,
}

/// Broker Win32 de uma sessão. O canal e o hive são selecionados pelo
/// supervisor. O chamador fornece o UID vigente a partir das credenciais
/// confiáveis do processo em cada chamada.
pub struct RegistrySession<C> {
    channel: Option<C>,
    owner: u32,
    access: RegistryAccess,
}

impl<C: RegistryChannel> RegistrySession<C> {
    /// Vincula o canal ao principal autenticado e aos direitos concedidos.
    pub fn new(channel: C, owner: u32, access: RegistryAccess) -> Self {
        Self {
            channel: Some(channel),
            owner,
            access,
        }
    }

    /// Semântica inicial de RegQueryValueExW. Atualiza o tamanho necessário
    /// em sucesso ou MoreData. Um destino ausente consulta só os metadados.
    pub fn query_value(
        &mut self,
        uid: u32,
        key: &[u16],
        name: &[u16],
        output: Option<&mut [u8]>,
        size: &mut usize,
    ) -> Result<u32, Win32Error> {
        let (response, len) = self.call(uid, [1, 1, 0, 0, 0], key, name, &[])?;
        let capacity = *size;
        *size = len;
        if let Some(output) = output {
            if capacity < len || output.len() < len {
                return Err(Win32Error::MoreData);
            }
            output[..len].copy_from_slice(&response[4..4 + len]);
        }
        Ok(u32::from(response[1]))
    }

    /// Semântica inicial de RegSetValueExW para REG_BINARY e REG_DWORD.
    pub fn set_value(
        &mut self,
        uid: u32,
        key: &[u16],
        name: &[u16],
        kind: u32,
        data: &[u8],
    ) -> Result<(), Win32Error> {
        let kind = match kind {
            3 if data.len() <= MAX_VALUE => 3,
            4 if data.len() == 4 => 4,
            _ => return Err(Win32Error::InvalidParameter),
        };
        self.call(uid, [1, 2, 0, 0, kind], key, name, data)?;
        Ok(())
    }

    /// Semântica inicial de RegDeleteValueW.
    pub fn delete_value(&mut self, uid: u32, key: &[u16], name: &[u16]) -> Result<(), Win32Error> {
        self.call(uid, [1, 3, 0, 0, 0], key, name, &[])?;
        Ok(())
    }

    fn call(
        &mut self,
        uid: u32,
        mut header: [u8; 5],
        key: &[u16],
        name: &[u16],
        data: &[u8],
    ) -> Result<([u8; 4 + MAX_VALUE], usize), Win32Error> {
        let op = header[1];
        if uid != self.owner || (op != 1 && matches!(self.access, RegistryAccess::QueryOnly)) {
            return Err(Win32Error::AccessDenied);
        }
        let channel = self.channel.as_mut().ok_or(Win32Error::InvalidHandle)?;
        if key.is_empty() || key.len().saturating_add(name.len()) > MAX_REQUEST - 5 - MAX_VALUE {
            return Err(Win32Error::InvalidParameter);
        }
        header[2] = key.len() as u8;
        header[3] = name.len() as u8;
        let mut request = [0; MAX_REQUEST];
        request[..5].copy_from_slice(&header);
        for (dest, &unit) in request[5..].iter_mut().zip(key.iter().chain(name)) {
            if !(0x20..=0x7e).contains(&unit) {
                return Err(Win32Error::InvalidParameter);
            }
            *dest = unit as u8;
        }
        let data_start = 5 + key.len() + name.len();
        let end = data_start + data.len();
        request[data_start..end].copy_from_slice(data);

        let mut response = [0; 4 + MAX_VALUE];
        let received = channel.exchange(&request[..end], &mut response);
        let [status, kind, low, high] = response[..4].try_into().unwrap();
        let len = usize::from(u16::from_le_bytes([low, high]));
        if len <= MAX_VALUE && received == Some(4 + len) {
            match (status, kind, len) {
                (0, 3, _) | (0, 4, 4) if op == 1 => return Ok((response, len)),
                (0, 0, 0) if op != 1 => return Ok((response, 0)),
                (1, 0, 0) => return Err(Win32Error::InvalidParameter),
                (2, 0, 0) if op != 2 => return Err(Win32Error::FileNotFound),
                (3, 0, 0) if op == 2 => return Err(Win32Error::DiskFull),
                _ => {}
            }
        }
        self.channel = None;
        Err(Win32Error::RegistryIoFailed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;

    struct Malformed<'a>(&'a Cell<bool>);

    impl Drop for Malformed<'_> {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }

    impl RegistryChannel for Malformed<'_> {
        fn exchange(&mut self, _: &[u8], response: &mut [u8; 4 + MAX_VALUE]) -> Option<usize> {
            response[..4].copy_from_slice(&[0, 4, 1, 0]);
            Some(5)
        }
    }

    #[test]
    fn rejects_malformed_dword_and_discards_the_channel() {
        let closed = Cell::new(false);
        let mut session = RegistrySession::new(Malformed(&closed), 1000, RegistryAccess::ReadWrite);
        let mut output = [0xEE; 4];
        let mut size = output.len();
        assert_eq!(
            session.query_value(1000, &[b'K' as u16], &[], Some(&mut output), &mut size),
            Err(Win32Error::RegistryIoFailed)
        );
        assert!(closed.get());
        assert_eq!(output, [0xEE; 4]);
        assert_eq!(size, 4);
        assert_eq!(
            session.query_value(1000, &[b'K' as u16], &[], None, &mut size),
            Err(Win32Error::InvalidHandle)
        );
    }
}
