//! Calculadora inline del launcher: aritmética, unidades, divisas y zonas
//! horarias («15:00 chile en madrid», «hora en tokio»).
//!
//! Lo que se puede calcular local se calcula local. Las divisas necesitan
//! tasas de cambio: [`evaluate_with`] las recibe del módulo `fx` (opt-in,
//! caché en disco) y el resultado viaja con su procedencia («valor oficial
//! del 25 sep») para que el launcher nunca muestre un número sin contexto.
//! Sin tasas no hay conversión de divisas, y sin certeza no se muestra nada.
//!
//! `evaluate_with` devuelve `None` cuando la query **no** es una cuenta: el
//! launcher usa eso para decidir si agrega el resultado como primer hit.

/// Tope de decimales del resultado. Más que esto es ruido de punto flotante.
const DECIMALS: usize = 6;

/// Idioma de la UI: manda al parsear montos ambiguos y al mostrarlos.
///
/// En es-CL el punto agrupa miles y la coma es decimal («30.000,50»); en
/// inglés es al revés («30,000.50»). Un único separador con cola de tres
/// dígitos («1.234») se resuelve con el idioma activo: es → miles, en →
/// decimal (en inglés el punto nunca agrupa: «1.234.567» se castiga igual).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Locale {
    Es,
    En,
}

/// Qué tan al día están las tasas que acompañan al resultado.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Freshness {
    Fresh,
    /// Pasó más de un día sin descarga buena: los números son viejos y se dice.
    Stale,
    /// El último intento falló: se conserva la última tabla, avisando.
    Offline,
}

/// Tasas disponibles para el conversor. La implementación real vive en `fx`
/// (red + caché); acá solo se consulta, sin red ni dependencias.
pub trait RatesLookup {
    /// Pesos chilenos por una unidad de `code` (p. ej. 41.024,46 por UF).
    /// Devuelve además la fecha ISO de la tasa y su origen (`bcch` oficial |
    /// `market` mercado).
    fn clp_per_unit(&self, code: &str) -> Option<(f64, &str, &str)>;

    fn freshness(&self) -> Freshness;
}

/// Resultado de evaluar una query: lo que se muestra/copia y, si hubo tasa,
/// su procedencia (fuente + fecha) para el subtítulo del launcher.
#[derive(Clone, Debug, PartialEq)]
pub struct Conversion {
    pub value: String,
    pub source: Option<String>,
}

/// Evalúa una query: aritmética, unidades o divisas (con las tasas recibidas).
///
/// Las divisas se prueban primero; lo que no sea una cuenta sigue el camino
/// local de siempre. `rates` en `None` deja la calculadora como antes de este
/// módulo: sin red y sin divisas.
pub fn evaluate_with(
    query: &str,
    rates: Option<&dyn RatesLookup>,
    locale: Locale,
) -> Option<Conversion> {
    let q = query.trim();
    if q.is_empty() {
        return None;
    }
    if let Some(hit) = convert_query(q, rates, locale) {
        return Some(hit);
    }
    if let Some(hit) = zone_query(q, chrono::Utc::now(), local_zone(), locale) {
        return Some(hit);
    }
    let mut parser = Parser::new(q, locale);
    let value = parser.expr()?;
    parser.skip_ws();
    // Si sobra texto, la query era otra cosa («2 chrome» no es una cuenta).
    if !parser.eof() {
        return None;
    }
    format_number(value).map(|value| Conversion {
        value,
        source: None,
    })
}

/// ¿La query es una conversión de divisas, haya o no tasas a mano?
///
/// Con el opt-in apagado, el launcher lo usa para ofrecer encenderlo; con el
/// opt-in encendido y la tabla vieja, para refrescar en background. Acá no se
/// exige que las monedas existan en la tabla: alcanza con que la query tenga
/// forma de conversión entre dinero.
pub fn money_conversion(query: &str, locale: Locale) -> bool {
    let lower = query.to_lowercase().replace('°', "");
    for &sep in SEPARATORS {
        let Some((left, right)) = lower.split_once(sep) else {
            continue;
        };
        let Some(from) = parse_side(left, locale) else {
            continue;
        };
        let Some(to) = parse_side(right.trim(), locale) else {
            continue;
        };
        if from.money.is_some() && to.money.is_some() {
            return true;
        }
    }
    false
}

/// Número listo para mostrar: sin ceros de cola, sin `-0`, sin separador de
/// miles (el valor se copia tal cual, así que tiene que ser pegable).
fn format_number(value: f64) -> Option<String> {
    if !value.is_finite() {
        return None;
    }
    let mut text = format!("{value:.DECIMALS$}");
    if text.contains('.') {
        text = text.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    if text.is_empty() || text == "-" {
        text = "0".into();
    }
    // Redondeado a cero pero no cero: se muestra en notación científica en vez
    // de mentir con un 0.
    if text == "0" || text == "-0" {
        if value != 0.0 {
            return Some(format!("{value:e}"));
        }
        return Some("0".into());
    }
    Some(text)
}

// ---------------------------------------------------------------------------
// Aritmética: descenso recursivo, sin dependencias.
// ---------------------------------------------------------------------------

struct Parser<'a> {
    src: &'a str,
    i: usize,
    locale: Locale,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str, locale: Locale) -> Self {
        Self { src, i: 0, locale }
    }

    fn eof(&self) -> bool {
        self.i >= self.src.len()
    }

    fn peek(&self) -> Option<char> {
        self.src[self.i..].chars().next()
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.i += c.len_utf8();
            } else {
                break;
            }
        }
    }

    fn eat(&mut self, c: char) -> bool {
        self.skip_ws();
        if self.peek() == Some(c) {
            self.i += c.len_utf8();
            true
        } else {
            false
        }
    }

    /// `expr := term (('+' | '-') term)*`
    fn expr(&mut self) -> Option<f64> {
        let mut acc = self.term()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('+') => {
                    self.i += 1;
                    acc += self.term()?;
                }
                Some('-') => {
                    self.i += 1;
                    acc -= self.term()?;
                }
                _ => return Some(acc),
            }
        }
    }

    /// `term := unary (('*' | '/' | '%') unary)*`
    fn term(&mut self) -> Option<f64> {
        let mut acc = self.unary()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('*') => {
                    self.i += 1;
                    acc *= self.unary()?;
                }
                Some('/') => {
                    self.i += 1;
                    let divisor = self.unary()?;
                    if divisor == 0.0 {
                        return None;
                    }
                    acc /= divisor;
                }
                Some('%') => {
                    self.i += 1;
                    let divisor = self.unary()?;
                    if divisor == 0.0 {
                        return None;
                    }
                    acc %= divisor;
                }
                _ => return Some(acc),
            }
        }
    }

    /// `unary := ('-' | '+')? power` — así `-4^2` da -16, como en una calculadora.
    fn unary(&mut self) -> Option<f64> {
        self.skip_ws();
        match self.peek() {
            Some('-') => {
                self.i += 1;
                Some(-self.unary()?)
            }
            Some('+') => {
                self.i += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }

    /// `power := primary ('^' unary)?` — asociativa a la derecha.
    fn power(&mut self) -> Option<f64> {
        let base = self.primary()?;
        self.skip_ws();
        if self.peek() == Some('^') {
            self.i += 1;
            let exp = self.unary()?;
            return Some(base.powf(exp));
        }
        Some(base)
    }

    fn primary(&mut self) -> Option<f64> {
        if self.eat('(') {
            let value = self.expr()?;
            if !self.eat(')') {
                return None;
            }
            return Some(value);
        }
        self.skip_ws();
        let start = self.i;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '.' || c == ',' || c == '_' {
                self.i += c.len_utf8();
            } else {
                break;
            }
        }
        if start == self.i {
            return None;
        }
        // `1_000` se acepta como separador visual; `30.000` y `30,000` según el
        // idioma.
        parse_amount(&self.src[start..self.i], self.locale)
    }
}

// ---------------------------------------------------------------------------
// Números: agrupación de miles y decimal, en los dos formatos.
// ---------------------------------------------------------------------------

