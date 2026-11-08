//! Las teclas viajan como códigos HID (USB, página 7: la posición física de
//! la tecla). Cada lado traduce desde y hacia lo suyo: los códigos de
//! escaneo en Windows y los `kVK_*` en macOS. Así una tecla es la misma tecla
//! en los dos, y cada sistema la interpreta con su distribución de teclado.

/// Sin tecla equivalente en ese sistema.
const NO: u16 = 0xFFFF;
/// En Windows, las teclas extendidas (prefijo `E0`).
const E0: u16 = 0xE000;

/// (HID, código de escaneo de Windows, `kVK` de macOS).
const TABLE: &[(u16, u16, u16)] = &[
    // Letras.
    (0x04, 0x1E, 0x00), // A
    (0x05, 0x30, 0x0B), // B
    (0x06, 0x2E, 0x08), // C
    (0x07, 0x20, 0x02), // D
    (0x08, 0x12, 0x0E), // E
    (0x09, 0x21, 0x03), // F
    (0x0A, 0x22, 0x05), // G
    (0x0B, 0x23, 0x04), // H
    (0x0C, 0x17, 0x22), // I
    (0x0D, 0x24, 0x26), // J
    (0x0E, 0x25, 0x28), // K
    (0x0F, 0x26, 0x25), // L
    (0x10, 0x32, 0x2E), // M
    (0x11, 0x31, 0x2D), // N
    (0x12, 0x18, 0x1F), // O
    (0x13, 0x19, 0x23), // P
    (0x14, 0x10, 0x0C), // Q
    (0x15, 0x13, 0x0F), // R
    (0x16, 0x1F, 0x01), // S
    (0x17, 0x14, 0x11), // T
    (0x18, 0x16, 0x20), // U
    (0x19, 0x2F, 0x09), // V
    (0x1A, 0x11, 0x0D), // W
    (0x1B, 0x2D, 0x07), // X
    (0x1C, 0x15, 0x10), // Y
    (0x1D, 0x2C, 0x06), // Z
    // Números de la fila de arriba.
    (0x1E, 0x02, 0x12), // 1
    (0x1F, 0x03, 0x13), // 2
    (0x20, 0x04, 0x14), // 3
    (0x21, 0x05, 0x15), // 4
    (0x22, 0x06, 0x17), // 5
    (0x23, 0x07, 0x16), // 6
    (0x24, 0x08, 0x1A), // 7
    (0x25, 0x09, 0x1C), // 8
    (0x26, 0x0A, 0x19), // 9
    (0x27, 0x0B, 0x1D), // 0
    (0x28, 0x1C, 0x24), // Enter
    (0x29, 0x01, 0x35), // Esc
    (0x2A, 0x0E, 0x33), // Retroceso
    (0x2B, 0x0F, 0x30), // Tab
    (0x2C, 0x39, 0x31), // Espacio
    (0x2D, 0x0C, 0x1B), // -
    (0x2E, 0x0D, 0x18), // =
    (0x2F, 0x1A, 0x21), // [
    (0x30, 0x1B, 0x1E), // ]
    (0x31, 0x2B, 0x2A), // \
    (0x32, NO, 0x2A),   // # de los teclados ISO (Windows lo da como \)
    (0x33, 0x27, 0x29), // ;
    (0x34, 0x28, 0x27), // '
    (0x35, 0x29, 0x32), // `
    (0x36, 0x33, 0x2B), // ,
    (0x37, 0x34, 0x2F), // .
    (0x38, 0x35, 0x2C), // /
    (0x39, 0x3A, 0x39), // Bloq Mayús
    // F1 a F12.
    (0x3A, 0x3B, 0x7A),
    (0x3B, 0x3C, 0x78),
    (0x3C, 0x3D, 0x63),
    (0x3D, 0x3E, 0x76),
    (0x3E, 0x3F, 0x60),
    (0x3F, 0x40, 0x61),
    (0x40, 0x41, 0x62),
    (0x41, 0x42, 0x64),
    (0x42, 0x43, 0x65),
    (0x43, 0x44, 0x6D),
    (0x44, 0x57, 0x67),
    (0x45, 0x58, 0x6F),
    // Impr Pant, Bloq Despl y Pausa: en los teclados de Apple están F13 a F15 en ese lugar.
    (0x46, E0 | 0x37, 0x69),
    (0x47, 0x46, 0x6B),
    (0x48, 0x45, 0x71),
    (0x49, E0 | 0x52, 0x72), // Insert (Help en el Mac)
    (0x4A, E0 | 0x47, 0x73), // Inicio
    (0x4B, E0 | 0x49, 0x74), // Re Pág
    (0x4C, E0 | 0x53, 0x75), // Supr
    (0x4D, E0 | 0x4F, 0x77), // Fin
    (0x4E, E0 | 0x51, 0x79), // Av Pág
    (0x4F, E0 | 0x4D, 0x7C), // →
    (0x50, E0 | 0x4B, 0x7B), // ←
    (0x51, E0 | 0x50, 0x7D), // ↓
    (0x52, E0 | 0x48, 0x7E), // ↑
    // Teclado numérico.
    (0x53, E0 | 0x45, 0x47), // Bloq Num (Clear en el Mac)
    (0x54, E0 | 0x35, 0x4B), // /
    (0x55, 0x37, 0x43),      // *
    (0x56, 0x4A, 0x4E),      // -
    (0x57, 0x4E, 0x45),      // +
    (0x58, E0 | 0x1C, 0x4C), // Enter
    (0x59, 0x4F, 0x53),      // 1
    (0x5A, 0x50, 0x54),      // 2
    (0x5B, 0x51, 0x55),      // 3
    (0x5C, 0x4B, 0x56),      // 4
    (0x5D, 0x4C, 0x57),      // 5
    (0x5E, 0x4D, 0x58),      // 6
    (0x5F, 0x47, 0x59),      // 7
    (0x60, 0x48, 0x5B),      // 8
    (0x61, 0x49, 0x5C),      // 9
    (0x62, 0x52, 0x52),      // 0
    (0x63, 0x53, 0x41),      // .
    (0x64, 0x56, 0x0A),      // < > de los teclados ISO
    (0x65, E0 | 0x5D, NO),   // Menú contextual
    (0x67, 0x59, 0x51),      // = del teclado numérico
    // F13 a F20 (los que tiene el Mac).
    (0x68, 0x64, 0x69),
    (0x69, 0x65, 0x6B),
    (0x6A, 0x66, 0x71),
    (0x6B, 0x67, 0x6A),
    (0x6C, 0x68, 0x40),
    (0x6D, 0x69, 0x4F),
    (0x6E, 0x6A, 0x50),
    (0x6F, 0x6B, 0x5A),
    // Volumen.
    (0x7F, E0 | 0x20, 0x4A), // Silenciar
    (0x80, E0 | 0x30, 0x48), // Subir
    (0x81, E0 | 0x2E, 0x49), // Bajar
    (0x87, 0x73, 0x5E),      // Ro (JIS)
    (0x89, 0x7D, 0x5D),      // Yen (JIS)
    // Modificadores.
    (0xE0, 0x1D, 0x3B),      // Ctrl izquierdo
    (0xE1, 0x2A, 0x38),      // Mayús izquierda
    (0xE2, 0x38, 0x3A),      // Alt / Opción izquierda
    (0xE3, E0 | 0x5B, 0x37), // Windows / Comando izquierdo
    (0xE4, E0 | 0x1D, 0x3E), // Ctrl derecho
    (0xE5, 0x36, 0x3C),      // Mayús derecha
    (0xE6, E0 | 0x38, 0x3D), // Alt Gr / Opción derecha
    (0xE7, E0 | 0x5C, 0x36), // Windows / Comando derecho
];

