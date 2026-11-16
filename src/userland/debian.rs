//! Leitor bounded de campos `control` para pacotes Debian/Ubuntu.
//!
//! A etapa reconhece metadados amd64 e dependências; não extrai `.deb`, não
//! resolve dependências e não executa scripts de mantenedor.

pub const DEPENDENCY_LIMIT: usize = 24;
const FIELD_BYTES: usize = 96;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlError {
    TooLarge,
    InvalidLine,
    MissingRequired,
    DuplicateRequired,
    InvalidValue,
    TooManyDependencies,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DebianPackageInfo {
    name: [u8; FIELD_BYTES],
    version: [u8; FIELD_BYTES],
    architecture: [u8; 16],
    description: [u8; FIELD_BYTES],
    installed_size_kib: u64,
    dependencies: [[u8; FIELD_BYTES]; DEPENDENCY_LIMIT],
    dependency_count: usize,
}

impl DebianPackageInfo {
    pub fn parse(control: &[u8]) -> Result<Self, ControlError> {
        if control.len() > 64 * 1024 {
            return Err(ControlError::TooLarge);
        }
        let text = core::str::from_utf8(control).map_err(|_| ControlError::InvalidValue)?;
        let mut result = Self {
            name: [0; FIELD_BYTES],
            version: [0; FIELD_BYTES],
            architecture: [0; 16],
            description: [0; FIELD_BYTES],
            installed_size_kib: 0,
            dependencies: [[0; FIELD_BYTES]; DEPENDENCY_LIMIT],
            dependency_count: 0,
        };
        let (mut name, mut version, mut arch) = (false, false, false);
        let mut in_description = false;
        for raw in text.lines() {
            if raw.starts_with(' ') || raw.starts_with('\t') {
                if in_description && result.description[0] != 0 {
                    continue;
                }
                return Err(ControlError::InvalidLine);
            }
            let line = raw.trim_end();
            if line.is_empty() {
                in_description = false;
                continue;
            }
            let (key, value) = line.split_once(':').ok_or(ControlError::InvalidLine)?;
            let value = value.trim();
            if value.is_empty() {
                return Err(ControlError::InvalidValue);
            }
            match key {
                "Package" => {
                    if name {
                        return Err(ControlError::DuplicateRequired);
                    }
                    copy(&mut result.name, value)?;
                    name = true;
                }
                "Version" => {
                    if version {
                        return Err(ControlError::DuplicateRequired);
                    }
                    copy(&mut result.version, value)?;
                    version = true;
                }
                "Architecture" => {
                    if arch {
                        return Err(ControlError::DuplicateRequired);
                    }
                    if value != "amd64" && value != "all" {
                        return Err(ControlError::InvalidValue);
                    }
                    copy(&mut result.architecture, value)?;
                    arch = true;
                }
                "Description" => {
                    copy(&mut result.description, value)?;
                    in_description = true;
                }
                "Installed-Size" => {
                    result.installed_size_kib =
                        value.parse().map_err(|_| ControlError::InvalidValue)?
                }
                "Depends" => {
                    for dependency in value.split(',').map(str::trim) {
                        if result.dependency_count == DEPENDENCY_LIMIT {
                            return Err(ControlError::TooManyDependencies);
                        }
                        let name = dependency.split([' ', '(']).next().unwrap_or("");
                        if name.is_empty() || name.len() >= FIELD_BYTES {
                            return Err(ControlError::InvalidValue);
                        }
                        copy(&mut result.dependencies[result.dependency_count], name)?;
                        result.dependency_count += 1;
                    }
                }
                _ => in_description = false,
            }
        }
        if !name || !version || !arch {
            return Err(ControlError::MissingRequired);
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
    pub const fn installed_size_kib(&self) -> u64 {
        self.installed_size_kib
    }
    pub fn dependencies(&self) -> impl Iterator<Item = &str> {
        self.dependencies[..self.dependency_count]
            .iter()
            .map(|value| field(value))
    }
}

fn copy<const N: usize>(destination: &mut [u8; N], value: &str) -> Result<(), ControlError> {
    if value.is_empty() || value.len() >= N || value.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(ControlError::InvalidValue);
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
    fn parses_ubuntu_amd64_control_fields() {
        let control = b"Package: carlio-tool\nVersion: 2:1.0-3ubuntu1\nArchitecture: amd64\nDepends: libc6 (>= 2.35), libgcc-s1\nInstalled-Size: 512\nDescription: Ferramenta\n resumo longo\n";
        let package = DebianPackageInfo::parse(control).unwrap();
        assert_eq!(package.name(), "carlio-tool");
        assert_eq!(package.architecture(), "amd64");
        assert_eq!(package.installed_size_kib(), 512);
        assert_eq!(
            package.dependencies().collect::<std::vec::Vec<_>>(),
            ["libc6", "libgcc-s1"]
        );
    }
    #[test]
    fn rejects_unknown_architecture_and_missing_fields() {
        assert_eq!(
            DebianPackageInfo::parse(b"Package: x\nVersion: 1\nArchitecture: armhf\n"),
            Err(ControlError::InvalidValue)
        );
        assert_eq!(
            DebianPackageInfo::parse(b"Package: x\nVersion: 1\n"),
            Err(ControlError::MissingRequired)
        );
    }
}