/// Monto o número escrito por un humano, en cualquiera de los dos formatos.
///
/// Con los dos separadores presentes, el último es el decimal («30.000,50» /
/// «30,000.50»). Con uno solo, el patrón de miles (grupos de exactamente 3
/// dígitos) manda cuando el separador es el de miles del idioma; si no, es
/// decimal aunque venga del otro formato («1.23» es 1,23 en los dos).
fn parse_amount(raw: &str, locale: Locale) -> Option<f64> {
    let text = raw.replace('_', "");
    let (sign, digits) = match text.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, text.strip_prefix('+').unwrap_or(&text)),
    };
    if digits.is_empty() {
        return None;
    }
    if !digits
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.' || c == ',')
    {
        return None;
    }

    let group = match locale {
        Locale::Es => '.',
        Locale::En => ',',
    };
    let normalized = if digits.contains('.') && digits.contains(',') {
        // El último separador es el decimal; el otro agrupa. Cualquier otro
        // uso (dos decimales, grupos que no son de 3) invalida el monto.
        let dec_index = digits.rfind(['.', ','])?;
        let dec = digits.as_bytes()[dec_index] as char;
        let group = if dec == '.' { ',' } else { '.' };
        let (before, after) = digits.split_at(dec_index);
        let after = &after[1..];
        if after.contains(dec) || after.contains(group) || before.contains(dec) {
            return None;
        }
        let int_digits = if before.contains(group) {
            if !is_grouped(before, group) {
                return None;
            }
            before.replace(group, "")
        } else {
            before.to_string()
        };
        if int_digits.is_empty() {
            format!(".{after}")
        } else {
            format!("{int_digits}.{after}")
        }
    } else if let Some(sep) = digits.chars().find(|c| *c == '.' || *c == ',') {
        let grouped = is_grouped(digits, sep);
        if grouped && (sep == group || digits.matches(sep).count() > 1) {
            digits.replace(sep, "")
        } else {
            digits.replace(sep, ".")
        }
    } else {
        digits.to_string()
    };

    let value = normalized.parse::<f64>().ok()?;
    if !value.is_finite() {
        return None;
    }
    Some(sign * value)
}

/// ¿`text` usa `sep` como separador de miles? Parte entera de 1 a 3 dígitos y
/// exactamente un grupo de 3 (o varios) detrás.
fn is_grouped(text: &str, sep: char) -> bool {
    let mut parts = text.split(sep);
    let Some(first) = parts.next() else {
        return false;
    };
    if first.is_empty() || first.len() > 3 || !first.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let mut groups = 0;
    for part in parts {
        if part.len() != 3 || !part.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
        groups += 1;
    }
    groups > 0
}

// ---------------------------------------------------------------------------
// Unidades: `<número> <unidad> (to | in | a | en | ->) <unidad>`
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Unit {
    // Longitud (base: metro)
    Mm,
    Cm,
    M,
    Km,
    In,
    Ft,
    Yd,
    Mi,
    // Masa (base: gramo)
    Mg,
    G,
    Kg,
    Oz,
    Lb,
    // Datos (base: byte). Decimales (kB = 1000 B), binarios (KiB = 1024 B) y
    // bits (kbit = 1000 bits = 125 B).
    Bit,
    Byte,
    Kb,
    Mb,
    Gb,
    Tb,
    Kib,
    Mib,
    Gib,
    Tib,
    Kbit,
    Mbit,
    Gbit,
    // Tiempo (base: segundo)
    Ms,
    S,
    Min,
    H,
    D,
    Week,
    Month,
    Year,
    // Temperatura (no lineal: fórmulas propias)
    Celsius,
    Fahrenheit,
    Kelvin,
    // Volumen (base: litro). Galón, taza y onza líquida de EE. UU.
    Ml,
    Cl,
    L,
    M3,
    Gal,
    Tbsp,
    Tsp,
    Cup,
    FlOz,
    // Área (base: metro cuadrado)
    Cm2,
    M2,
    Km2,
    Ha,
    Acre,
    Ft2,
    // Velocidad (base: metro por segundo)
    Mps,
    Kmh,
    Mph,
    Knot,
}

/// Familia de la unidad: convertir entre familias distintas no tiene sentido.
fn family(unit: Unit) -> u8 {
    use Unit::*;
    match unit {
        Mm | Cm | M | Km | In | Ft | Yd | Mi => 0,
        Mg | G | Kg | Oz | Lb => 1,
        Bit | Byte | Kb | Mb | Gb | Tb | Kib | Mib | Gib | Tib | Kbit | Mbit | Gbit => 2,
        Ms | S | Min | H | D | Week | Month | Year => 3,
        Celsius | Fahrenheit | Kelvin => 4,
        Ml | Cl | L | M3 | Gal | Tbsp | Tsp | Cup | FlOz => 5,
        Cm2 | M2 | Km2 | Ha | Acre | Ft2 => 6,
        Mps | Kmh | Mph | Knot => 7,
    }
}

/// Los nombres de cada unidad, en singular, minúsculas y sin tildes. Los
/// plurales se resuelven en `parse_unit` («metros cuadrados», «galones»).
///
/// Ojo con los choques: «m» es metro y «ms» milisegundo (no «m» en plural);
/// «mes» es mes y no «me» + s; «min» es minuto y «mi» milla; «mb» es megabyte
/// y «mbit»/«mbps» megabit. «libra» es masa aquí y libra esterlina en
/// `MONEY_ALIASES`: el desempate lo hace `convert_sides`.
const UNIT_NAMES: &[(&str, Unit)] = {
    use Unit::*;
    &[
        ("mm", Mm),
        ("milimetro", Mm),
        ("millimeter", Mm),
        ("cm", Cm),
        ("centimetro", Cm),
        ("centimeter", Cm),
        ("m", M),
        ("metro", M),
        ("meter", M),
        ("metre", M),
        ("km", Km),
        ("kilometro", Km),
        ("kilometer", Km),
        ("in", In),
        ("pulgada", In),
        ("inch", In),
        ("inche", In),
        ("ft", Ft),
        ("pie", Ft),
        ("foot", Ft),
        ("feet", Ft),
        ("yd", Yd),
        ("yarda", Yd),
        ("yard", Yd),
        ("mi", Mi),
        ("milla", Mi),
        ("mile", Mi),
        ("mg", Mg),
        ("miligramo", Mg),
        ("g", G),
        ("gr", G),
        ("gramo", G),
        ("gram", G),
        ("kg", Kg),
        ("kilo", Kg),
        ("kilogramo", Kg),
        ("kilogram", Kg),
        ("oz", Oz),
        ("onza", Oz),
        ("ounce", Oz),
        ("lb", Lb),
        ("libra", Lb),
        ("pound", Lb),
        ("bit", Bit),
        ("byte", Byte),
        ("kb", Kb),
        ("kilobyte", Kb),
        ("mb", Mb),
        ("megabyte", Mb),
        ("gb", Gb),
        ("gigabyte", Gb),
        ("tb", Tb),
        ("terabyte", Tb),
        ("kib", Kib),
        ("kibibyte", Kib),
        ("mib", Mib),
        ("mebibyte", Mib),
        ("gib", Gib),
        ("gibibyte", Gib),
        ("tib", Tib),
        ("tebibyte", Tib),
        ("kbit", Kbit),
        ("kbps", Kbit),
        ("kilobit", Kbit),
        ("mbit", Mbit),
        ("mbps", Mbit),
        ("megabit", Mbit),
        ("gbit", Gbit),
        ("gbps", Gbit),
        ("gigabit", Gbit),
        ("ms", Ms),
        ("milisegundo", Ms),
        ("millisecond", Ms),
        ("s", S),
        ("seg", S),
        ("sec", S),
        ("segundo", S),
        ("second", S),
        ("min", Min),
        ("minuto", Min),
        ("minute", Min),
        ("h", H),
        ("hr", H),
        ("hora", H),
        ("hour", H),
        ("d", D),
        ("dia", D),
        ("day", D),
        ("sem", Week),
        ("semana", Week),
        ("week", Week),
        ("wk", Week),
        ("mes", Month),
        ("month", Month),
        ("ano", Year),
        ("year", Year),
        ("yr", Year),
        ("c", Celsius),
        ("celsius", Celsius),
        ("centigrado", Celsius),
        ("f", Fahrenheit),
        ("fahrenheit", Fahrenheit),
        ("k", Kelvin),
        ("kelvin", Kelvin),
        ("ml", Ml),
        ("mililitro", Ml),
        ("milliliter", Ml),
        ("millilitre", Ml),
        ("cl", Cl),
        ("centilitro", Cl),
        ("centiliter", Cl),
        ("l", L),
        ("lt", L),
        ("litro", L),
        ("liter", L),
        ("litre", L),
        ("m3", M3),
        ("m³", M3),
        ("metro cubico", M3),
        ("cubic meter", M3),
        ("gal", Gal),
        ("galon", Gal),
        ("gallon", Gal),
        ("cucharada", Tbsp),
        ("cda", Tbsp),
        ("tbsp", Tbsp),
        ("tablespoon", Tbsp),
        ("cucharadita", Tsp),
        ("cdta", Tsp),
        ("tsp", Tsp),
        ("teaspoon", Tsp),
        ("taza", Cup),
        ("cup", Cup),
        ("fl oz", FlOz),
        ("floz", FlOz),
        ("onza liquida", FlOz),
        ("fluid ounce", FlOz),
        ("cm2", Cm2),
        ("cm²", Cm2),
        ("centimetro cuadrado", Cm2),
        ("square centimeter", Cm2),
        ("m2", M2),
        ("m²", M2),
        ("metro cuadrado", M2),
        ("square meter", M2),
        ("square metre", M2),
        ("sq m", M2),
        ("km2", Km2),
        ("km²", Km2),
        ("kilometro cuadrado", Km2),
        ("square kilometer", Km2),
        ("ha", Ha),
        ("hectarea", Ha),
        ("hectare", Ha),
        ("acre", Acre),
        ("ft2", Ft2),
        ("ft²", Ft2),
        ("pie cuadrado", Ft2),
        ("square foot", Ft2),
        ("square feet", Ft2),
        ("sq ft", Ft2),
        ("sqft", Ft2),
        ("m/s", Mps),
        ("metro por segundo", Mps),
        ("meter per second", Mps),
        ("km/h", Kmh),
        ("kmh", Kmh),
        ("kph", Kmh),
        ("kilometro por hora", Kmh),
        ("kilometer per hour", Kmh),
        ("mph", Mph),
        ("mi/h", Mph),
        ("milla por hora", Mph),
        ("mile per hour", Mph),
        ("nudo", Knot),
        ("knot", Knot),
        ("kn", Knot),
    ]
};

