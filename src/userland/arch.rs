//! Leitura bounded dos metadados `.PKGINFO` de pacotes Arch Linux.
//!
//! Não extrai tar/zstd nem instala arquivos. O parser host serve como primeiro
//! contrato do catálogo ALPM e recusa nomes/versões/dependências truncados.

pub const ARCH_DEPENDENCY_LIMIT: usize = 16;
pub const ARCH_LICENSE_LIMIT: usize = 8;
const FIELD_BYTES: usize = 96;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PkgInfoError {
    TooLarge,
    InvalidLine,
    MissingRequired,
    DuplicateRequired,
    InvalidValue,
    TooManyDependencies,
    TooManyLicenses,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchPackageInfo {
    name: [u8; FIELD_BYTES],
    version: [u8; FIELD_BYTES],
    architecture: [u8; 16],
    description: [u8; FIELD_BYTES],
    size: u64,
    build_date: Option<u64>,
    dependencies: [[u8; FIELD_BYTES]; ARCH_DEPENDENCY_LIMIT],
    dependency_count: usize,
    licenses: [[u8; 32]; ARCH_LICENSE_LIMIT],
    license_count: usize,
}

impl ArchPackageInfo {
    pub fn parse(input: &[u8]) -> Result<Self, PkgInfoError> {
        if input.len() > 64 * 1024 {
            return Err(PkgInfoError::TooLarge);
        }
        let text = core::str::from_utf8(input).map_err(|_| PkgInfoError::InvalidValue)?;
        let mut result = Self {
            name: [0; FIELD_BYTES],
            version: [0; FIELD_BYTES],
            architecture: [0; 16],
            description: [0; FIELD_BYTES],
            size: 0,
            build_date: None,
            dependencies: [[0; FIELD_BYTES]; ARCH_DEPENDENCY_LIMIT],
            dependency_count: 0,
            licenses: [[0; 32]; ARCH_LICENSE_LIMIT],
            license_count: 0,
        };
        let mut seen_name = false;
        let mut seen_version = false;
        let mut seen_arch = false;
        for raw_line in text.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = line.split_once('=').ok_or(PkgInfoError::InvalidLine)?;
            let key = key.trim();
            let value = value.trim();
            if value.is_empty() {
                return Err(PkgInfoError::InvalidValue);
            }
            match key {
                "pkgname" => {
                    if seen_name {
                        return Err(PkgInfoError::DuplicateRequired);
                    }
                    validate_field(value, true)?;
                    copy_field(&mut result.name, value)?;
                    seen_name = true;
                }
                "pkgver" => {
                    if seen_version {
                        return Err(PkgInfoError::DuplicateRequired);
                    }
                    validate_field(value, true)?;
                    copy_field(&mut result.version, value)?;
                    seen_version = true;
                }
                "arch" => {
                    if seen_arch {
                        return Err(PkgInfoError::DuplicateRequired);
                    }
                    if value != "x86_64" && value != "any" {
                        return Err(PkgInfoError::InvalidValue);
                    }
                    copy_field(&mut result.architecture, value)?;
                    seen_arch = true;
                }
                "pkgdesc" => copy_field(&mut result.description, value)?,
                "size" => result.size = value.parse().map_err(|_| PkgInfoError::InvalidValue)?,
                "builddate" => {
                    result.build_date = Some(value.parse().map_err(|_| PkgInfoError::InvalidValue)?)
                }
                "depend" => {
                    if result.dependency_count == ARCH_DEPENDENCY_LIMIT {
                        return Err(PkgInfoError::TooManyDependencies);
                    }
                    validate_field(value, false)?;
                    copy_field(&mut result.dependencies[result.dependency_count], value)?;
                    result.dependency_count += 1;
                }
                "license" => {
                    if result.license_count == ARCH_LICENSE_LIMIT {
                        return Err(PkgInfoError::TooManyLicenses);
                    }
                    copy_field(&mut result.licenses[result.license_count], value)?;
                    result.license_count += 1;
                }
                _ => {} // metadados ALPM futuros são preservados fora deste modelo.
            }
        }
        if !seen_name || !seen_version || !seen_arch {
            return Err(PkgInfoError::MissingRequired);
        }
        Ok(result)
    }

    pub fn name(&self) -> &str {
        field(&self.name)
    }
    pub fn version(&self) -> &str {
        field(&self.version)
    }
    pub fn architecture(&self) -> &str {
        field(&self.architecture)
    }
    pub fn description(&self) -> &str {
        field(&self.description)
    }
    pub const fn installed_size(&self) -> u64 {
        self.size
    }
    pub const fn build_date(&self) -> Option<u64> {
        self.build_date
    }
    pub fn dependencies(&self) -> impl Iterator<Item = &str> {
        self.dependencies[..self.dependency_count]
            .iter()
            .map(|value| field(value))
    }
    pub fn licenses(&self) -> impl Iterator<Item = &str> {
        self.licenses[..self.license_count]
            .iter()
            .map(|value| field(value))
    }
}

fn validate_field(value: &str, package_identifier: bool) -> Result<(), PkgInfoError> {
    if value.len() >= FIELD_BYTES || value.bytes().any(|b| b.is_ascii_control()) {
        return Err(PkgInfoError::InvalidValue);
    }
    if package_identifier
        && !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
    {
        return Err(PkgInfoError::InvalidValue);
    }
    Ok(())
}
fn copy_field<const N: usize>(destination: &mut [u8; N], value: &str) -> Result<(), PkgInfoError> {
    if value.len() >= N || value.bytes().any(|b| b.is_ascii_control()) {
        return Err(PkgInfoError::InvalidValue);
    }
    destination[..value.len()].copy_from_slice(value.as_bytes());
    Ok(())
}
fn field(value: &[u8]) -> &str {
    let length = value
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(value.len());
    core::str::from_utf8(&value[..length]).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_arch_metadata_and_dependency_lists() {
        let metadata = b"pkgname = carlio-demo\npkgver = 1.2.3-1\npkgdesc = Demo package\narch = x86_64\nsize = 4096\nbuilddate = 1700000000\ndepend = glibc>=2.38\nlicense = MIT\nlicense = Apache-2.0\n";
        let package = ArchPackageInfo::parse(metadata).unwrap();
        assert_eq!(package.name(), "carlio-demo");
        assert_eq!(package.version(), "1.2.3-1");
        assert_eq!(package.architecture(), "x86_64");
        assert_eq!(package.installed_size(), 4096);
        assert_eq!(package.build_date(), Some(1_700_000_000));
        assert_eq!(
            package.dependencies().collect::<std::vec::Vec<_>>(),
            ["glibc>=2.38"]
        );
        assert_eq!(
            package.licenses().collect::<std::vec::Vec<_>>(),
            ["MIT", "Apache-2.0"]
        );
    }

    #[test]
    fn rejects_missing_duplicate_and_foreign_architecture() {
        assert_eq!(
            ArchPackageInfo::parse(b"pkgname = a\narch = x86_64\n"),
            Err(PkgInfoError::MissingRequired)
        );
        assert_eq!(
            ArchPackageInfo::parse(b"pkgname = a\npkgname = b\npkgver = 1\narch = x86_64\n"),
            Err(PkgInfoError::DuplicateRequired)
        );
        assert_eq!(
            ArchPackageInfo::parse(b"pkgname = a\npkgver = 1\narch = armv7h\n"),
            Err(PkgInfoError::InvalidValue)
        );
    }
}
