//! Esquemas de herramientas que entiende cualquier proveedor.
//!
//! schemars escribe JSON Schema 2020-12 completo: `$ref` a `$defs`, `anyOf` con
//! `null` para los `Option`, `type: ["string", "null"]`, formatos como `uint64`.
//! Claude y OpenAI lo aceptan, pero Gemini (Antigravity) convierte el esquema a su
//! subconjunto de OpenAPI y rechaza la petición entera si algo no calza. Acá se deja
//! en la forma más simple que sirve para todos: sin referencias, sin uniones con
//! `null` y solo con formatos conocidos. Un campo opcional se nota porque no va en
//! `required`.

use rmcp::model::{JsonObject, Tool};
use serde_json::Value;
use std::sync::Arc;

/// Formatos que Gemini acepta; el resto (`uint32`, `uint64`…) se quita.
const FORMATOS: [&str; 5] = ["date-time", "int32", "int64", "float", "double"];

/// La herramienta con su esquema de entrada (y de salida, si tiene) simplificado.
pub fn portatil(mut herramienta: Tool) -> Tool {
    herramienta.input_schema = Arc::new(simplificar(&herramienta.input_schema));
    if let Some(salida) = herramienta.output_schema.take() {
        herramienta.output_schema = Some(Arc::new(simplificar(&salida)));
    }
    herramienta
}

pub fn simplificar(raiz: &JsonObject) -> JsonObject {
    let defs = raiz.get("$defs").or_else(|| raiz.get("definitions")).and_then(Value::as_object).cloned().unwrap_or_default();
    let mut limpio = raiz.clone();
    limpio.remove("$schema");
    limpio.remove("$defs");
    limpio.remove("definitions");
    match limpiar(Value::Object(limpio), &defs, 0) {
        Value::Object(mapa) => mapa,
        _ => JsonObject::new(),
    }
}

fn limpiar(valor: Value, defs: &JsonObject, hondura: usize) -> Value {
    // Un `$ref` que se apunta a sí mismo no puede quedar inline: se corta acá.
    if hondura > 32 {
        return serde_json::json!({});
    }
    let mut mapa = match valor {
        Value::Object(mapa) => mapa,
        // `true` (cualquier cosa) se vuelve `{}`; Gemini exige un objeto.
        Value::Bool(_) => return Value::Object(JsonObject::new()),
        otro => return otro,
    };

    if let Some(destino) = mapa.remove("$ref") {
        let nombre = destino.as_str().and_then(|r| r.rsplit('/').next()).unwrap_or_default();
        if let Some(Value::Object(definicion)) = defs.get(nombre) {
            // Lo que acompaña al `$ref` (la descripción, el `default`) gana.
            let mut junto = definicion.clone();
            junto.extend(mapa);
            mapa = junto;
        }
        return limpiar(Value::Object(mapa), defs, hondura + 1);
    }

    for union in ["anyOf", "oneOf"] {
        let Some(Value::Array(opciones)) = mapa.get(union) else { continue };
        let sin_null: Vec<Value> = opciones.iter().filter(|o| o.get("type") != Some(&Value::from("null"))).cloned().collect();
        if sin_null.len() == 1 && sin_null.len() < opciones.len() {
            mapa.remove(union);
            let Value::Object(opcion) = limpiar(sin_null[0].clone(), defs, hondura + 1) else { continue };
            for (clave, valor) in opcion {
                mapa.entry(clave).or_insert(valor);
            }
        }
    }

    if let Some(Value::Array(tipos)) = mapa.get("type") {
        let sin_null: Vec<Value> = tipos.iter().filter(|t| t.as_str() != Some("null")).cloned().collect();
        if let [solo] = sin_null.as_slice() {
            mapa.insert("type".into(), solo.clone());
        }
    }
    if mapa.get("default") == Some(&Value::Null) {
        mapa.remove("default");
    }
    if mapa.get("format").and_then(Value::as_str).is_some_and(|f| !FORMATOS.contains(&f)) {
        mapa.remove("format");
    }
    if let Some(Value::Array(valores)) = mapa.get_mut("enum") {
        valores.retain(|v| !v.is_null());
    }

    for clave in ["properties", "patternProperties"] {
        if let Some(Value::Object(hijos)) = mapa.remove(clave) {
            let hijos = hijos.into_iter().map(|(n, v)| (n, limpiar(v, defs, hondura + 1))).collect();
            mapa.insert(clave.into(), Value::Object(hijos));
        }
    }
    for clave in ["items", "additionalProperties"] {
        match mapa.remove(clave) {
            // `additionalProperties: false` es válido y se respeta; `items: true` no.
            Some(Value::Bool(b)) if clave == "additionalProperties" => {
                mapa.insert(clave.into(), Value::Bool(b));
            }
            Some(valor) => {
                mapa.insert(clave.into(), limpiar(valor, defs, hondura + 1));
            }
            None => {}
        }
    }
    for clave in ["anyOf", "oneOf", "allOf"] {
        if let Some(Value::Array(opciones)) = mapa.remove(clave) {
            let opciones = opciones.into_iter().map(|v| limpiar(v, defs, hondura + 1)).collect();
            mapa.insert(clave.into(), Value::Array(opciones));
        }
    }
    if mapa.get("type").and_then(Value::as_str) == Some("array") && !mapa.contains_key("items") {
        mapa.insert("items".into(), serde_json::json!({}));
    }
    Value::Object(mapa)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn simple(valor: Value) -> Value {
        Value::Object(simplificar(valor.as_object().unwrap()))
    }

    #[test]
    fn deja_inline_las_referencias_y_quita_el_null() {
        let esquema = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "properties": {
                "modo": { "anyOf": [{ "$ref": "#/$defs/Modo" }, { "type": "null" }], "description": "El modo" },
                "fijo": { "$ref": "#/$defs/Modo", "default": "a" },
                "nombre": { "type": ["string", "null"], "default": null },
                "espera": { "type": ["integer", "null"], "format": "uint64", "minimum": 0 },
                "ops": { "type": "array", "items": true }
            },
            "additionalProperties": false,
            "$defs": { "Modo": { "type": "string", "enum": ["a", "b"] } }
        });
        assert_eq!(
            simple(esquema),
            json!({
                "type": "object",
                "properties": {
                    "modo": { "type": "string", "enum": ["a", "b"], "description": "El modo" },
                    "fijo": { "type": "string", "enum": ["a", "b"], "default": "a" },
                    "nombre": { "type": "string" },
                    "espera": { "type": "integer", "minimum": 0 },
                    "ops": { "type": "array", "items": {} }
                },
                "additionalProperties": false
            })
        );
    }

    #[test]
    fn una_referencia_circular_no_se_cuelga() {
        let esquema = json!({
            "type": "object",
            "properties": { "nodo": { "$ref": "#/$defs/Nodo" } },
            "$defs": { "Nodo": { "type": "object", "properties": { "hijo": { "$ref": "#/$defs/Nodo" } } } }
        });
        assert!(simple(esquema).to_string().len() < 20_000);
    }
}