/// Cada palabra en singular por una de las reglas de plural: con `s`,
/// «metros cuadrados» → «metro cuadrado»; con `es`, «galones» → «galon».
fn singular_words(text: &str, suffix: &str) -> String {
    text.split(' ')
        .map(|word| word.strip_suffix(suffix).filter(|w| !w.is_empty()).unwrap_or(word))
        .collect::<Vec<_>>()
        .join(" ")
}

/// El nombre tal cual primero y recién después sus singulares: así «ms» es
/// milisegundo y «mes» es mes, no «m» y «me» en plural.
fn parse_unit(raw: &str) -> Option<Unit> {
    // Sin tildes ni espacios de sobra; el grado es opcional («°C»).
    let key = normalize(raw).replace('°', "");
    let key = key.split_whitespace().collect::<Vec<_>>().join(" ");
    let find = |text: &str| UNIT_NAMES.iter().find(|(name, _)| *name == text).map(|(_, unit)| *unit);
    find(&key)
        .or_else(|| find(&singular_words(&key, "s")))
        .or_else(|| find(&singular_words(&key, "es")))
}

/// Un año gregoriano medio; el mes es su doceava parte (≈ 30,44 días), así
/// «1 año a meses» da 12 justo.
const YEAR_S: f64 = 365.2425 * 86_400.0;

/// Cuántas unidades base vale una unidad (temperatura se resuelve aparte).
fn factor(unit: Unit) -> f64 {
    use Unit::*;
    match unit {
        Mm => 0.001,
        Cm => 0.01,
        M => 1.0,
        Km => 1000.0,
        In => 0.0254,
        Ft => 0.3048,
        Yd => 0.9144,
        Mi => 1609.344,
        Mg => 0.001,
        G => 1.0,
        Kg => 1000.0,
        Oz => 28.349_523_125,
        Lb => 453.592_37,
        Bit => 0.125,
        Byte => 1.0,
        Kb => 1e3,
        Mb => 1e6,
        Gb => 1e9,
        Tb => 1e12,
        Kib => 1024.0,
        Mib => 1_048_576.0,
        Gib => 1_073_741_824.0,
        Tib => 1_099_511_627_776.0,
        Kbit => 125.0,
        Mbit => 125_000.0,
        Gbit => 125_000_000.0,
        Ms => 0.001,
        S => 1.0,
        Min => 60.0,
        H => 3600.0,
        D => 86_400.0,
        Week => 604_800.0,
        Month => YEAR_S / 12.0,
        Year => YEAR_S,
        Celsius | Fahrenheit | Kelvin => 1.0,
        Ml => 0.001,
        Cl => 0.01,
        L => 1.0,
        M3 => 1000.0,
        Gal => 3.785_411_784,
        Tbsp => 0.014_786_764_781_25,
        Tsp => 0.004_928_921_593_75,
        Cup => 0.236_588_236_5,
        FlOz => 0.029_573_529_562_5,
        Cm2 => 0.0001,
        M2 => 1.0,
        Km2 => 1e6,
        Ha => 10_000.0,
        Acre => 4046.856_422_4,
        Ft2 => 0.092_903_04,
        Mps => 1.0,
        Kmh => 1.0 / 3.6,
        Mph => 0.447_04,
        Knot => 1852.0 / 3600.0,
    }
}

/// Hacia la unidad base de la familia.
fn to_base(unit: Unit, value: f64) -> f64 {
    value * factor(unit)
}

fn from_base(unit: Unit, value: f64) -> f64 {
    value / factor(unit)
}

fn to_celsius(unit: Unit, value: f64) -> f64 {
    use Unit::*;
    match unit {
        Fahrenheit => (value - 32.0) * 5.0 / 9.0,
        Kelvin => value - 273.15,
        _ => value,
    }
}

fn from_celsius(unit: Unit, value: f64) -> f64 {
    use Unit::*;
    match unit {
        Fahrenheit => value * 9.0 / 5.0 + 32.0,
        Kelvin => value + 273.15,
        _ => value,
    }
}

fn convert_unit(from: Unit, to: Unit, value: f64) -> Option<f64> {
    if family(from) != family(to) {
        return None;
    }
    if family(from) == 4 {
        return Some(from_celsius(to, to_celsius(from, value)));
    }
    Some(from_base(to, to_base(from, value)))
}

// ---------------------------------------------------------------------------
// Divisas y unidades reajustables (UF, UTM)
// ---------------------------------------------------------------------------

/// Monedas y unidades del conversor, con sus decimales al mostrar.
///
/// La UF y la UTM no son medios de pago, pero el usuario las trata como
/// unidad («1 UF = $41.024»), así que viven acá con el resto. La lista es
/// curada a propósito: cada una tiene que poder nombrarse en los dos idiomas
/// y venir de una fuente que publique su tasa a diario.
const CURRENCIES: &[(&str, u8)] = &[
    ("CLP", 0),
    ("UF", 2),
    ("UTM", 0),
    ("USD", 2),
    ("EUR", 2),
    ("ARS", 2),
    ("BRL", 2),
    ("PEN", 2),
    ("COP", 0),
    ("MXN", 2),
    ("UYU", 2),
    ("BOB", 2),
    ("PYG", 0),
    ("VES", 2),
    ("CAD", 2),
    ("GBP", 2),
    ("CHF", 2),
    ("JPY", 0),
    ("CNY", 2),
    ("AUD", 2),
];

/// Códigos que el conversor conoce (`fx` completa la tabla con ellos).
pub fn currencies() -> &'static [(&'static str, u8)] {
    CURRENCIES
}

fn currency_decimals(code: &str) -> u8 {
    CURRENCIES
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, d)| *d)
        .unwrap_or(2)
}

/// Alias de nombres en español e inglés, sin acentos y en minúsculas.
///
/// «libra» es ambiguo a propósito (masa y libra esterlina): el desempate real
/// lo hace la otra punta de la conversión («1 libra a kg» es masa; «100 libras
/// a clp» es GBP).
const MONEY_ALIASES: &[(&str, &str)] = &[
    ("clp", "CLP"),
    ("peso", "CLP"),
    ("pesos", "CLP"),
    ("peso chileno", "CLP"),
    ("pesos chilenos", "CLP"),
    ("uf", "UF"),
    ("unidad de fomento", "UF"),
    ("unidades de fomento", "UF"),
    ("utm", "UTM"),
    ("unidad tributaria mensual", "UTM"),
    ("unidades tributarias mensuales", "UTM"),
    ("usd", "USD"),
    ("dolar", "USD"),
    ("dolares", "USD"),
    ("dolar observado", "USD"),
    ("dolar estadounidense", "USD"),
    ("dolares estadounidenses", "USD"),
    ("dollar", "USD"),
    ("dollars", "USD"),
    ("us dollar", "USD"),
    ("us dollars", "USD"),
    ("eur", "EUR"),
    ("euro", "EUR"),
    ("euros", "EUR"),
    ("ars", "ARS"),
    ("peso argentino", "ARS"),
    ("pesos argentinos", "ARS"),
    ("brl", "BRL"),
    ("real", "BRL"),
    ("reales", "BRL"),
    ("real brasileno", "BRL"),
    ("reales brasilenos", "BRL"),
    ("pen", "PEN"),
    ("sol", "PEN"),
    ("soles", "PEN"),
    ("sol peruano", "PEN"),
    ("soles peruanos", "PEN"),
    ("cop", "COP"),
    ("peso colombiano", "COP"),
    ("pesos colombianos", "COP"),
    ("mxn", "MXN"),
    ("peso mexicano", "MXN"),
    ("pesos mexicanos", "MXN"),
    ("uyu", "UYU"),
    ("peso uruguayo", "UYU"),
    ("pesos uruguayos", "UYU"),
    ("bob", "BOB"),
    ("boliviano", "BOB"),
    ("bolivianos", "BOB"),
    ("pyg", "PYG"),
    ("guarani", "PYG"),
    ("guaranies", "PYG"),
    ("ves", "VES"),
    ("bolivar", "VES"),
    ("bolivares", "VES"),
    ("cad", "CAD"),
    ("dolar canadiense", "CAD"),
    ("dolares canadienses", "CAD"),
    ("gbp", "GBP"),
    ("libra", "GBP"),
    ("libras", "GBP"),
    ("libra esterlina", "GBP"),
    ("libras esterlinas", "GBP"),
    ("chf", "CHF"),
    ("franco", "CHF"),
    ("francos", "CHF"),
    ("franco suizo", "CHF"),
    ("francos suizos", "CHF"),
    ("jpy", "JPY"),
    ("yen", "JPY"),
    ("yenes", "JPY"),
    ("yen japones", "JPY"),
    ("cny", "CNY"),
    ("yuan", "CNY"),
    ("yuanes", "CNY"),
    ("renminbi", "CNY"),
    ("aud", "AUD"),
    ("dolar australiano", "AUD"),
    ("dolares australianos", "AUD"),
];

