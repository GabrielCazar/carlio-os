//! Planejador limitado de transações ALPM a partir de metadados `.PKGINFO`.
//!
//! Resolve dependências nomeadas e restrições de versão para um catálogo já
//! carregado. Não baixa nem extrai arquivos e não executa install scripts.

use super::arch::ArchPackageInfo;

pub const TRANSACTION_LIMIT: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolveError {
    TooManyPackages,
    TooManyRoots,
    MissingDependency,
    VersionMismatch,
    DependencyCycle,
    InvalidDependency,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transaction {
    indexes: [usize; TRANSACTION_LIMIT],
    len: usize,
}

impl Transaction {
    pub fn packages(&self) -> &[usize] {
        &self.indexes[..self.len]
    }
}

/// Retorna índices em ordem de instalação (dependências antes dos dependentes).
pub fn resolve(catalog: &[ArchPackageInfo], roots: &[&str]) -> Result<Transaction, ResolveError> {
    if catalog.len() > TRANSACTION_LIMIT {
        return Err(ResolveError::TooManyPackages);
    }
    if roots.len() > TRANSACTION_LIMIT {
        return Err(ResolveError::TooManyRoots);
    }
    let mut state = [0u8; TRANSACTION_LIMIT]; // 0 ausente, 1 visitando, 2 concluído
    let mut transaction = Transaction {
        indexes: [0; TRANSACTION_LIMIT],
        len: 0,
    };
    for root in roots {
        let index = find_name(catalog, root).ok_or(ResolveError::MissingDependency)?;
        visit(index, catalog, &mut state, &mut transaction)?;
    }
    Ok(transaction)
}

fn visit(
    index: usize,
    catalog: &[ArchPackageInfo],
    state: &mut [u8; TRANSACTION_LIMIT],
    transaction: &mut Transaction,
) -> Result<(), ResolveError> {
    match state[index] {
        1 => return Err(ResolveError::DependencyCycle),
        2 => return Ok(()),
        _ => {}
    }
    state[index] = 1;
    for dependency in catalog[index].dependencies() {
        let (name, operator, version) = parse_dependency(dependency)?;
        let dep_index = find_name(catalog, name).ok_or(ResolveError::MissingDependency)?;
        if !version_matches(catalog[dep_index].version(), operator, version) {
            return Err(ResolveError::VersionMismatch);
        }
        visit(dep_index, catalog, state, transaction)?;
    }
    state[index] = 2;
    transaction.indexes[transaction.len] = index;
    transaction.len += 1;
    Ok(())
}

fn find_name(catalog: &[ArchPackageInfo], name: &str) -> Option<usize> {
    catalog.iter().position(|package| package.name() == name)
}

fn parse_dependency(input: &str) -> Result<(&str, Option<u8>, &str), ResolveError> {
    for (offset, byte) in input.bytes().enumerate() {
        if matches!(byte, b'>' | b'<' | b'=') {
            let name = input[..offset].trim();
            let tail = &input[offset..];
            let (operator, version) = if let Some(v) = tail.strip_prefix(">=") {
                (1, v)
            } else if let Some(v) = tail.strip_prefix("<=") {
                (2, v)
            } else if let Some(v) = tail.strip_prefix('>') {
                (3, v)
            } else if let Some(v) = tail.strip_prefix('<') {
                (4, v)
            } else if let Some(v) = tail.strip_prefix('=') {
                (5, v)
            } else {
                return Err(ResolveError::InvalidDependency);
            };
            if name.is_empty()
                || version.is_empty()
                || version.bytes().any(|b| b.is_ascii_whitespace())
            {
                return Err(ResolveError::InvalidDependency);
            }
            return Ok((name, Some(operator), version));
        }
    }
    if input.is_empty() || input.bytes().any(|b| b.is_ascii_whitespace()) {
        return Err(ResolveError::InvalidDependency);
    }
    Ok((input, None, ""))
}

fn version_matches(actual: &str, operator: Option<u8>, required: &str) -> bool {
    let Some(operator) = operator else {
        return true;
    };
    let ordering = compare_version(actual, required);
    match operator {
        1 => ordering.is_ge(),
        2 => ordering.is_le(),
        3 => ordering.is_gt(),
        4 => ordering.is_lt(),
        5 => ordering.is_eq(),
        _ => false,
    }
}

/// Compara segmentos numéricos sem overflow; segmentos textuais são ordenados
/// lexicalmente. É suficiente para o subconjunto comum de pkgver em pacotes.
fn compare_version(left: &str, right: &str) -> core::cmp::Ordering {
    use core::cmp::Ordering;
    let mut a = left
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty());
    let mut b = right
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty());
    loop {
        match (a.next(), b.next()) {
            (Some(x), Some(y)) => {
                let xn = x.bytes().all(|c| c.is_ascii_digit());
                let yn = y.bytes().all(|c| c.is_ascii_digit());
                let order = match (xn, yn) {
                    (true, true) => x
                        .trim_start_matches('0')
                        .len()
                        .cmp(&y.trim_start_matches('0').len())
                        .then_with(|| x.trim_start_matches('0').cmp(y.trim_start_matches('0'))),
                    (true, false) => Ordering::Greater,
                    (false, true) => Ordering::Less,
                    (false, false) => x.cmp(y),
                };
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), None) => {
                return if x.bytes().all(|c| c.is_ascii_digit()) && x.trim_matches('0').is_empty() {
                    Ordering::Equal
                } else {
                    Ordering::Greater
                }
            }
            (None, Some(y)) => {
                return if y.bytes().all(|c| c.is_ascii_digit()) && y.trim_matches('0').is_empty() {
                    Ordering::Equal
                } else {
                    Ordering::Less
                }
            }
            (None, None) => return Ordering::Equal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(name: &str, version: &str, dependencies: &[&str]) -> ArchPackageInfo {
        let mut text = std::format!("pkgname = {name}\npkgver = {version}\narch = x86_64\n");
        for dependency in dependencies {
            text.push_str(&std::format!("depend = {dependency}\n"));
        }
        ArchPackageInfo::parse(text.as_bytes()).unwrap()
    }

    #[test]
    fn resolves_dependencies_in_install_order_and_deduplicates() {
        let catalog = [
            pkg("app", "1.0", &["libc>=2.38", "zlib"]),
            pkg("libc", "2.39", &[]),
            pkg("zlib", "1.3", &[]),
        ];
        let plan = resolve(&catalog, &["app", "zlib"]).unwrap();
        assert_eq!(plan.packages(), &[1, 2, 0]);
    }

    #[test]
    fn rejects_missing_versions_cycles_and_malformed_constraints() {
        assert_eq!(
            resolve(&[pkg("a", "1", &["b>=2"])], &["a"]),
            Err(ResolveError::MissingDependency)
        );
        assert_eq!(
            resolve(&[pkg("a", "1", &["b>=2"]), pkg("b", "1", &[])], &["a"]),
            Err(ResolveError::VersionMismatch)
        );
        assert_eq!(
            resolve(&[pkg("a", "1", &["b"]), pkg("b", "1", &["a"])], &["a"]),
            Err(ResolveError::DependencyCycle)
        );
        assert_eq!(
            resolve(&[pkg("a", "1", &["b>>2"])], &["a"]),
            Err(ResolveError::MissingDependency)
        );
    }
}
