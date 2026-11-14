//! Conversões de endereços fornecidas pelo protocolo Limine.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KernelAddress {
    pub physical_base: u64,
    pub virtual_base: u64,
}

impl KernelAddress {
    /// Converte somente endereços dentro do segmento carregado do kernel.
    /// O chamador ainda precisa garantir que o intervalo completo do buffer
    /// pertence ao kernel e está contíguo para DMA.
    pub fn virtual_to_physical(self, address: u64) -> Option<u64> {
        let offset = address.checked_sub(self.virtual_base)?;
        self.physical_base.checked_add(offset)
    }
}

pub fn hhdm_address(offset: u64, physical: u64) -> Option<u64> {
    offset.checked_add(physical)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_kernel_addresses_with_checked_arithmetic() {
        let layout = KernelAddress {
            physical_base: 0x20_0000,
            virtual_base: 0xffff_ffff_8000_0000,
        };
        assert_eq!(
            layout.virtual_to_physical(0xffff_ffff_8000_1234),
            Some(0x20_1234)
        );
        assert_eq!(layout.virtual_to_physical(0xffff_ffff_7fff_ffff), None);
        assert_eq!(
            KernelAddress {
                physical_base: u64::MAX,
                ..layout
            }
            .virtual_to_physical(layout.virtual_base + 1),
            None
        );
    }

    #[test]
    fn hhdm_translation_rejects_overflow() {
        assert_eq!(
            hhdm_address(0xffff_8000_0000_0000, 0x1234),
            Some(0xffff_8000_0000_1234)
        );
        assert_eq!(hhdm_address(u64::MAX, 1), None);
    }
}
