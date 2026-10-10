//! La búsqueda de la Configuración: un índice de los ajustes de todas las pestañas (pestaña,
//! sección, título, descripción y palabras clave) y una función pura que filtra por lo que
//! escribe el usuario. Los atajos salen de la tabla de `shortcuts` con la tecla de ahora, así
//! que se encuentran por descripción, grupo o tecla («ctrl k», «paleta», «mayus»).
//!
//! Se compara sin mayúsculas ni tildes, por palabras: cada palabra de la búsqueda tiene que
//! empezar alguna palabra del ajuste («config» encuentra «configuración»; «k» solo una tecla K).

use std::collections::HashMap;

use gpui::{div, prelude::*, px, AnyElement, ClickEvent, Context, FontWeight};
use gpui_m3::{Icon, ListGroup, SettingRow};

use super::shortcuts::{self, Shortcut};
use super::style::t;
use super::CodeView;

/// Las pestañas de la Configuración, en el orden en que se muestran.
pub const CLAUDE_TAB: usize = 0;
pub const APPEARANCE_TAB: usize = 1;
pub const SHORTCUTS_TAB: usize = shortcuts::TAB;
pub const TAB_TITLES: [&str; 3] = ["Claude", "Apariencia", "Atajos"];

/// Un ajuste que no es un atajo.
pub struct Setting {
    pub tab: usize,
    pub section: &'static str,
    pub title: &'static str,
    pub desc: &'static str,
    /// Otras palabras con las que el usuario podría buscarlo.
    pub keywords: &'static str,
    /// Solo existe en Formal y Glass (Expressive no lo muestra).
    pub flat_only: bool,
}

const fn setting(tab: usize, section: &'static str, title: &'static str, desc: &'static str, keywords: &'static str, flat_only: bool) -> Setting {
    Setting { tab, section, title, desc, keywords, flat_only }
}

/// Los ajustes de las pestañas «Claude» y «Apariencia».
pub const SETTINGS: [Setting; 14] = [
    setting(CLAUDE_TAB, "Claude Code", "Buscar actualización", "Comprueba si hay una versión nueva de Claude Code y la instala", "actualizar update nueva version", false),
    setting(CLAUDE_TAB, "Instalación", "Versión instalada", "La versión de Claude Code que usa Atic Code", "claude code numero", false),
    setting(CLAUDE_TAB, "Instalación", "Última publicada", "La versión más nueva disponible", "actualizacion release", false),
    setting(CLAUDE_TAB, "Instalación", "Ejecutable", "Dónde está instalado Claude Code", "ruta path binario carpeta", false),
    setting(CLAUDE_TAB, "Cuenta", "Sesión iniciada", "La cuenta con la que entraste a Claude", "correo email login usuario", false),
    setting(CLAUDE_TAB, "Cuenta", "Plan", "Tu plan de Claude", "max pro team enterprise gratis suscripcion", false),
    setting(CLAUDE_TAB, "Cuenta", "Organización", "La organización de tu cuenta", "empresa equipo", false),
    setting(CLAUDE_TAB, "Valores del proyecto", "Modelo", "El modelo de las conversaciones nuevas del proyecto", "opus sonnet haiku", true),
    setting(CLAUDE_TAB, "Valores del proyecto", "Permisos", "Cómo pide permiso Claude para actuar", "modo aprobar plan automatico", true),
    setting(CLAUDE_TAB, "Valores del proyecto", "Esfuerzo", "Cuánto razona Claude antes de responder", "effort bajo medio alto", true),
    setting(CLAUDE_TAB, "Valores del proyecto", "Razonamiento", "Mostrar u ocultar el pensamiento de Claude", "thinking pensar", true),
    setting(APPEARANCE_TAB, "Estilo", "Estilo", "Formal, Expressive o Glass", "formal expressive glass vidrio tema diseño", false),
    setting(APPEARANCE_TAB, "Modo de color", "Modo de color", "Claro, oscuro o el del sistema", "tema claro oscuro dark light sistema", false),
    setting(APPEARANCE_TAB, "Color de acento", "Color de acento", "El color principal de cada espacio, en cada estilo", "color paleta tono espacio", false),
];

/// Un resultado.
#[derive(Clone)]
pub enum Target {
    Setting(&'static Setting),
    /// El atajo con la tecla que vale ahora.
    Shortcut(&'static Shortcut, String),
}

/// Los resultados de una pestaña (o de un grupo de atajos).
pub struct Section {
    pub tab: usize,
    pub title: String,
    pub hits: Vec<Target>,
}

/// Minúsculas y sin tildes: «Mayús» → «mayus».
pub fn fold(text: &str) -> String {
    text.chars()
        .flat_map(|c| c.to_lowercase())
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            other => other,
        })
        .collect()
}

