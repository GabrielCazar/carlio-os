//! Validação bounded de imagens PE32+ x86_64 para a futura userland Windows.
//!
//! A preparação da imagem escreve em um buffer comum. A execução exige
//! integração com o espaço de usuário e a camada Win32/NT.

pub const PE_MACHINE_AMD64: u16 = 0x8664;
pub const PE_OPTIONAL_MAGIC_64: u16 = 0x20b;
pub const PE_SECTION_EXECUTE: u32 = 0x2000_0000;
pub const PE_SECTION_LIMIT: usize = 96;
const DOS_HEADER_MIN: usize = 64;
const COFF_HEADER_SIZE: usize = 20;
const OPTIONAL_HEADER_MIN: usize = 112;
const SECTION_HEADER_SIZE: usize = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeError {
    Truncated,
    InvalidDosHeader,
    InvalidSignature,
    UnsupportedMachine,
    InvalidHeader,
    TooManySections,
    InvalidSection,
    InvalidEntryPoint,
    ImageBufferTooSmall,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PeSection {
    pub virtual_address: u32,
    pub virtual_size: u32,
    pub raw_offset: u32,
    pub raw_size: u32,
    pub characteristics: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeImage<const N: usize> {
    pub entry_rva: u32,
    pub image_base: u64,
    pub image_size: u32,
    pub headers_size: u32,
    sections: [PeSection; N],
    section_count: usize,
}

impl<const N: usize> PeImage<N> {
    /// Prepara o layout virtual no endereço preferido. A saída só é alterada
    /// após validar todo o arquivo e o tamanho do destino.
    pub fn materialize(file: &[u8], output: &mut [u8]) -> Result<Self, PeError> {
        let image = Self::parse(file)?;
        let output = output
            .get_mut(..image.image_size as usize)
            .ok_or(PeError::ImageBufferTooSmall)?;
        output.fill(0);
        let headers = image.headers_size as usize;
        output[..headers].copy_from_slice(&file[..headers]);
        for section in image.sections() {
            if section.raw_size == 0 {
                continue;
            }
            let start = section.virtual_address as usize;
            let raw = section.raw_offset as usize;
            let size = section.raw_size as usize;
            output[start..start + size].copy_from_slice(&file[raw..raw + size]);
        }
        Ok(image)
    }

    /// Valida PE32+ AMD64 sem alocar memória e conserva apenas metadados.
    /// Rejeita intervalos que estouram, seções fora do arquivo e entry point
    /// que não pertença a uma seção executável.
    pub fn parse(file: &[u8]) -> Result<Self, PeError> {
        if file.len() < DOS_HEADER_MIN {
            return Err(PeError::Truncated);
        }
        if &file[..2] != b"MZ" {
            return Err(PeError::InvalidDosHeader);
        }
        let pe_offset = read_u32(file, 0x3c)? as usize;
        let coff = pe_offset.checked_add(4).ok_or(PeError::InvalidHeader)?;
        let coff_end = coff
            .checked_add(COFF_HEADER_SIZE)
            .ok_or(PeError::InvalidHeader)?;
        if coff_end > file.len() {
            return Err(PeError::Truncated);
        }
        if file.get(pe_offset..coff) != Some(b"PE\0\0") {
            return Err(PeError::InvalidSignature);
        }
        if read_u16(file, coff)? != PE_MACHINE_AMD64 {
            return Err(PeError::UnsupportedMachine);
        }
        let section_count = read_u16(file, coff + 2)? as usize;
        if section_count == 0 || section_count > PE_SECTION_LIMIT || section_count > N {
            return Err(PeError::TooManySections);
        }
        let optional_size = read_u16(file, coff + 16)? as usize;
        if optional_size < OPTIONAL_HEADER_MIN {
            return Err(PeError::InvalidHeader);
        }
        let optional = coff_end;
        let optional_end = optional
            .checked_add(optional_size)
            .ok_or(PeError::InvalidHeader)?;
        if optional_end > file.len() {
            return Err(PeError::Truncated);
        }
        if read_u16(file, optional)? != PE_OPTIONAL_MAGIC_64 {
            return Err(PeError::InvalidHeader);
        }
        let entry_rva = read_u32(file, optional + 16)?;
        let image_base = read_u64(file, optional + 24)?;
        let section_alignment = read_u32(file, optional + 32)?;
        let file_alignment = read_u32(file, optional + 36)?;
        let image_size = read_u32(file, optional + 56)?;
        let headers_size = read_u32(file, optional + 60)?;
        if image_size == 0
            || headers_size == 0
            || headers_size as usize > file.len()
            || headers_size > image_size
            || section_alignment == 0
            || file_alignment == 0
        {
            return Err(PeError::InvalidHeader);
        }
        let table_size = section_count
            .checked_mul(SECTION_HEADER_SIZE)
            .ok_or(PeError::InvalidHeader)?;
        let table_end = optional_end
            .checked_add(table_size)
            .ok_or(PeError::InvalidHeader)?;
        if table_end > file.len() || table_end > headers_size as usize {
            return Err(PeError::Truncated);
        }
        let mut sections = [PeSection::default(); N];
        let mut entry_is_executable = false;
        for index in 0..section_count {
            let offset = optional_end + index * SECTION_HEADER_SIZE;
            let virtual_size = read_u32(file, offset + 8)?;
            let virtual_address = read_u32(file, offset + 12)?;
            let raw_size = read_u32(file, offset + 16)?;
            let raw_offset = read_u32(file, offset + 20)?;
            let characteristics = read_u32(file, offset + 36)?;
            let mapped_size = virtual_size.max(raw_size);
            let virtual_end = virtual_address
                .checked_add(mapped_size)
                .ok_or(PeError::InvalidSection)?;
            let raw_end = raw_offset
                .checked_add(raw_size)
                .ok_or(PeError::InvalidSection)?;
            if mapped_size == 0
                || virtual_address < headers_size
                || virtual_end > image_size
                || (raw_size != 0 && (raw_offset < headers_size || raw_end as usize > file.len()))
                || sections[..index].iter().any(|previous| {
                    let start = previous.virtual_address;
                    let end = start + previous.virtual_size.max(previous.raw_size);
                    virtual_address < end && start < virtual_end
                })
            {
                return Err(PeError::InvalidSection);
            }
            if entry_rva >= virtual_address
                && entry_rva < virtual_end
                && characteristics & PE_SECTION_EXECUTE != 0
            {
                entry_is_executable = true;
            }
            sections[index] = PeSection {
                virtual_address,
                virtual_size,
                raw_offset,
                raw_size,
                characteristics,
            };
        }
        if entry_rva == 0 || entry_rva >= image_size || !entry_is_executable {
            return Err(PeError::InvalidEntryPoint);
        }
        Ok(Self {
            entry_rva,
            image_base,
            image_size,
            headers_size,
            sections,
            section_count,
        })
    }

    pub fn sections(&self) -> &[PeSection] {
        &self.sections[..self.section_count]
    }
}

fn read_u16(input: &[u8], offset: usize) -> Result<u16, PeError> {
    let bytes = input
        .get(offset..offset.checked_add(2).ok_or(PeError::InvalidHeader)?)
        .ok_or(PeError::Truncated)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(input: &[u8], offset: usize) -> Result<u32, PeError> {
    let bytes = input
        .get(offset..offset.checked_add(4).ok_or(PeError::InvalidHeader)?)
        .ok_or(PeError::Truncated)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u64(input: &[u8], offset: usize) -> Result<u64, PeError> {
    let bytes = input
        .get(offset..offset.checked_add(8).ok_or(PeError::InvalidHeader)?)
        .ok_or(PeError::Truncated)?;
    Ok(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> [u8; 1024] {
        let mut b = [0u8; 1024];
        b[..2].copy_from_slice(b"MZ");
        b[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        let c = 0x84;
        b[c..c + 2].copy_from_slice(&PE_MACHINE_AMD64.to_le_bytes());
        b[c + 2..c + 4].copy_from_slice(&1u16.to_le_bytes());
        b[c + 16..c + 18].copy_from_slice(&240u16.to_le_bytes());
        let o = c + COFF_HEADER_SIZE;
        b[o..o + 2].copy_from_slice(&PE_OPTIONAL_MAGIC_64.to_le_bytes());
        b[o + 16..o + 20].copy_from_slice(&0x1000u32.to_le_bytes());
        b[o + 24..o + 32].copy_from_slice(&0x0001_4000_0000_u64.to_le_bytes());
        b[o + 32..o + 36].copy_from_slice(&0x1000u32.to_le_bytes());
        b[o + 36..o + 40].copy_from_slice(&0x200u32.to_le_bytes());
        b[o + 56..o + 60].copy_from_slice(&0x2000u32.to_le_bytes());
        b[o + 60..o + 64].copy_from_slice(&0x200u32.to_le_bytes());
        let s = o + 240;
        b[s + 8..s + 12].copy_from_slice(&0x100u32.to_le_bytes());
        b[s + 12..s + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        b[s + 16..s + 20].copy_from_slice(&0x200u32.to_le_bytes());
        b[s + 20..s + 24].copy_from_slice(&0x200u32.to_le_bytes());
        b[s + 36..s + 40].copy_from_slice(&PE_SECTION_EXECUTE.to_le_bytes());
        b
    }

    #[test]
    fn parses_bounded_amd64_pe32_plus_image() {
        let image = PeImage::<8>::parse(&fixture()).unwrap();
        assert_eq!(image.entry_rva, 0x1000);
        assert_eq!(image.image_base, 0x0001_4000_0000);
        assert_eq!(image.sections().len(), 1);
    }

    #[test]
    fn materializes_headers_data_and_zero_filled_virtual_tail() {
        let mut file = fixture();
        file[0x200] = 0x90;
        let section = 0x84 + COFF_HEADER_SIZE + 240;
        file[section + 8..section + 12].copy_from_slice(&0x400u32.to_le_bytes());
        let mut destination = [0xff; 0x2001];
        PeImage::<8>::materialize(&file, &mut destination).unwrap();
        assert_eq!(&destination[..0x200], &file[..0x200]);
        assert_eq!(destination[0x1000], 0x90);
        assert!(destination[0x1200..0x2000].iter().all(|byte| *byte == 0));
        assert_eq!(destination[0x2000], 0xff);
        let mut short = [0xff; 16];
        assert_eq!(
            PeImage::<8>::materialize(&file, &mut short),
            Err(PeError::ImageBufferTooSmall)
        );
        assert_eq!(short, [0xff; 16]);
    }

    #[test]
    fn rejects_overlapping_virtual_sections_before_writing() {
        let mut file = fixture();
        file[0x86..0x88].copy_from_slice(&2u16.to_le_bytes());
        let section = 0x84 + COFF_HEADER_SIZE + 240;
        file.copy_within(
            section..section + SECTION_HEADER_SIZE,
            section + SECTION_HEADER_SIZE,
        );
        let mut destination = [0xff; 0x2000];
        assert_eq!(
            PeImage::<8>::materialize(&file, &mut destination),
            Err(PeError::InvalidSection)
        );
        assert!(destination.iter().all(|byte| *byte == 0xff));
    }

    #[test]
    fn rejects_non_executable_entry_and_truncated_raw_section() {
        let mut bytes = fixture();
        let section = 0x84 + COFF_HEADER_SIZE + 240;
        bytes[section + 36..section + 40].copy_from_slice(&0x4000_0000u32.to_le_bytes());
        assert_eq!(PeImage::<8>::parse(&bytes), Err(PeError::InvalidEntryPoint));
        let mut bytes = fixture();
        bytes[section + 20..section + 24].copy_from_slice(&0xffff_ff00u32.to_le_bytes());
        assert_eq!(PeImage::<8>::parse(&bytes), Err(PeError::InvalidSection));
    }

    #[test]
    fn rejects_bad_signatures_machine_and_section_capacity() {
        let mut bytes = fixture();
        bytes[0x80] = b'X';
        assert_eq!(PeImage::<8>::parse(&bytes), Err(PeError::InvalidSignature));
        let mut bytes = fixture();
        bytes[0x84..0x86].copy_from_slice(&0x14cu16.to_le_bytes());
        assert_eq!(
            PeImage::<8>::parse(&bytes),
            Err(PeError::UnsupportedMachine)
        );
        assert_eq!(
            PeImage::<0>::parse(&fixture()),
            Err(PeError::TooManySections)
        );
    }
}