/// Clave tolerante: minúsculas, sin acentos y sin espacios de sobra.
fn normalize(raw: &str) -> String {
    raw.trim()
        .chars()
        .flat_map(|c| c.to_lowercase())
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' | 'ã' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' | 'õ' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            other => other,
        })
        .collect()
}

fn parse_money(raw: &str) -> Option<&'static str> {
    let key = normalize(raw);
    MONEY_ALIASES
        .iter()
        .find(|(alias, _)| *alias == key)
        .map(|(_, code)| *code)
}

/// Separadores de conversión. El orden importa: « to » primero para no partir
/// «10 in to cm» por el « in » de la unidad.
const SEPARATORS: &[&str] = &[" to ", "->", " in ", " a ", " en "];

/// Una punta de la conversión, con las dos lecturas posibles cuando el nombre
/// es ambiguo («libra» = masa | GBP; «peso» = CLP).
#[derive(Clone, Copy)]
struct Side {
    value: f64,
    money: Option<&'static str>,
    unit: Option<Unit>,
}

/// `<número> <nombre>` o solo `<nombre>` (vale 1: «uf a clp»).
fn parse_side(text: &str, locale: Locale) -> Option<Side> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let mut parts = text.splitn(2, char::is_whitespace);
    let first = parts.next()?;
    let rest = parts.next().map(str::trim).filter(|s| !s.is_empty());
    let (value, name) = match (parse_amount(first, locale), rest) {
        (Some(value), Some(name)) => (value, name),
        // Un número sin nombre («30») no es conversión.
        (Some(_), None) => return None,
        // Sin número: «uf», «peso chileno», «libra esterlina».
        (None, _) => (1.0, text),
    };
    let money = parse_money(name);
    let unit = parse_unit(name);
    if money.is_none() && unit.is_none() {
        return None;
    }
    Some(Side { value, money, unit })
}

/// Convierte la punta izquierda hacia la derecha. Dinero primero (es el uso
/// más probable en el launcher), pero si no cierra se reintenta como unidades
/// físicas: así «1 libra a kg» no termina como 1 GBP.
fn convert_sides(
    from: Side,
    to: Side,
    rates: Option<&dyn RatesLookup>,
    locale: Locale,
) -> Option<Conversion> {
    if let (Some(from_code), Some(to_code)) = (from.money, to.money) {
        if let Some(hit) = convert_money(from.value, from_code, to_code, rates, locale) {
            return Some(hit);
        }
    }
    match (from.unit, to.unit) {
        (Some(from_unit), Some(to_unit)) => {
            let value = convert_unit(from_unit, to_unit, from.value)?;
            Some(Conversion {
                value: format_number(value)?,
                source: None,
            })
        }
        _ => None,
    }
}

/// `<número> <unidad> (to | in | a | en | ->) <unidad>`, en cualquier caja.
///
/// Cada separador se prueba en orden y, si el candidato no parsea, se sigue
/// con el siguiente: «100 in a cm» corta por « in » y deja «a cm», que no es
/// nada; recién « a » deja las dos puntas buenas.
fn convert_query(
    query: &str,
    rates: Option<&dyn RatesLookup>,
    locale: Locale,
) -> Option<Conversion> {
    let lower = query.to_lowercase().replace('°', "");
    for &sep in SEPARATORS {
        let Some((left, right)) = lower.split_once(sep) else {
            continue;
        };
        let (Some(from), Some(to)) = (parse_side(left, locale), parse_side(right, locale)) else {
            continue;
        };
        if let Some(hit) = convert_sides(from, to, rates, locale) {
            return Some(hit);
        }
    }
    None
}

fn convert_money(
    value: f64,
    from_code: &str,
    to_code: &str,
    rates: Option<&dyn RatesLookup>,
    locale: Locale,
) -> Option<Conversion> {
    if !value.is_finite() {
        return None;
    }
    if from_code == to_code {
        return Some(Conversion {
            value: format_money(value, to_code, locale),
            source: None,
        });
    }
    let rates = rates?;
    let (from_clp, from_date, from_origin) = rates.clp_per_unit(from_code)?;
    let (to_clp, to_date, to_origin) = rates.clp_per_unit(to_code)?;
    if from_clp <= 0.0 || to_clp <= 0.0 {
        return None;
    }
    let converted = value * from_clp / to_clp;
    if !converted.is_finite() {
        return None;
    }
    Some(Conversion {
        value: format_money(converted, to_code, locale),
        source: Some(rate_source(
            from_code,
            from_date,
            from_origin,
            to_code,
            to_date,
            to_origin,
            rates.freshness(),
            locale,
        )),
    })
}

/// Monto con separadores del idioma activo y el código detrás: «35.500 CLP».
///
/// Si el redondeo a los decimales de la moneda diera 0 sin serlo, se agregan
/// decimales en vez de mentir con «0,00»; si ni así alcanza, notación
/// científica.
fn format_money(value: f64, code: &str, locale: Locale) -> String {
    let mut decimals = currency_decimals(code) as usize;
    while decimals <= 8 {
        let text = money_text(value, decimals, locale);
        if !is_zero_money(&text, locale) || value == 0.0 {
            return format!("{text} {code}");
        }
        decimals += 1;
    }
    format!("{value:e} {code}")
}

fn money_text(value: f64, decimals: usize, locale: Locale) -> String {
    let raw = format!("{value:.decimals$}");
    let (sign, body) = match raw.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", raw.as_str()),
    };
    let (int_part, dec_part) = match body.split_once('.') {
        Some((i, d)) => (i, Some(d)),
        None => (body, None),
    };
    let (group_sep, dec_sep) = match locale {
        Locale::Es => ('.', ','),
        Locale::En => (',', '.'),
    };
    let grouped = insert_groups(int_part, group_sep);
    match dec_part {
        Some(dec) => format!("{sign}{grouped}{dec_sep}{dec}"),
        None => format!("{sign}{grouped}"),
    }
}

fn insert_groups(digits: &str, sep: char) -> String {
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(c);
    }
    out
}

/// ¿El texto representa cero? Se ignoran separadores y el signo.
fn is_zero_money(text: &str, locale: Locale) -> bool {
    let (group, decimal) = match locale {
        Locale::Es => ('.', ','),
        Locale::En => (',', '.'),
    };
    text.trim_start_matches(['-', '+'])
        .chars()
        .all(|c| c == '0' || c == group || c == decimal)
}

/// Procedencia de la tasa, para que el número nunca vaya solo.
///
/// CLP es la base de la tabla: su «fecha» no es una tasa y no se muestra.
/// Cuando participa una sola tasa con fecha (USD → CLP), manda esa fecha;
/// cuando participan dos, se nombran las dos.
// Dos monedas × (código, fecha, origen) más frescura e idioma: agruparlos en
// un struct solo movería la lista de lugar.
#[allow(clippy::too_many_arguments)]
fn rate_source(
    from_code: &str,
    from_date: &str,
    from_origin: &str,
    to_code: &str,
    to_date: &str,
    to_origin: &str,
    freshness: Freshness,
    locale: Locale,
) -> String {
    let (official, date_text) = match (from_code == "CLP", to_code == "CLP") {
        (true, _) => (to_origin == "bcch", day_label(to_date, locale)),
        (_, true) => (from_origin == "bcch", day_label(from_date, locale)),
        _ => (
            from_origin == "bcch" && to_origin == "bcch",
            dates_label(from_date, to_date, locale),
        ),
    };
    let label = match (official, locale) {
        (true, Locale::Es) => "valor oficial",
        (true, Locale::En) => "official value",
        (false, Locale::Es) => "tipo de cambio",
        (false, Locale::En) => "exchange rate",
    };
    let mut text = match locale {
        Locale::Es => format!("{label} del {date_text}"),
        Locale::En => format!("{label}, {date_text}"),
    };
    match freshness {
        Freshness::Fresh => {}
        Freshness::Stale => text.push_str(match locale {
            Locale::Es => " · datos antiguos",
            Locale::En => " · outdated data",
        }),
        Freshness::Offline => text.push_str(match locale {
            Locale::Es => " · sin conexión",
            Locale::En => " · offline",
        }),
    }
    text
}

