//! Calculadora inline del launcher: aritmética y conversión de unidades.
//!
//! Sin dependencias y sin red: lo que se puede calcular local se calcula local.
//! Divisas y cripto quedan afuera a propósito (necesitan una API y el producto
//! es local-first: si algún día entran, es con opt-in explícito, como Groq).
//!
//! `evaluate` devuelve `None` cuando la query **no** es una cuenta: el launcher
//! usa eso para decidir si agrega el resultado como primer hit. Todo lo que no
//! se puede resolver con certeza se rechaza en vez de mostrar un número dudoso.

/// Tope de decimales del resultado. Más que esto es ruido de punto flotante.
const DECIMALS: usize = 6;

/// Evalúa una query como cuenta matemática o conversión de unidades.
///
/// Devuelve el valor ya formateado (lo que se muestra y lo que se copia).
pub fn evaluate(query: &str) -> Option<String> {
    let q = query.trim();
    if q.is_empty() {
        return None;
    }
    if let Some(value) = convert_units(q) {
        return format_number(value);
    }
    let mut parser = Parser::new(q);
    let value = parser.expr()?;
    parser.skip_ws();
    // Si sobra texto, la query era otra cosa («2 chrome» no es una cuenta).
    if !parser.eof() {
        return None;
    }
    format_number(value)
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
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Self {
        Self { src, i: 0 }
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
            if c.is_ascii_digit() || c == '.' || c == '_' {
                self.i += c.len_utf8();
            } else {
                break;
            }
        }
        if start == self.i {
            return None;
        }
        // `1_000` se acepta como separador visual.
        let raw = self.src[start..self.i].replace('_', "");
        let value: f64 = raw.parse().ok()?;
        Some(value)
    }
}

// ---------------------------------------------------------------------------
// Unidades: `<número> <unidad> (to | in) <unidad>`
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

fn convert(from: Unit, to: Unit, value: f64) -> Option<f64> {
    if family(from) != family(to) {
        return None;
    }
    if family(from) == 4 {
        return Some(from_celsius(to, to_celsius(from, value)));
    }
    Some(from_base(to, to_base(from, value)))
}

/// `<número> <unidad> (to | in) <unidad>`, en cualquier caja.
fn convert_units(query: &str) -> Option<f64> {
    let lower = query.to_lowercase().replace('°', "");
    // Se prueban los separadores en orden: «1 in to cm» también contiene « in ».
    for sep in [" to ", "->", " in "] {
        let Some((left, right)) = lower.split_once(sep) else {
            continue;
        };
        let Some((value, from)) = number_and_unit(left) else {
            continue;
        };
        let to = parse_unit(right.trim())?;
        return convert(from, to, value);
    }
    None
}

/// «10 km» → (10.0, Km). El número tiene que ser literal (no una expresión).
fn number_and_unit(text: &str) -> Option<(f64, Unit)> {
    let mut parts = text.split_whitespace();
    let number: f64 = parts.next()?.replace('_', "").parse().ok()?;
    let unit = parse_unit(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some((number, unit))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(query: &str) -> Option<String> {
        evaluate(query)
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
    }

    #[test]
    fn conversion_de_unidades() {
        assert_eq!(value("10 km to mi").as_deref(), Some("6.213712"));
        assert_eq!(value("30 c to f").as_deref(), Some("86"));
        assert_eq!(value("2 gb to mb").as_deref(), Some("2000"));
        assert_eq!(value("1 h to min").as_deref(), Some("60"));
        assert_eq!(value("1 in to cm").as_deref(), Some("2.54"));
        assert_eq!(value("100 f to c").as_deref(), Some("37.777778"));
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
}
