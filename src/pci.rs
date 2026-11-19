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
    /// Nome da classe PCI definida pela especificação Base Class Codes.
    pub const fn class_name(self) -> &'static str {
        match self.class {
            0x01 => "armazenamento",
            0x02 => "rede",
            0x03 => "vídeo",
            0x04 => "multimídia",
            0x05 => "memória",
            0x06 => "bridge",
            0x07 => "comunicação",
            0x08 => "sistema",
            0x09 => "entrada",
            0x0a => "dock",
            0x0b => "processador",
            0x0c => "barramento serial",
            0x0d => "wireless",
            0x0e => "inteligente",
            0x0f => "satélite",
            0x10 => "criptografia",
            0x11 => "sinalização",
            0x12 => "acelerador",
            0x13 => "instrumentação",
            0x40 => "coprocessador",
            _ => "desconhecido",
        }
    }

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

    /// Identifica os controladores Ethernet de referência sem habilitar BARs
    /// nem reivindicar suporte de rede.
    pub const fn network_model(self) -> Option<NetworkModel> {
        if !self.is_network() {
            return None;
        }
        match (self.vendor_id, self.device_id) {
            (0x10ec, 0x8168) => Some(NetworkModel::RealtekRtl8111Nas),
            (0x8086, 0x15d7) => Some(NetworkModel::IntelI219Local),
            (0x8086, 0x10d3) => Some(NetworkModel::Intel82574Qemu),
            _ => Some(NetworkModel::Unsupported),
        }
    }

    pub const fn is_video(self) -> bool {
        self.class == 0x03
    }

    /// Modelos iGPU Intel previstos na máquina local e no servidor. A
    /// classificação não habilita BARs nem equivale a modesetting/aceleração.
    pub const fn integrated_graphics_model(self) -> Option<IntegratedGraphicsModel> {
        if !self.is_video() || self.vendor_id != 0x8086 {
            return None;
        }
        match self.device_id {
            0x1916 => Some(IntegratedGraphicsModel::IntelHdGraphics520),
            0x0152 => Some(IntegratedGraphicsModel::IntelIvyBridgeGt1),
            _ => Some(IntegratedGraphicsModel::UnsupportedIntel),
        }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkModel {
    RealtekRtl8111Nas,
    IntelI219Local,
    Intel82574Qemu,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegratedGraphicsModel {
    IntelHdGraphics520,
    IntelIvyBridgeGt1,
    UnsupportedIntel,
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

    #[test]
    fn identifies_server_and_development_ethernet_without_claiming_driver_support() {
        let ethernet = PciFunction {
            bus: 0,
            device: 0,
            function: 0,
            vendor_id: 0x10ec,
            device_id: 0x8168,
            class: 2,
            subclass: 0,
            programming_interface: 0,
        };
        assert_eq!(
            ethernet.network_model(),
            Some(NetworkModel::RealtekRtl8111Nas)
        );
        assert_eq!(
            PciFunction {
                vendor_id: 0x8086,
                device_id: 0x15d7,
                ..ethernet
            }
            .network_model(),
            Some(NetworkModel::IntelI219Local)
        );
        assert_eq!(
            PciFunction {
                device_id: 0x9999,
                ..ethernet
            }
            .network_model(),
            Some(NetworkModel::Unsupported)
        );
        assert_eq!(
            PciFunction {
                class: 3,
                ..ethernet
            }
            .network_model(),
            None
        );
    }

    #[test]
    fn classifies_supported_intel_integrated_graphics_ids() {
        let gpu = PciFunction {
            bus: 0,
            device: 2,
            function: 0,
            vendor_id: 0x8086,
            device_id: 0x1916,
            class: 0x03,
            subclass: 0x00,
            programming_interface: 0,
        };
        assert_eq!(
            gpu.integrated_graphics_model(),
            Some(IntegratedGraphicsModel::IntelHdGraphics520)
        );
        assert_eq!(
            PciFunction {
                device_id: 0x0152,
                ..gpu
            }
            .integrated_graphics_model(),
            Some(IntegratedGraphicsModel::IntelIvyBridgeGt1)
        );
        assert_eq!(
            PciFunction { class: 0x02, ..gpu }.integrated_graphics_model(),
            None
        );
    }
}