/// Las palabras de un texto: letras y números, sin tildes ni mayúsculas (`ctrl-k` → `ctrl`, `k`).
pub fn words(text: &str) -> Vec<String> {
    fold(text).split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect()
}

/// Cada palabra de la búsqueda empieza alguna palabra del texto. Una búsqueda sin palabras no
/// encuentra nada.
pub fn matches(query: &[String], haystack: &[String]) -> bool {
    !query.is_empty() && query.iter().all(|q| haystack.iter().any(|w| w.starts_with(q.as_str())))
}

fn setting_words(item: &Setting) -> Vec<String> {
    words(&format!("{} {} {} {}", item.section, item.title, item.desc, item.keywords))
}

fn shortcut_words(row: &Shortcut, key: &str) -> Vec<String> {
    // La tecla de ahora, la de fábrica y cómo se lee en español («Mayús», «Esc»).
    let text = format!("{} {} atajo tecla {} {} {} {}", row.desc, row.group, key, row.key, shortcuts::label(key), shortcuts::label(row.key));
    words(&text)
}

/// Los resultados de `query` en todas las pestañas, agrupados por pestaña (y los atajos por
/// grupo). `expressive` oculta lo que solo existe en Formal y Glass.
pub fn search(query: &str, saved: &HashMap<String, String>, expressive: bool) -> Vec<Section> {
    let query = words(query);
    if query.is_empty() {
        return Vec::new();
    }
    let mut sections: Vec<Section> = Vec::new();
    let mut push = |tab: usize, title: String, hit: Target| match sections.iter_mut().find(|s| s.tab == tab && s.title == title) {
        Some(section) => section.hits.push(hit),
        None => sections.push(Section { tab, title, hits: vec![hit] }),
    };
    for item in SETTINGS.iter().filter(|item| !(expressive && item.flat_only)) {
        if matches(&query, &setting_words(item)) {
            push(item.tab, TAB_TITLES[item.tab].to_string(), Target::Setting(item));
        }
    }
    for group in shortcuts::GROUPS {
        for (row, key) in shortcuts::effective(saved).into_iter().filter(|(row, _)| row.group == group) {
            if matches(&query, &shortcut_words(row, &key)) {
                push(SHORTCUTS_TAB, format!("{} · {}", TAB_TITLES[SHORTCUTS_TAB], group), Target::Shortcut(row, key));
            }
        }
    }
    sections.sort_by_key(|s| s.tab);
    sections
}

impl CodeView {
    /// Lo que escribió el usuario en el buscador de la Configuración.
    pub(super) fn settings_query(&self, cx: &gpui::App) -> String {
        self.settings_search.read(cx).text().trim().to_string()
    }

    /// Vuelve a la pestaña de antes: borra la búsqueda.
    pub(super) fn clear_settings_search(&mut self, cx: &mut Context<Self>) {
        self.settings_search.update(cx, |field, cx| field.set_text("", cx));
    }