fn month_name(month: usize, locale: Locale) -> Option<&'static str> {
    const MONTHS_ES: [&str; 12] = [
        "ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic",
    ];
    const MONTHS_EN: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    if !(1..=12).contains(&month) {
        return None;
    }
    Some(match locale {
        Locale::Es => MONTHS_ES[month - 1],
        Locale::En => MONTHS_EN[month - 1],
    })
}

/// `2026-09-25` → «25 sep» / «Sep 25». Si no es ISO, se devuelve tal cual.
fn day_label(iso: &str, locale: Locale) -> String {
    let mut parts = iso.split('-');
    let (Some(year), Some(month), Some(day)) = (parts.next(), parts.next(), parts.next()) else {
        return iso.to_string();
    };
    let (Ok(month), Ok(day)) = (month.parse::<usize>(), day.parse::<u32>()) else {
        return iso.to_string();
    };
    let Some(name) = month_name(month, locale) else {
        return iso.to_string();
    };
    let _ = year;
    match locale {
        Locale::Es => format!("{day} {name}"),
        Locale::En => format!("{name} {day}"),
    }
}

/// «25 y 26 sep» cuando es el mismo mes; si no, «30 sep y 1 oct».
fn dates_label(from: &str, to: &str, locale: Locale) -> String {
    if from == to {
        return day_label(from, locale);
    }
    let same_month = from.get(0..7) == to.get(0..7);
    if same_month {
        let from_day = from.get(8..).unwrap_or(from).trim_start_matches('0');
        let to_day = to.get(8..).unwrap_or(to).trim_start_matches('0');
        let month = from
            .get(5..7)
            .and_then(|m| m.parse::<usize>().ok())
            .and_then(|m| month_name(m, locale))
            .unwrap_or("");
        return match locale {
            Locale::Es => format!("{from_day} y {to_day} {month}"),
            Locale::En => format!("{month} {from_day} and {to_day}"),
        };
    }
    match locale {
        Locale::Es => format!("{} y {}", day_label(from, locale), day_label(to, locale)),
        Locale::En => format!("{} and {}", day_label(from, locale), day_label(to, locale)),
    }
}

// ---------------------------------------------------------------------------
// Zonas horarias: «15:00 chile en madrid», «3pm santiago a tokio»,
// «hora en tokio», «tokio hora», «15:00 en tokio».
// ---------------------------------------------------------------------------

use chrono::{DateTime, Datelike, Duration, NaiveTime, Offset, TimeZone, Timelike, Utc, Weekday};
use chrono_tz::Tz;

/// Un lugar con su zona IANA y cómo se nombra en cada idioma.
struct Place {
    tz: Tz,
    es: &'static str,
    en: &'static str,
    /// Cómo se escribe: minúsculas y sin tildes (se compara con `normalize`).
    aliases: &'static [&'static str],
}

/// Tabla curada a mano: ciudades y países que se escriben en el lanzador,
/// en los dos idiomas. Un país con varias zonas toma la de su capital o su
/// ciudad más poblada (Chile = Santiago, EE. UU. no tiene entrada propia).
const PLACES: &[Place] = {
    use chrono_tz::{America, Asia, Australia, Europe, Pacific, Africa};
    &[
        Place { tz: America::Santiago, es: "Chile", en: "Chile", aliases: &["chile", "santiago", "santiago de chile", "scl"] },
        Place { tz: America::Punta_Arenas, es: "Punta Arenas", en: "Punta Arenas", aliases: &["punta arenas", "magallanes"] },
        Place { tz: Pacific::Easter, es: "Isla de Pascua", en: "Easter Island", aliases: &["isla de pascua", "rapa nui", "easter island"] },
        Place { tz: Europe::Madrid, es: "Madrid", en: "Madrid", aliases: &["madrid", "espana", "spain", "barcelona"] },
        Place { tz: Asia::Tokyo, es: "Tokio", en: "Tokyo", aliases: &["tokio", "tokyo", "japon", "japan"] },
        Place { tz: America::New_York, es: "Nueva York", en: "New York", aliases: &["nueva york", "new york", "nyc", "ny", "miami", "washington", "boston"] },
        Place { tz: America::Chicago, es: "Chicago", en: "Chicago", aliases: &["chicago"] },
        Place { tz: America::Denver, es: "Denver", en: "Denver", aliases: &["denver"] },
        Place { tz: America::Los_Angeles, es: "Los Ángeles", en: "Los Angeles", aliases: &["los angeles", "san francisco", "california", "seattle"] },
        Place { tz: Pacific::Honolulu, es: "Honolulu", en: "Honolulu", aliases: &["honolulu", "hawai", "hawaii"] },
        Place { tz: America::Toronto, es: "Toronto", en: "Toronto", aliases: &["toronto", "canada", "montreal"] },
        Place { tz: America::Vancouver, es: "Vancouver", en: "Vancouver", aliases: &["vancouver"] },
        Place { tz: Europe::London, es: "Londres", en: "London", aliases: &["londres", "london", "reino unido", "uk", "inglaterra", "england"] },
        Place { tz: Europe::Dublin, es: "Dublín", en: "Dublin", aliases: &["dublin", "irlanda", "ireland"] },
        Place { tz: Europe::Lisbon, es: "Lisboa", en: "Lisbon", aliases: &["lisboa", "lisbon", "portugal"] },
        Place { tz: Europe::Paris, es: "París", en: "Paris", aliases: &["paris", "francia", "france"] },
        Place { tz: Europe::Berlin, es: "Berlín", en: "Berlin", aliases: &["berlin", "alemania", "germany"] },
        Place { tz: Europe::Rome, es: "Roma", en: "Rome", aliases: &["roma", "rome", "italia", "italy"] },
        Place { tz: Europe::Amsterdam, es: "Ámsterdam", en: "Amsterdam", aliases: &["amsterdam", "holanda", "paises bajos", "netherlands"] },
        Place { tz: Europe::Zurich, es: "Zúrich", en: "Zurich", aliases: &["zurich", "suiza", "switzerland"] },
        Place { tz: Europe::Stockholm, es: "Estocolmo", en: "Stockholm", aliases: &["estocolmo", "stockholm", "suecia", "sweden"] },
        Place { tz: Europe::Athens, es: "Atenas", en: "Athens", aliases: &["atenas", "athens", "grecia", "greece"] },
        Place { tz: Europe::Istanbul, es: "Estambul", en: "Istanbul", aliases: &["estambul", "istanbul", "turquia", "turkey"] },
        Place { tz: Europe::Moscow, es: "Moscú", en: "Moscow", aliases: &["moscu", "moscow", "rusia", "russia"] },
        Place { tz: Africa::Cairo, es: "El Cairo", en: "Cairo", aliases: &["el cairo", "cairo", "egipto", "egypt"] },
        Place { tz: Africa::Johannesburg, es: "Johannesburgo", en: "Johannesburg", aliases: &["johannesburgo", "johannesburg", "sudafrica", "south africa"] },
        Place { tz: America::Mexico_City, es: "Ciudad de México", en: "Mexico City", aliases: &["ciudad de mexico", "cdmx", "mexico", "mexico city", "df"] },
        Place { tz: America::Bogota, es: "Bogotá", en: "Bogota", aliases: &["bogota", "colombia", "medellin"] },
        Place { tz: America::Lima, es: "Lima", en: "Lima", aliases: &["lima", "peru"] },
        Place { tz: America::Guayaquil, es: "Quito", en: "Quito", aliases: &["quito", "ecuador", "guayaquil"] },
        Place { tz: America::Caracas, es: "Caracas", en: "Caracas", aliases: &["caracas", "venezuela"] },
        Place { tz: America::La_Paz, es: "La Paz", en: "La Paz", aliases: &["la paz", "bolivia"] },
        Place { tz: America::Asuncion, es: "Asunción", en: "Asuncion", aliases: &["asuncion", "paraguay"] },
        Place { tz: America::Montevideo, es: "Montevideo", en: "Montevideo", aliases: &["montevideo", "uruguay"] },
        Place { tz: America::Argentina::Buenos_Aires, es: "Buenos Aires", en: "Buenos Aires", aliases: &["buenos aires", "argentina"] },
        Place { tz: America::Sao_Paulo, es: "São Paulo", en: "São Paulo", aliases: &["sao paulo", "brasil", "brazil", "rio de janeiro", "rio"] },
        Place { tz: America::Panama, es: "Panamá", en: "Panama", aliases: &["panama"] },
        Place { tz: America::Costa_Rica, es: "Costa Rica", en: "Costa Rica", aliases: &["costa rica", "san jose"] },
        Place { tz: Asia::Shanghai, es: "Shanghái", en: "Shanghai", aliases: &["shanghai", "china", "pekin", "beijing"] },
        Place { tz: Asia::Hong_Kong, es: "Hong Kong", en: "Hong Kong", aliases: &["hong kong"] },
        Place { tz: Asia::Taipei, es: "Taipéi", en: "Taipei", aliases: &["taipei", "taiwan"] },
        Place { tz: Asia::Seoul, es: "Seúl", en: "Seoul", aliases: &["seul", "seoul", "corea", "corea del sur", "korea"] },
        Place { tz: Asia::Singapore, es: "Singapur", en: "Singapore", aliases: &["singapur", "singapore"] },
        Place { tz: Asia::Bangkok, es: "Bangkok", en: "Bangkok", aliases: &["bangkok", "tailandia", "thailand"] },
        Place { tz: Asia::Kolkata, es: "India", en: "India", aliases: &["india", "nueva delhi", "delhi", "new delhi", "mumbai", "bombay", "bangalore"] },
        Place { tz: Asia::Dubai, es: "Dubái", en: "Dubai", aliases: &["dubai", "emiratos", "uae"] },
        Place { tz: Asia::Jerusalem, es: "Jerusalén", en: "Jerusalem", aliases: &["jerusalen", "jerusalem", "israel", "tel aviv"] },
        Place { tz: Australia::Sydney, es: "Sídney", en: "Sydney", aliases: &["sidney", "sydney", "australia", "melbourne"] },
        Place { tz: Australia::Perth, es: "Perth", en: "Perth", aliases: &["perth"] },
        Place { tz: Pacific::Auckland, es: "Auckland", en: "Auckland", aliases: &["auckland", "nueva zelanda", "new zealand"] },
        Place { tz: Tz::UTC, es: "UTC", en: "UTC", aliases: &["utc", "gmt", "zulu"] },
    ]
};

