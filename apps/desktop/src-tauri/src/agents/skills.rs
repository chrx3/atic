//! Descubrimiento de skills en disco.
//!
//! # Por qué se leen los archivos en vez de preguntarle al agente
//!
//! El CLI ya informa las skills al arrancar, pero mezcladas entre los comandos
//! de barra y con el nombre pelado: `dataviz`, `run`, `simplify`. Eso alcanza
//! para invocar una que ya conocés y no alcanza para descubrirla, que es el
//! trabajo de un selector. La descripción —lo único que dice para qué sirve—
//! vive en el `SKILL.md`, así que hay que ir a buscarla ahí.
//!
//! Se escanean los mismos dos lugares que mira el CLI, con la misma precedencia
//! —lo del proyecto gana sobre lo del usuario—, porque una lista que no
//! coincide con lo que el agente realmente cargó es peor que no tener lista.
//!
//! Idea tomada de T3 Code (`provider/Drivers/ClaudeSkills.ts`, MIT,
//! © 2026 T3 Tools Inc.), que resuelve lo mismo por el mismo motivo.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::AgentSkill;

/// Dónde guarda el CLI su configuración.
///
/// `CLAUDE_CONFIG_DIR` gana porque es lo que el propio CLI mira primero, y es
/// la variable con la que se separan perfiles —una cuenta de trabajo y una
/// personal— sin tocar `HOME`, que rompería el acceso a las credenciales.
fn config_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("CLAUDE_CONFIG_DIR") {
        let dir = dir.trim();
        if !dir.is_empty() {
            return Some(PathBuf::from(dir));
        }
    }
    // Sin crate de rutas: para este único caso son las dos variables que ese
    // crate consultaría igual, y una dependencia entera por un `join` no se
    // paga sola.
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()?;
    Some(PathBuf::from(home).join(".claude"))
}

/// Lee `name` y `description` del frontmatter de un `SKILL.md`.
///
/// No es un parser de YAML: reconoce solo esas dos claves del primer nivel,
/// que es todo lo que un `SKILL.md` usa. Traer un parser entero costaría una
/// dependencia y no leería ni un archivo más.
///
/// Lo que sí hace falta es juntar las líneas de continuación: las descripciones
/// reales son largas y siguen indentadas abajo —de las once skills de esta
/// máquina, la primera ya lo hace—, así que cortar en el salto dejaba media
/// frase en el selector. Un valor en bloque (`description: >` o `|`) es el
/// mismo caso con la primera línea vacía.
fn frontmatter(text: &str) -> Option<(Option<String>, Option<String>)> {
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))?;
    let end = rest.find("\n---")?;
    let block = &rest[..end];

    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    // A qué clave le sigue perteneciendo lo que venga indentado. `None` corta
    // la continuación: es lo que distingue un texto que sigue en la línea de
    // abajo de un mapa anidado.
    let mut open: Option<&str> = None;

    for line in block.lines() {
        if line.starts_with(char::is_whitespace) {
            // Una descripción larga sigue indentada en la línea siguiente, y es
            // lo normal: cortarla ahí dejaba media frase en el selector. Solo
            // continúa una clave que ya traía texto — si venía vacía, lo de
            // abajo es un mapa anidado (`metadata:` / `  type: user`) y sus
            // claves no son estas.
            let text = line.trim();
            if let (Some(key), false) = (open, text.is_empty()) {
                let target = match key {
                    "name" => &mut name,
                    _ => &mut description,
                };
                if let Some(acc) = target.as_mut() {
                    // El espacio solo entre trozos: con `description: >` el
                    // acumulador arranca vacío y quedaría empezando con blanco.
                    if !acc.is_empty() {
                        acc.push(' ');
                    }
                    acc.push_str(text.trim_matches(['"', '\'']));
                }
            }
            continue;
        }
        open = None;
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        // `>` y `|` anuncian que el valor entero viene abajo, indentado.
        let value = value.trim().trim_matches(['>', '|']).trim();
        let value = value.trim_matches(['"', '\'']).trim();
        if key != "name" && key != "description" {
            continue;
        }
        open = Some(if key == "name" { "name" } else { "description" });
        let slot = if key == "name" {
            &mut name
        } else {
            &mut description
        };
        // Aunque venga vacío se abre el hueco: con `description: >` el texto
        // está entero en las líneas de abajo y sin esto no habría dónde ponerlo.
        *slot = Some(value.to_string());
    }
    Some((
        name.filter(|s| !s.is_empty()),
        description.filter(|s| !s.is_empty()),
    ))
}

