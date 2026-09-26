//! Calculadora inline del launcher: aritmética, unidades y divisas.
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

#[derive(Clone, Copy, PartialEq, Eq)]
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
    // Datos (base: byte, decimal)
    Kb,
    Mb,
    Gb,
    Tb,
    // Tiempo (base: segundo)
    S,
    Min,
    H,
    D,
    // Temperatura (no lineal: fórmulas propias)
    Celsius,
    Fahrenheit,
    Kelvin,
}

/// Familia de la unidad: convertir entre familias distintas no tiene sentido.
fn family(unit: Unit) -> u8 {
    use Unit::*;
    match unit {
        Mm | Cm | M | Km | In | Ft | Yd | Mi => 0,
        Mg | G | Kg | Oz | Lb => 1,
        Kb | Mb | Gb | Tb => 2,
        S | Min | H | D => 3,
        Celsius | Fahrenheit | Kelvin => 4,
    }
}

fn parse_unit(raw: &str) -> Option<Unit> {
    use Unit::*;
    // Plural tolerante y grado opcional: «kms», «°C», «mins».
    let text = raw.trim().trim_end_matches('s').trim();
    Some(match text {
        "mm" | "milimetro" | "milimetros" => Mm,
        "cm" | "centimetro" | "centimetros" => Cm,
        "m" | "metro" | "metros" => M,
        "km" | "kilometro" | "kilometros" => Km,
        "in" | "pulgada" | "pulgadas" => In,
        "ft" | "pie" | "pies" => Ft,
        "yd" | "yarda" | "yardas" => Yd,
        "mi" | "milla" | "millas" => Mi,
        "mg" => Mg,
        "g" | "gramo" | "gramos" => G,
        "kg" | "kilo" | "kilos" | "kilogramo" | "kilogramos" => Kg,
        "oz" | "onza" | "onzas" => Oz,
        "lb" | "lbs" | "libra" | "libras" => Lb,
        "kb" | "kilobyte" | "kilobytes" => Kb,
        "mb" | "megabyte" | "megabytes" => Mb,
        "gb" | "gigabyte" | "gigabytes" => Gb,
        "tb" | "terabyte" | "terabytes" => Tb,
        "s" | "seg" | "segundo" | "segundos" => S,
        "min" | "minuto" | "minutos" => Min,
        "h" | "hs" | "hora" | "horas" => H,
        "d" | "dia" | "dias" => D,
        "c" | "°c" | "celsius" | "centigrado" | "centigrados" => Celsius,
        "f" | "°f" | "fahrenheit" => Fahrenheit,
        "k" | "°k" | "kelvin" => Kelvin,
        _ => return None,
    })
}

/// Factor hacia la unidad base de la familia (temperatura se resuelve aparte).
fn to_base(unit: Unit, value: f64) -> f64 {
    use Unit::*;
    match unit {
        Mm => value / 1000.0,
        Cm => value / 100.0,
        M => value,
        Km => value * 1000.0,
        In => value * 0.0254,
        Ft => value * 0.3048,
        Yd => value * 0.9144,
        Mi => value * 1609.344,
        Mg => value / 1000.0,
        G => value,
        Kg => value * 1000.0,
        Oz => value * 28.349_523_125,
        Lb => value * 453.592_37,
        Kb => value * 1000.0,
        Mb => value * 1_000_000.0,
        Gb => value * 1_000_000_000.0,
        Tb => value * 1_000_000_000_000.0,
        S => value,
        Min => value * 60.0,
        H => value * 3600.0,
        D => value * 86_400.0,
        Celsius | Fahrenheit | Kelvin => value,
    }
}

fn from_base(unit: Unit, value: f64) -> f64 {
    use Unit::*;
    match unit {
        M => value,
        Mm => value * 1000.0,
        Cm => value * 100.0,
        Km => value / 1000.0,
        In => value / 0.0254,
        Ft => value / 0.3048,
        Yd => value / 0.9144,
        Mi => value / 1609.344,
        G => value,
        Mg => value * 1000.0,
        Kg => value / 1000.0,
        Oz => value / 28.349_523_125,
        Lb => value / 453.592_37,
        Kb => value / 1000.0,
        Mb => value / 1_000_000.0,
        Gb => value / 1_000_000_000.0,
        Tb => value / 1_000_000_000_000.0,
        S => value,
        Min => value / 60.0,
        H => value / 3600.0,
        D => value / 86_400.0,
        Celsius | Fahrenheit | Kelvin => value,
    }
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
        if i > 0 && (digits.len() - i) % 3 == 0 {
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
}