fn find_place(text: &str) -> Option<&'static Place> {
    let key = normalize(text);
    let key = key.split_whitespace().collect::<Vec<_>>().join(" ");
    if key.is_empty() {
        return None;
    }
    PLACES.iter().find(|place| place.aliases.contains(&key.as_str()))
}

/// La zona del sistema («America/Santiago»), si se puede saber.
fn local_zone() -> Option<Tz> {
    iana_time_zone::get_timezone().ok()?.parse().ok()
}

/// Una hora escrita a mano: «15:00», «15h», «3pm», «3:30 p.m.», «mediodía».
/// Una hora pelada («15») solo vale si `bare` lo permite: sola, en «15 en
/// tokio», es demasiado parecida a un número cualquiera.
fn parse_clock(text: &str, bare: bool) -> Option<NaiveTime> {
    let compact: String = normalize(text).chars().filter(|c| !c.is_whitespace()).collect();
    match compact.as_str() {
        "mediodia" | "noon" => return NaiveTime::from_hms_opt(12, 0, 0),
        "medianoche" | "midnight" => return NaiveTime::from_hms_opt(0, 0, 0),
        _ => {}
    }
    let (body, meridiem, marked) = if let Some(rest) = compact.strip_suffix("a.m.").or_else(|| compact.strip_suffix("am")) {
        (rest, Some(false), true)
    } else if let Some(rest) = compact.strip_suffix("p.m.").or_else(|| compact.strip_suffix("pm")) {
        (rest, Some(true), true)
    } else if let Some(rest) = ["hrs", "hr", "h"].iter().find_map(|suffix| compact.strip_suffix(suffix)) {
        (rest, None, true)
    } else {
        (compact.as_str(), None, false)
    };
    let (hour, minute) = match body.split_once(':') {
        Some((h, m)) if m.len() == 2 => (h, m),
        Some(_) => return None,
        None if marked || bare => (body, "00"),
        None => return None,
    };
    if hour.is_empty() || hour.len() > 2 || !hour.chars().all(|c| c.is_ascii_digit()) || !minute.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let (mut hour, minute) = (hour.parse::<u32>().ok()?, minute.parse::<u32>().ok()?);
    if let Some(pm) = meridiem {
        if !(1..=12).contains(&hour) {
            return None;
        }
        hour = match (hour, pm) {
            (12, false) => 0,
            (12, true) => 12,
            (h, true) => h + 12,
            (h, false) => h,
        };
    }
    NaiveTime::from_hms_opt(hour, minute, 0)
}

/// Esa hora, hoy en esa zona. En el salto del horario de verano (una hora
/// que no existe) se corre una hora; en la que se repite, la primera.
fn at_clock(tz: Tz, clock: NaiveTime, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let date = now.with_timezone(&tz).date_naive();
    let naive = date.and_time(clock);
    tz.from_local_datetime(&naive)
        .earliest()
        .or_else(|| tz.from_local_datetime(&(naive + Duration::hours(1))).earliest())
        .map(|time| time.with_timezone(&Utc))
}

/// Segundos que la zona está adelantada respecto de UTC en ese instante.
fn offset_secs(tz: Tz, at: DateTime<Utc>) -> i32 {
    tz.offset_from_utc_datetime(&at.naive_utc()).fix().local_minus_utc()
}

fn weekday_name(day: Weekday, locale: Locale) -> &'static str {
    let i = day.num_days_from_monday() as usize;
    match locale {
        Locale::Es => ["lunes", "martes", "miércoles", "jueves", "viernes", "sábado", "domingo"][i],
        Locale::En => ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"][i],
    }
}

/// «+12 h», «-3 h», «+8:30 h»; sin diferencia, «misma hora».
fn offset_label(diff_secs: i32, locale: Locale) -> String {
    if diff_secs == 0 {
        return match locale {
            Locale::Es => "misma hora".into(),
            Locale::En => "same time".into(),
        };
    }
    let sign = if diff_secs < 0 { '-' } else { '+' };
    let minutes = diff_secs.unsigned_abs() / 60;
    match minutes % 60 {
        0 => format!("{sign}{} h", minutes / 60),
        rest => format!("{sign}{}:{rest:02} h", minutes / 60),
    }
}

/// Lo que se copia es la hora sola («08:00»); el día, el lugar y la
/// diferencia van en la procedencia: «sábado (Tokio, +12 h)».
fn zone_result(at: DateTime<Utc>, to: &Place, from: Option<Tz>, locale: Locale) -> Conversion {
    let there = at.with_timezone(&to.tz);
    let name = match locale {
        Locale::Es => to.es,
        Locale::En => to.en,
    };
    let detail = match from {
        Some(from) => format!("{name}, {}", offset_label(offset_secs(to.tz, at) - offset_secs(from, at), locale)),
        None => name.to_string(),
    };
    Conversion {
        value: format!("{:02}:{:02}", there.hour(), there.minute()),
        source: Some(format!("{} ({detail})", weekday_name(there.weekday(), locale))),
    }
}

/// Las formas de pedir la hora actual: «hora en tokio», «what time is it in».
const NOW_WORDS: &[&str] = &["hora", "la hora", "hora actual", "que hora es", "time", "the time", "current time", "what time is it", "ahora"];