fn scan(root: &Path, scope: &str, out: &mut BTreeMap<String, AgentSkill>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        // Que no exista es lo normal: casi nadie tiene skills de proyecto.
        return;
    };
    for entry in entries.flatten() {
        let file = entry.path().join("SKILL.md");
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        // Sin frontmatter el CLI tampoco la carga; listarla sería prometer algo
        // que al invocarlo no está.
        let Some((name, description)) = frontmatter(&text) else {
            continue;
        };
        let name = name.unwrap_or_else(|| entry.file_name().to_string_lossy().into_owned());
        if name.trim().is_empty() {
            continue;
        }
        out.insert(
            name.clone(),
            AgentSkill {
                name,
                description: description.unwrap_or_default(),
                path: file.to_string_lossy().into_owned(),
                scope: scope.to_string(),
            },
        );
    }
}

/// Las skills que el agente vería desde `cwd`, ordenadas por nombre.
///
/// El proyecto se escanea después del usuario a propósito: `insert` pisa, y
/// esa es la precedencia del CLI —lo más específico gana.
pub fn discover(cwd: Option<&str>) -> Vec<AgentSkill> {
    let mut found = BTreeMap::new();
    if let Some(dir) = config_dir() {
        scan(&dir.join("skills"), "user", &mut found);
    }
    if let Some(cwd) = cwd {
        scan(
            &Path::new(cwd).join(".claude").join("skills"),
            "project",
            &mut found,
        );
    }
    found.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_las_dos_claves_que_importan() {
        let (name, description) =
            frontmatter("---\nname: dataviz\ndescription: Gráficos\n---\nbody")
                .expect("hay frontmatter");
        assert_eq!(name.as_deref(), Some("dataviz"));
        assert_eq!(description.as_deref(), Some("Gráficos"));
    }

    #[test]
    fn ignora_las_claves_anidadas() {
        // `metadata.name` no es el nombre de la skill; tomarlo la renombraría.
        let (name, _) = frontmatter("---\nname: real\nmetadata:\n  name: otro\n---\n").unwrap();
        assert_eq!(name.as_deref(), Some("real"));
    }

    #[test]
    fn junta_la_descripcion_que_sigue_abajo() {
        // Caso real: la primera skill del disco de esta máquina parte así, y
        // cortar en el salto dejaba media frase en el selector.
        let (_, description) = frontmatter(
            "---\nname: x\ndescription: Experto en 3D para la web\n  con Three.js\n---\n",
        )
        .unwrap();
        assert_eq!(
            description.as_deref(),
            Some("Experto en 3D para la web con Three.js")
        );
    }

    #[test]
    fn lee_el_valor_en_bloque() {
        // `description: >` no trae nada en su línea: el texto está todo abajo.
        let (_, description) = frontmatter("---\ndescription: |\n  Todo acá\n---\n").unwrap();
        assert_eq!(description.as_deref(), Some("Todo acá"));
    }

    #[test]
    fn el_mapa_anidado_no_contamina_la_descripcion() {
        // `metadata:` viene vacío, así que lo de abajo es suyo y no continúa a
        // `description`. Sin el corte, «type: user» terminaba en el selector.
        let (_, description) =
            frontmatter("---\ndescription: Corta\nmetadata:\n  type: user\n---\n").unwrap();
        assert_eq!(description.as_deref(), Some("Corta"));
    }

    #[test]
    fn sin_frontmatter_no_hay_skill() {
        // El CLI tampoco la carga: listarla sería ofrecer algo que no responde.
        assert!(frontmatter("# Solo un título\n").is_none());
    }

    #[test]
    fn saca_las_comillas() {
        let (_, description) = frontmatter("---\ndescription: \"Con comillas\"\n---\n").unwrap();
        assert_eq!(description.as_deref(), Some("Con comillas"));
    }
}