    /// Los resultados de la búsqueda, en lugar de la pestaña: cada coincidencia con su
    /// control (los atajos se pueden cambiar aquí mismo; el resto lleva a su pestaña).
    pub(super) fn settings_results(&self, query: &str, expressive: bool, cx: &mut Context<Self>) -> AnyElement {
        let t = t();
        let sections = search(query, &self.configs.shortcuts, expressive);
        let mut page = div().flex().flex_col().gap(px(14.));
        if let Some(notice) = self.shortcut_notice(cx) {
            page = page.child(notice);
        }
        if sections.is_empty() {
            return page
                .child(
                    div()
                        .py(px(28.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(6.))
                        .child(Icon::new("search").size(px(28.)).color(t.faint))
                        .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(t.text).child(format!("Nada coincide con «{query}»")))
                        .child(div().text_size(px(12.5)).text_color(t.muted).child("Prueba con otra palabra o con una tecla, como «ctrl k».")),
                )
                .into_any_element();
        }
        for section in sections {
            let mut group = ListGroup::new().title(section.title.clone());
            for hit in section.hits {
                group = match hit {
                    Target::Shortcut(row, key) => group.custom_row(self.shortcut_row(row, &key, cx)),
                    Target::Setting(item) => {
                        let tab = item.tab;
                        let title = item.title;
                        group.custom_row(
                            SettingRow::new(gpui::SharedString::from(format!("result-{tab}-{}-{title}", item.section)), title)
                                .description(format!("{} · {}", item.section, item.desc))
                                .control(Icon::new("chevron-right").size(px(18.)).color(t.faint))
                                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                    view.settings_tab = tab;
                                    view.clear_settings_search(cx);
                                    cx.notify();
                                })),
                        )
                    }
                };
            }
            page = page.child(group);
        }
        page.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none() -> HashMap<String, String> {
        HashMap::new()
    }

    fn ids(sections: &[Section]) -> Vec<String> {
        sections
            .iter()
            .flat_map(|s| s.hits.iter())
            .map(|hit| match hit {
                Target::Setting(item) => item.title.to_string(),
                Target::Shortcut(row, _) => row.id.to_string(),
            })
            .collect()
    }

    #[test]
    fn folds_case_and_accents() {
        assert_eq!(fold("Mayús CONFIGURACIÓN ñandú"), "mayus configuracion nandu");
        assert_eq!(words("Ctrl+Mayús-K"), ["ctrl", "mayus", "k"]);
        assert!(words("  ,. ").is_empty());
    }

    #[test]
    fn every_query_word_must_start_a_word() {
        let hay = words("Abrir la paleta de comandos Ctrl K");
        assert!(matches(&words("PALETA"), &hay));
        assert!(matches(&words("ctrl k"), &hay));
        assert!(matches(&words("coman pal"), &hay));
        assert!(!matches(&words("ctrl j"), &hay));
        // Empezar, no contener: «eta» no es una palabra que empiece así.
        assert!(!matches(&words("eta"), &hay));
        assert!(!matches(&[], &hay));
    }

    #[test]
    fn an_empty_query_finds_nothing() {
        assert!(search("", &none(), false).is_empty());
        assert!(search("  ¿? ", &none(), false).is_empty());
    }

    #[test]
    fn finds_shortcuts_by_key_description_and_group() {
        let by_key = search("ctrl k", &none(), false);
        assert_eq!(ids(&by_key), ["palette"]);
        assert_eq!(by_key[0].tab, SHORTCUTS_TAB);
        assert_eq!(by_key[0].title, "Atajos · General");
        assert!(ids(&search("paleta", &none(), false)).contains(&"palette_alt".to_string()));
        // Sin tildes ni mayúsculas, y por el grupo.
        assert!(ids(&search("CONVERSACION", &none(), false)).contains(&"rewind".to_string()));
        // La tecla se lee como se muestra: «Mayús».
        assert_eq!(ids(&search("mayus enter", &none(), false)), ["newline"]);
    }

    #[test]
    fn shortcuts_follow_the_saved_key() {
        let map: HashMap<String, String> = [("palette".to_string(), "alt-shift-p".to_string())].into();
        let found = search("alt shift p", &map, false);
        assert_eq!(ids(&found), ["palette"]);
        match &found[0].hits[0] {
            Target::Shortcut(_, key) => assert_eq!(key, "alt-shift-p"),
            _ => panic!("debía ser un atajo"),
        }
        // La de fábrica sigue encontrándolo, pero ya no queda «ctrl k» como tecla de otro.
        assert!(ids(&search("ctrl k", &map, false)).contains(&"palette".to_string()));
    }

    #[test]
    fn finds_settings_in_every_tab_grouped_by_tab() {
        let found = search("color", &none(), false);
        let tabs: Vec<usize> = found.iter().map(|s| s.tab).collect();
        assert_eq!(tabs, [APPEARANCE_TAB]);
        assert_eq!(ids(&found), ["Modo de color", "Color de acento"]);
        let versions = search("version", &none(), false);
        assert_eq!(versions[0].tab, CLAUDE_TAB);
        assert_eq!(versions[0].title, "Claude");
        assert!(ids(&versions).contains(&"Versión instalada".to_string()));
        // Una búsqueda que toca varias pestañas sale en el orden de las pestañas.
        let mixed = search("tema", &none(), false);
        assert_eq!(mixed.iter().map(|s| s.tab).collect::<Vec<_>>(), [APPEARANCE_TAB]);
        let wide = search("abrir", &none(), false);
        assert!(wide.iter().all(|s| s.tab == SHORTCUTS_TAB));
        let both = search("claude", &none(), false);
        assert!(both.iter().any(|s| s.tab == CLAUDE_TAB));
        assert!(both.windows(2).all(|pair| pair[0].tab <= pair[1].tab));
    }

    #[test]
    fn expressive_hides_what_only_the_flat_styles_have() {
        assert!(ids(&search("razonamiento", &none(), true)).is_empty());
        assert_eq!(ids(&search("razonamiento", &none(), false)), ["Razonamiento"]);
    }

    #[test]
    fn index_points_at_real_tabs() {
        for item in SETTINGS.iter() {
            assert!(item.tab < TAB_TITLES.len(), "pestaña inexistente: {}", item.title);
        }
        assert_eq!(TAB_TITLES[SHORTCUTS_TAB], "Atajos");
    }
}