/// Las consultas de zona horaria, con el «ahora» y la zona local de
/// parámetro para poder probarlas. `None` si no es una de estas formas.
fn zone_query(query: &str, now: DateTime<Utc>, local: Option<Tz>, locale: Locale) -> Option<Conversion> {
    let text = normalize(query).replace(['¿', '?'], "");
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");

    // «tokio hora», «hora tokio», «tokyo time»: la hora actual allá.
    for word in ["hora", "time", "ahora"] {
        let place = text
            .strip_suffix(word)
            .and_then(|rest| rest.strip_suffix(' '))
            .or_else(|| text.strip_prefix(word).and_then(|rest| rest.strip_prefix(' ')))
            .and_then(find_place);
        if let Some(place) = place {
            return Some(zone_result(now, place, local, locale));
        }
    }

    for sep in [" en ", " a ", " to ", " in ", " -> ", "->"] {
        for (at, _) in text.match_indices(sep) {
            let (left, right) = (text[..at].trim(), text[at + sep.len()..].trim());
            let Some(to) = find_place(right) else {
                continue;
            };
            // «hora en tokio»
            if NOW_WORDS.contains(&left) {
                return Some(zone_result(now, to, local, locale));
            }
            // «15:00 chile en madrid» o «15:00 en tokio» (desde la local). La
            // hora más larga que calce: «3 pm chile» es «3 pm» + «chile».
            let words: Vec<&str> = left.split(' ').collect();
            for split in (1..=words.len()).rev() {
                let (clock, zone) = (words[..split].join(" "), words[split..].join(" "));
                let from = if zone.is_empty() { local } else { find_place(&zone).map(|place| place.tz) };
                let Some(from) = from else {
                    continue;
                };
                let Some(clock) = parse_clock(&clock, !zone.is_empty()) else {
                    continue;
                };
                let instant = at_clock(from, clock, now)?;
                return Some(zone_result(instant, to, Some(from), locale));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tasas fijas para probar sin red: mismo formato que publica `fx`.
    struct TestRates {
        entries: Vec<(&'static str, f64, &'static str, &'static str)>,
        freshness: Freshness,
    }

    impl Default for TestRates {
        fn default() -> Self {
            Self {
                entries: vec![
                    ("CLP", 1.0, "2026-09-26", "bcch"),
                    ("UF", 41_024.46, "2026-09-26", "bcch"),
                    ("UTM", 71_721.0, "2026-09-01", "bcch"),
                    ("USD", 965.71, "2026-09-25", "bcch"),
                    ("EUR", 1098.15, "2026-09-25", "bcch"),
                    ("ARS", 1523.96, "2026-09-25", "market"),
                    ("GBP", 1268.4, "2026-09-25", "market"),
                ],
                freshness: Freshness::Fresh,
            }
        }
    }

    impl RatesLookup for TestRates {
        fn clp_per_unit(&self, code: &str) -> Option<(f64, &str, &str)> {
            self.entries
                .iter()
                .find(|(c, ..)| *c == code)
                .map(|(_, value, date, origin)| (*value, *date, *origin))
        }

        fn freshness(&self) -> Freshness {
            self.freshness
        }
    }

    fn value(query: &str) -> Option<String> {
        evaluate_with(query, None, Locale::Es).map(|hit| hit.value)
    }

    fn value_with(query: &str, locale: Locale) -> Option<String> {
        evaluate_with(query, Some(&TestRates::default()), locale).map(|hit| hit.value)
    }

    #[test]
    fn precedencia_y_parentesis() {
        assert_eq!(value("2+3*4").as_deref(), Some("14"));
        assert_eq!(value("(2+3)*4").as_deref(), Some("20"));
        assert_eq!(value("10-2-3").as_deref(), Some("5"));
        assert_eq!(value("2^3^2").as_deref(), Some("512"));
    }

    #[test]
    fn unarios_y_negativos() {
        assert_eq!(value("-4^2").as_deref(), Some("-16"));
        assert_eq!(value("(-4)^2").as_deref(), Some("16"));
        assert_eq!(value("+7").as_deref(), Some("7"));
        assert_eq!(value("3*-2").as_deref(), Some("-6"));
    }

    #[test]
    fn modulo_y_decimales() {
        assert_eq!(value("10%3").as_deref(), Some("1"));
        assert_eq!(value("0.1+0.2").as_deref(), Some("0.3"));
        assert_eq!(value("1_000/4").as_deref(), Some("250"));
        // En español la coma también es decimal en aritmética.
        assert_eq!(value_with("1,5+1,5", Locale::Es).as_deref(), Some("3"));
    }

    #[test]
    fn conversion_de_unidades() {
        assert_eq!(value("10 km to mi").as_deref(), Some("6.213712"));
        assert_eq!(value("30 c to f").as_deref(), Some("86"));
        assert_eq!(value("2 gb to mb").as_deref(), Some("2000"));
        assert_eq!(value("1 h to min").as_deref(), Some("60"));
        assert_eq!(value("1 in to cm").as_deref(), Some("2.54"));
        assert_eq!(value("100 f to c").as_deref(), Some("37.777778"));
        // Español: «a» y «en» son los separadores naturales.
        assert_eq!(value("10 km a mi").as_deref(), Some("6.213712"));
        assert_eq!(value("1 hora en minutos").as_deref(), Some("60"));
        // El separador no gana por aparecer antes si el candidato no parsea.
        assert_eq!(value("100 in a cm").as_deref(), Some("254"));
    }

    #[test]
    fn rechaza_lo_que_no_es_una_cuenta() {
        assert_eq!(value(""), None);
        assert_eq!(value("abc"), None);
        assert_eq!(value("1 km to kg"), None); // familias distintas
        assert_eq!(value("1 km to nope"), None); // unidad desconocida
        assert_eq!(value("1/0"), None);
        assert_eq!(value("10%0"), None);
        assert_eq!(value("2 chrome"), None); // sobra texto
        assert_eq!(value("lock"), None);
        assert_eq!(value("(2+3"), None);
    }

    #[test]
    fn no_miente_con_ceros_redondeados() {
        assert_eq!(value("0.0000001").as_deref(), Some("1e-7"));
        assert_eq!(value("0").as_deref(), Some("0"));
        assert_eq!(value("2-2").as_deref(), Some("0"));
    }

    #[test]
    fn montos_con_separadores_de_los_dos_formatos() {
        // es-CL: punto de miles, coma decimal.
        assert_eq!(
            value_with("30.000 CLP a CLP", Locale::Es).as_deref(),
            Some("30.000 CLP")
        );
        assert_eq!(
            value_with("1.234,60 CLP a CLP", Locale::Es).as_deref(),
            Some("1.235 CLP") // CLP no muestra centavos
        );
        // en-US: al revés.
        assert_eq!(
            value_with("30,000 CLP to CLP", Locale::En).as_deref(),
            Some("30,000 CLP")
        );
        assert_eq!(
            value_with("1,234.50 USD to USD", Locale::En).as_deref(),
            Some("1,234.50 USD")
        );
        // El separador visual viejo sigue vivo.
        assert_eq!(
            value_with("1_000 CLP a CLP", Locale::Es).as_deref(),
            Some("1.000 CLP")
        );
    }

    #[test]
    fn el_monto_ambiguo_sigue_el_idioma() {
        // «1.234» es 1234 en es y 1,234 en en.
        assert_eq!(
            value_with("1.234 CLP a CLP", Locale::Es).as_deref(),
            Some("1.234 CLP")
        );
        assert_eq!(
            value_with("1.234 CLP to CLP", Locale::En).as_deref(),
            Some("1 CLP") // 1,234 CLP redondeado a cero decimales
        );
        // Y al revés con la coma.
        assert_eq!(
            value_with("30,000 CLP a CLP", Locale::Es).as_deref(),
            Some("30 CLP") // en es la coma es decimal: 30,000 = 30,0
        );
        assert_eq!(
            value_with("30,000 CLP to CLP", Locale::En).as_deref(),
            Some("30,000 CLP")
        );
    }

    #[test]
    fn convierte_divisas_con_las_tasas_recibidas() {
        // 30 USD × 965,71 = 28.971,30 CLP (sin centavos).
        assert_eq!(
            value_with("30 USD a CLP", Locale::Es).as_deref(),
            Some("28.971 CLP")
        );
        assert_eq!(
            value_with("30 USD to CLP", Locale::En).as_deref(),
            Some("28,971 CLP")
        );
        // 30.000 CLP ÷ 41.024,46 = 0,7313… UF.
        assert_eq!(
            value_with("30.000 CLP a UF", Locale::Es).as_deref(),
            Some("0,73 UF")
        );
        // Sin número, vale 1: «uf a clp» es la cotización de hoy.
        assert_eq!(
            value_with("uf a clp", Locale::Es).as_deref(),
            Some("41.024 CLP")
        );
        assert_eq!(
            value_with("dolar a clp", Locale::Es).as_deref(),
            Some("966 CLP")
        );
    }

    #[test]
    fn la_procedencia_de_la_tasa_acompana_al_resultado() {
        let hit = evaluate_with("30 USD a CLP", Some(&TestRates::default()), Locale::Es).unwrap();
        // CLP es la base: la única tasa con fecha es la del dólar observado.
        assert_eq!(hit.source.as_deref(), Some("valor oficial del 25 sep"));

        let hit =
            evaluate_with("30.000 CLP a ARS", Some(&TestRates::default()), Locale::Es).unwrap();
        assert_eq!(hit.source.as_deref(), Some("tipo de cambio del 25 sep"));

        // UF (26) y dólar (25) tienen fechas distintas: se nombran las dos.
        let hit = evaluate_with("1 UF a USD", Some(&TestRates::default()), Locale::Es).unwrap();
        assert_eq!(hit.source.as_deref(), Some("valor oficial del 26 y 25 sep"));

        let hit = evaluate_with("1 UF to USD", Some(&TestRates::default()), Locale::En).unwrap();
        assert_eq!(hit.source.as_deref(), Some("official value, Sep 26 and 25"));
    }

    #[test]
    fn avisa_cuando_las_tasas_estan_viejas_o_no_hay_red() {
        let stale = TestRates {
            freshness: Freshness::Stale,
            ..TestRates::default()
        };
        let hit = evaluate_with("30 USD a CLP", Some(&stale), Locale::Es).unwrap();
        assert_eq!(
            hit.source.as_deref(),
            Some("valor oficial del 25 sep · datos antiguos")
        );

        let offline = TestRates {
            freshness: Freshness::Offline,
            ..TestRates::default()
        };
        let hit = evaluate_with("30 USD to CLP", Some(&offline), Locale::En).unwrap();
        assert_eq!(
            hit.source.as_deref(),
            Some("official value, Sep 25 · offline")
        );
    }

    #[test]
    fn sin_tasas_no_hay_conversion_pero_si_aviso() {
        assert_eq!(evaluate_with("30 USD a CLP", None, Locale::Es), None);
        assert!(money_conversion("30 USD a CLP", Locale::Es));
        assert!(money_conversion("30.000 CLP to UF", Locale::En));
        assert!(money_conversion("100 libras a clp", Locale::Es));
        // Una unidad física no es una conversión de divisas.
        assert!(!money_conversion("10 km a mi", Locale::Es));
        assert!(!money_conversion("2+3", Locale::Es));
        assert!(!money_conversion("lock", Locale::Es));
        // Moneda desconocida: no se ofrece nada.
        assert!(!money_conversion("30 XYZ a CLP", Locale::Es));
    }

    #[test]
    fn libra_es_masa_o_es_esterlina_segun_la_otra_punta() {
        assert_eq!(value("1 libra a kg").as_deref(), Some("0.453592"));
        assert_eq!(
            value_with("100 libras a clp", Locale::Es).as_deref(),
            Some("126.840 CLP")
        );
        // «100 gbp a lb» no cierra por ningún lado: mejor nada que un invento.
        assert_eq!(value_with("100 gbp a lb", Locale::Es), None);
    }

    #[test]
    fn no_mezcla_dinero_con_unidades() {
        assert_eq!(value_with("5 USD a kg", Locale::Es), None);
        assert_eq!(value_with("2 km a CLP", Locale::Es), None);
        assert_eq!(value_with("30 USD a XYZ", Locale::Es), None);
    }

    #[test]
    fn la_misma_moneda_no_necesita_tasas() {
        assert_eq!(value("30 USD a USD").as_deref(), Some("30,00 USD"));
        let hit = evaluate_with("30 USD a USD", None, Locale::Es).unwrap();
        assert_eq!(hit.source, None);
    }

    #[test]
    fn tampoco_miente_con_ceros_en_divisas() {
        // 0,000001 USD son 0,000966 CLP: no es «0 CLP».
        let text = value_with("0,000001 USD a CLP", Locale::Es).unwrap();
        assert_ne!(text, "0 CLP");
        assert!(text.ends_with("CLP"));
        assert!(text.starts_with("0,00"));
    }

    #[test]
    fn montos_invalidos_se_rechazan() {
        assert_eq!(value_with("30..000 CLP a UF", Locale::Es), None);
        assert_eq!(value_with("3,4,5 USD a CLP", Locale::Es), None);
        // Dos decimales o grupos que no miden 3: mejor nada que un monto inventado.
        assert_eq!(value_with("1.2,3.4 USD a CLP", Locale::Es), None);
        assert_eq!(value_with("1.234,56.7 USD a CLP", Locale::Es), None);
        assert_eq!(evaluate_with("+ USD a CLP", None, Locale::Es), None);
    }

    #[test]
    fn volumen_area_y_velocidad() {
        assert_eq!(value("1 l a ml").as_deref(), Some("1000"));
        assert_eq!(value("33 cl a l").as_deref(), Some("0.33"));
        assert_eq!(value("1 gal a litros").as_deref(), Some("3.785412"));
        assert_eq!(value("3 cucharaditas a cucharadas").as_deref(), Some("1"));
        assert_eq!(value("1 taza a ml").as_deref(), Some("236.588236"));
        assert_eq!(value("8 fl oz a tazas").as_deref(), Some("1"));
        assert_eq!(value("2 onzas liquidas a ml").as_deref(), Some("59.147059"));
        assert_eq!(value("1 ha a m2").as_deref(), Some("10000"));
        assert_eq!(value("1 km² a hectareas").as_deref(), Some("100"));
        assert_eq!(value("100 metros cuadrados a ft2").as_deref(), Some("1076.391042"));
        assert_eq!(value("1 acre to m²").as_deref(), Some("4046.856422"));
        assert_eq!(value("1 m2 a cm2").as_deref(), Some("10000"));
        assert_eq!(value("100 km/h a mph").as_deref(), Some("62.137119"));
        assert_eq!(value("10 m/s a km/h").as_deref(), Some("36"));
        assert_eq!(value("1 nudo a km/h").as_deref(), Some("1.852"));
        assert_eq!(value("20 knots to mph").as_deref(), Some("23.015589"));
    }

    #[test]
    fn tiempo_largo_y_datos_binarios() {
        assert_eq!(value("2 semanas a dias").as_deref(), Some("14"));
        assert_eq!(value("1 año a meses").as_deref(), Some("12"));
        assert_eq!(value("1 mes a dias").as_deref(), Some("30.436875"));
        assert_eq!(value("1 year to weeks").as_deref(), Some("52.1775"));
        // «ms» es milisegundo y «mes» es mes: ninguno es «m» en plural.
        assert_eq!(value("1500 ms a s").as_deref(), Some("1.5"));
        assert_eq!(value("2 meses a semanas").as_deref(), Some("8.69625"));
        assert_eq!(value("90 min a h").as_deref(), Some("1.5"));
        assert_eq!(value("1 h a s").as_deref(), Some("3600"));
        assert_eq!(value("1 gib a mb").as_deref(), Some("1073.741824"));
        assert_eq!(value("1024 kib a mib").as_deref(), Some("1"));
        assert_eq!(value("1 tib a gib").as_deref(), Some("1024"));
        assert_eq!(value("100 mbps a mb").as_deref(), Some("12.5"));
        assert_eq!(value("1 gbit a mbit").as_deref(), Some("1000"));
        assert_eq!(value("8 bits a bytes").as_deref(), Some("1"));
        // Lo de siempre no cambia.
        assert_eq!(value("1 libra a kg").as_deref(), Some("0.453592"));
        assert_eq!(value("1 l a kg"), None);
        assert_eq!(value("1 m2 a m"), None);
    }

    fn utc(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text).unwrap().with_timezone(&Utc)
    }

    fn zone(query: &str, now: &str) -> Option<(String, String)> {
        zone_query(query, utc(now), Some(chrono_tz::America::Santiago), Locale::Es)
            .map(|hit| (hit.value, hit.source.unwrap_or_default()))
    }

    #[test]
    fn convierte_horas_entre_zonas() {
        // 17 oct 2026: Chile en horario de verano (-3), Tokio +9.
        let now = "2026-10-17T15:00:00Z";
        assert_eq!(
            zone("15:00 chile en tokio", now),
            Some(("03:00".into(), "domingo (Tokio, +12 h)".into()))
        );
        assert_eq!(zone("3pm santiago a tokio", now).map(|z| z.0).as_deref(), Some("03:00"));
        assert_eq!(zone("3 pm Santiago a Tokio", now).map(|z| z.0).as_deref(), Some("03:00"));
        assert_eq!(zone("15 chile en madrid", now).map(|z| z.0).as_deref(), Some("20:00"));
        // Desde la zona local (aquí, Chile).
        assert_eq!(zone("15:00 en tokio", now).map(|z| z.0).as_deref(), Some("03:00"));
        // India va con media hora.
        assert_eq!(
            zone("12:00 utc en india", now),
            Some(("17:30".into(), "sábado (India, +5:30 h)".into()))
        );
        assert_eq!(zone("9am nueva york a londres", now).map(|z| z.0).as_deref(), Some("14:00"));
        assert_eq!(zone("mediodía São Paulo a Lima", now).map(|z| z.0).as_deref(), Some("10:00"));
    }

    #[test]
    fn la_hora_actual_en_otra_zona() {
        let now = "2026-10-17T15:00:00Z";
        let tokio = Some(("00:00".into(), "domingo (Tokio, +12 h)".into()));
        assert_eq!(zone("hora en tokio", now), tokio);
        assert_eq!(zone("tokio hora", now), tokio);
        assert_eq!(zone("¿Qué hora es en Tokio?", now), tokio);
        assert_eq!(zone("time in tokyo", now), tokio);
        assert_eq!(
            zone("hora en santiago", now),
            Some(("12:00".into(), "sábado (Chile, misma hora)".into()))
        );
    }

    #[test]
    fn el_horario_de_verano_cuenta() {
        // Julio: Chile en invierno (-4) y Madrid en verano (+2) → +6 h.
        assert_eq!(
            zone("15:00 chile en madrid", "2026-07-15T12:00:00Z"),
            Some(("21:00".into(), "miércoles (Madrid, +6 h)".into()))
        );
        // Noviembre: Chile en verano (-3) y Madrid en invierno (+1) → +4 h.
        assert_eq!(
            zone("15:00 chile en madrid", "2026-11-10T12:00:00Z"),
            Some(("19:00".into(), "martes (Madrid, +4 h)".into()))
        );
    }

    #[test]
    fn las_zonas_no_se_inventan() {
        let now = "2026-10-17T15:00:00Z";
        assert_eq!(zone("tokio", now), None);
        assert_eq!(zone("15 en tokio", now), None);
        assert_eq!(zone("15:00 narnia en tokio", now), None);
        assert_eq!(zone("25:00 chile en tokio", now), None);
        assert_eq!(zone("13pm chile en tokio", now), None);
        // Las unidades de tiempo siguen siendo unidades.
        assert_eq!(value("1 hora en minutos").as_deref(), Some("60"));
    }
}
