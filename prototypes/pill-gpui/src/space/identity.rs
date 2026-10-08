//! El nombre y el color de cada consola, puestos solos al abrirla.
//!
//! Con tres Claude abiertos, «Claude Code» no dice cuál es cuál. Cada consola
//! recibe un nombre corto (fauna de Chile) y un color, los que menos se están
//! usando entre las abiertas: así dos vivas casi nunca se repiten. El color
//! pinta su logo, su barra en el panel y su fila en la barra lateral.

const NAMES: [&str; 12] = [
    "Cóndor", "Puma", "Huemul", "Pudú", "Zorro", "Loica", "Chucao", "Tagua", "Guanaco", "Chungungo",
    "Ñandú", "Picaflor",
];

/// Colores que se distinguen entre sí sobre el fondo oscuro.
const COLORS: [u32; 8] = [0xf28b6c, 0xf2c14e, 0x9bd65a, 0x4fc6b8, 0x5fb0f5, 0xa98bf5, 0xf27fb8, 0xd9b38c];

/// El primero de `options` que menos aparece en `used`; empata el de más atrás
/// en la lista, para que se repartan en orden.
fn least_used<T: PartialEq>(options: &[T], used: &[T]) -> usize {
    (0..options.len())
        .min_by_key(|&index| used.iter().filter(|u| **u == options[index]).count())
        .unwrap_or(0)
}

/// Un nombre y un color para una consola nueva, sabiendo los de las abiertas.
pub fn pick(names: &[&str], colors: &[u32]) -> (&'static str, u32) {
    (NAMES[least_used(&NAMES, names)], COLORS[least_used(&COLORS, colors)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_primera_consola_toma_el_primer_nombre_y_color() {
        assert_eq!(pick(&[], &[]), ("Cóndor", 0xf28b6c));
    }

    #[test]
    fn no_repite_lo_que_ya_esta_abierto() {
        let (name, color) = pick(&["Cóndor", "Puma"], &[0xf28b6c, 0xf2c14e]);
        assert_eq!((name, color), ("Huemul", 0x9bd65a));
    }

    #[test]
    fn al_cerrarse_una_su_nombre_vuelve_a_estar_libre() {
        let (name, _) = pick(&["Cóndor", "Huemul"], &[]);
        assert_eq!(name, "Puma");
    }
}
