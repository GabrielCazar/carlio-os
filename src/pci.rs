//! Tipos e identificação de funções PCI compartilhados pelo kernel e testes.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciFunction {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class: u8,
    pub subclass: u8,
    pub programming_interface: u8,
}

impl PciFunction {
    pub const fn is_sata_ahci(self) -> bool {
        self.class == 0x01 && self.subclass == 0x06
    }

    /// IDs AHCI vistos na matriz de hardware e no controlador padrão do QEMU.
    /// A classificação não significa que o driver de I/O esteja disponível.
    pub const fn ahci_model(self) -> Option<AhciModel> {
        if !self.is_sata_ahci() || self.programming_interface != 0x01 {
            return None;
        }
        match (self.vendor_id, self.device_id) {
            (0x8086, 0x282a) => Some(AhciModel::IntelRstLocal),
            (0x8086, 0x1c02) => Some(AhciModel::IntelNas),
            (0x8086, 0x2922) => Some(AhciModel::IntelQemu),
            _ => Some(AhciModel::Unsupported),
        }
    }

    pub const fn is_network(self) -> bool {
        self.class == 0x02
    }

    pub const fn is_video(self) -> bool {
        self.class == 0x03
    }

    pub const fn is_usb(self) -> bool {
        self.class == 0x0c && self.subclass == 0x03
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AhciModel {
    IntelRstLocal,
    IntelNas,
    IntelQemu,
    Unsupported,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_target_driver_families() {
        let ahci = PciFunction {
            bus: 0,
            device: 31,
            function: 2,
            vendor_id: 0x8086,
            device_id: 0x1c02,
            class: 1,
            subclass: 6,
            programming_interface: 1,
        };
        assert!(ahci.is_sata_ahci());
        assert!(!ahci.is_network());
        let ethernet = PciFunction { class: 2, ..ahci };
        assert!(ethernet.is_network());
        let usb = PciFunction {
            class: 0x0c,
            subclass: 3,
            ..ahci
        };
        assert!(usb.is_usb());
        let video = PciFunction { class: 3, ..ahci };
        assert!(video.is_video());
    }

    #[test]
    fn identifies_known_ahci_ids_and_rejects_other_modes() {
        let ahci = PciFunction {
            bus: 0,
            device: 31,
            function: 2,
            vendor_id: 0x8086,
            device_id: 0x282a,
            class: 0x01,
            subclass: 0x06,
            programming_interface: 0x01,
        };
        assert_eq!(ahci.ahci_model(), Some(AhciModel::IntelRstLocal));
        assert_eq!(
            PciFunction {
                device_id: 0x1c02,
                ..ahci
            }
            .ahci_model(),
            Some(AhciModel::IntelNas)
        );
        assert_eq!(
            PciFunction {
                device_id: 0x2922,
                ..ahci
            }
            .ahci_model(),
            Some(AhciModel::IntelQemu)
        );
        assert_eq!(
            PciFunction {
                vendor_id: 0x1234,
                ..ahci
            }
            .ahci_model(),
            Some(AhciModel::Unsupported)
        );
        assert_eq!(
            PciFunction {
                programming_interface: 0x00,
                ..ahci
            }
            .ahci_model(),
            None
        );
    }
}