pub const LEFT_CTRL: u16 = 0xE0;
pub const LEFT_ALT: u16 = 0xE2;
pub const LEFT_META: u16 = 0xE3;
pub const RIGHT_CTRL: u16 = 0xE4;
pub const RIGHT_ALT: u16 = 0xE6;
pub const RIGHT_META: u16 = 0xE7;
pub const ESCAPE: u16 = 0x29;
pub const CAPS_LOCK: u16 = 0x39;

pub fn is_modifier(hid: u16) -> bool {
    (0xE0..=0xE7).contains(&hid)
}

/// Ctrl por Comando (o Windows) y al revés, para que Ctrl+C en el PC sea ⌘C en
/// el Mac. Lo aplica quien recibe cuando el otro equipo es de otro sistema.
pub fn swap_ctrl_meta(hid: u16) -> u16 {
    match hid {
        LEFT_CTRL => LEFT_META,
        LEFT_META => LEFT_CTRL,
        RIGHT_CTRL => RIGHT_META,
        RIGHT_META => RIGHT_CTRL,
        other => other,
    }
}

/// La tecla de un gancho de teclado de Windows (`KBDLLHOOKSTRUCT`): su código
/// de escaneo, si es extendida y su tecla virtual (para las que el código no
/// distingue bien).
pub fn hid_from_windows(scan: u32, extended: bool, vk: u32) -> Option<u16> {
    // Pausa y Bloq Num comparten el código 0x45; las distingue la tecla virtual.
    match vk {
        0x13 => return Some(0x48), // VK_PAUSE
        0x90 => return Some(0x53), // VK_NUMLOCK
        0xA1 => return Some(0xE5), // VK_RSHIFT: a veces llega marcada como extendida
        0x2C => return Some(0x46), // VK_SNAPSHOT
        _ => {}
    }
    // Alt Gr manda un Ctrl izquierdo falso con el bit 0x200: no es una tecla.
    if scan & 0x200 != 0 {
        return None;
    }
    let code = (scan & 0xFF) as u16 | if extended { E0 } else { 0 };
    TABLE.iter().find(|&&(_, win, _)| win == code).map(|&(hid, _, _)| hid)
}

/// Para `SendInput` con `KEYEVENTF_SCANCODE`: (código, extendida).
pub fn windows_from_hid(hid: u16) -> Option<(u16, bool)> {
    let &(_, win, _) = TABLE.iter().find(|&&(h, _, _)| h == hid)?;
    if win == NO {
        return None;
    }
    Some((win & 0xFF, win & E0 == E0))
}

pub fn hid_from_mac(vk: u16) -> Option<u16> {
    // F13 a F15 están dos veces en la tabla (como Impr Pant, Bloq Despl y
    // Pausa): desde el Mac son F13 a F15.
    match vk {
        0x69 => return Some(0x68),
        0x6B => return Some(0x69),
        0x71 => return Some(0x6A),
        0x2A => return Some(0x31),
        _ => {}
    }
    TABLE.iter().find(|&&(_, _, mac)| mac == vk).map(|&(hid, _, _)| hid)
}

pub fn mac_from_hid(hid: u16) -> Option<u16> {
    let &(_, _, mac) = TABLE.iter().find(|&&(h, _, _)| h == hid)?;
    (mac != NO).then_some(mac)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cada_hid_esta_una_sola_vez() {
        for (i, a) in TABLE.iter().enumerate() {
            assert!(TABLE[i + 1..].iter().all(|b| b.0 != a.0), "HID {:#x} repetido", a.0);
        }
    }

    #[test]
    fn de_windows_y_de_vuelta() {
        for &(hid, win, _) in TABLE {
            if win == NO {
                continue;
            }
            let (scan, ext) = windows_from_hid(hid).unwrap();
            // Pausa, Bloq Num, Impr Pant y Mayús derecha se reconocen por la tecla virtual.
            if [0x46, 0x48, 0x53, 0xE5].contains(&hid) {
                continue;
            }
            assert_eq!(hid_from_windows(scan as u32, ext, 0), Some(hid), "HID {hid:#x}");
        }
    }

    #[test]
    fn del_mac_y_de_vuelta() {
        for &(hid, _, mac) in TABLE {
            if mac == NO {
                continue;
            }
            let back = hid_from_mac(mac).unwrap();
            assert_eq!(mac_from_hid(back), Some(mac), "HID {hid:#x}");
        }
    }

    #[test]
    fn letras_y_atajos_conocidos() {
        // C: escaneo 0x2E en Windows, kVK_ANSI_C (8) en el Mac.
        assert_eq!(hid_from_windows(0x2E, false, 0x43), Some(0x06));
        assert_eq!(mac_from_hid(0x06), Some(0x08));
        // Ctrl izquierdo de Windows llega como ⌘ izquierdo al cambiarlos.
        let ctrl = hid_from_windows(0x1D, false, 0xA2).unwrap();
        assert_eq!(mac_from_hid(swap_ctrl_meta(ctrl)), Some(0x37));
        // Flecha derecha es extendida; sin el E0 es el 6 del teclado numérico.
        assert_eq!(hid_from_windows(0x4D, true, 0x27), Some(0x4F));
        assert_eq!(hid_from_windows(0x4D, false, 0x66), Some(0x5E));
        // El Ctrl falso de Alt Gr no se manda.
        assert_eq!(hid_from_windows(0x21D, false, 0xA2), None);
    }
}
