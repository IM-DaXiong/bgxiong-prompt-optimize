//! Generate closed serde records and TypeScript types from one JSON Schema graph.
//! This generator defines representation; callers still validate schema constraints
//! (patterns, numeric ranges, references and allOf) at the domain boundary.
//! It never fetches a schema or invents a default for a required nullable field.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

pub struct GeneratedTypes {
    pub rust: String,
    pub typescript: String,
}

/// Explicit Rust representation choices owned by one generation entry point.
/// Keys are exact generated type names and JSON property names, never wire-value aliases.
/// Serde names, nullability, schema documents and TypeScript remain schema-owned.
#[derive(Clone, Default)]
pub struct RustRepresentationOptions {
    pub enum_variant_prefixes: BTreeMap<String, String>,
    pub boxed_fields: BTreeSet<(String, String)>,
}

struct Generator<'a> {
    documents: &'a BTreeMap<String, Value>,
    prefix: &'a str,
    emitted: BTreeSet<String>,
    names: BTreeMap<String, String>,
    rust: String,
    ts: String,
    rust_options: RustRepresentationOptions,
    used_enum_options: BTreeSet<String>,
    used_box_options: BTreeSet<(String, String)>,
}

fn pascal(value: &str) -> String {
    // SCREAMING_SNAKE values (error codes) become PascalCase words, not one all-caps run.
    let screaming = !value.chars().any(|c| c.is_ascii_lowercase());
    let mut out = String::new();
    let mut upper = true;
    for c in value.chars() {
        if !c.is_ascii_alphanumeric() {
            upper = true;
        } else if upper {
            out.push(c.to_ascii_uppercase());
            upper = false;
        } else if screaming {
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn snake(value: &str) -> String {
    let mut out = String::new();
    for (index, c) in value.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if index > 0 && !out.ends_with('_') {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    match out.as_str() {
        "type" | "ref" | "mod" | "match" | "self" | "move" | "use" | "enum" | "struct"
        | "const" | "fn" | "pub" | "where" | "loop" | "in" | "as" => format!("r#{out}"),
        _ => out,
    }
}

fn quoted(value: &str) -> String {
    serde_json::to_string(value).expect("a string is serializable")
}

impl Generator<'_> {
    fn definition(&mut self, document: &str, key: &str) -> Result<String, String> {
        let identity = format!("{document}#{key}");
        // One schema identity keeps one name, including identities owned by a shared document
        // generated elsewhere (see `generate_with_shared`).
        if let Some((known, _)) = self.names.iter().find(|(_, id)| **id == identity) {
            return Ok(known.clone());
        }
        let mut name = format!("{}{}", self.prefix, pascal(key));
        if self
            .names
            .get(&name)
            .is_some_and(|previous| previous != &identity)
        {
            // Cross-document schemas commonly reuse utility names such as `id` and `digest`.
            // Keep the root document's stable short name and namespace only the colliding
            // referenced definition. This is deterministic and preserves one generated type per
            // schema identity instead of requiring callers to copy external definitions locally.
            name = format!("{}{}{}", self.prefix, pascal(document), pascal(key));
            if self
                .names
                .get(&name)
                .is_some_and(|previous| previous != &identity)
            {
                return Err(format!("generated type collision: {name}"));
            }
        }
        self.names.insert(name.clone(), identity.clone());
        if !self.emitted.insert(identity) {
            return Ok(name);
        }
        let doc = self
            .documents
            .get(document)
            .ok_or_else(|| format!("missing schema {document}"))?;
        let schema = if key == "Index" {
            doc.clone()
        } else {
            doc.get("definitions")
                .and_then(|defs| defs.get(key))
                .ok_or_else(|| format!("missing definition {document}#{key}"))?
                .clone()
        };
        self.emit_named(document, &name, &schema)?;
        Ok(name)
    }

    fn emit_named(&mut self, document: &str, name: &str, schema: &Value) -> Result<(), String> {
        if schema.get("oneOf").is_some() {
            return self.emit_union(document, name, schema);
        }
        if schema.get("type").and_then(Value::as_str) == Some("object")
            && schema.get("properties").is_some()
        {
            return self.emit_object(document, name, schema);
        }
        if let Some(values) = schema.get("enum").and_then(Value::as_array) {
            if values.is_empty() || values.iter().any(|v| !v.is_string()) {
                return Err(format!("{name}: only nonempty string enums are supported"));
            }
            let mut seen = BTreeSet::new();
            let mut variants = String::new();
            let mut ts = Vec::new();
            let strip_prefix = self.rust_options.enum_variant_prefixes.get(name).cloned();
            if strip_prefix
                .as_ref()
                .is_some_and(|prefix| prefix.is_empty())
            {
                return Err(format!("{name}: Rust variant prefix must not be empty"));
            }
            for value in values {
                let value = value.as_str().expect("checked string enum");
                let rust_value = match strip_prefix.as_deref() {
                    Some(prefix) => value.strip_prefix(prefix).ok_or_else(|| {
                        format!("{name}: wire value {value:?} does not carry Rust namespace prefix {prefix:?}")
                    })?,
                    None => value,
                };
                let variant = pascal(rust_value);
                if variant.is_empty() || !seen.insert(variant.clone()) {
                    return Err(format!("{name}: colliding/empty enum variant"));
                }
                variants.push_str(&format!(
                    "    #[serde(rename = {})]\n    {variant},\n",
                    quoted(value)
                ));
                ts.push(quoted(value));
            }
            if strip_prefix.is_some() {
                self.used_enum_options.insert(name.to_owned());
            }
            self.rust.push_str(&format!("#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]\npub enum {name} {{\n{variants}}}\n\n"));
            self.ts
                .push_str(&format!("export type {name} = {};\n\n", ts.join(" | ")));
            return Ok(());
        }
        let (rust, ts) = self.representation(document, name, schema)?;
        self.rust
            .push_str(&format!("pub type {name} = {rust};\n\n"));
        self.ts.push_str(&format!("export type {name} = {ts};\n\n"));
        Ok(())
    }

    fn representation(
        &mut self,
        document: &str,
        hint: &str,
        schema: &Value,
    ) -> Result<(String, String), String> {
        if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
            let (file, fragment) = reference
                .split_once('#')
                .ok_or("schema ref must have a fragment")?;
            let file = if file.is_empty() { document } else { file };
            let key = fragment
                .strip_prefix("/definitions/")
                .ok_or("only definition refs are supported")?;
            let name = self.definition(file, key)?;
            return Ok((name.clone(), name));
        }
        if schema.get("oneOf").is_some() {
            self.emit_union(document, hint, schema)?;
            return Ok((hint.to_owned(), hint.to_owned()));
        }
        if let Some(alternatives) = schema.get("anyOf").and_then(Value::as_array) {
            if alternatives.len() != 2 {
                return Err(format!("{hint}: expected nullable anyOf"));
            }
            let nonnull: Vec<&Value> = alternatives
                .iter()
                .filter(|v| v.get("type").and_then(Value::as_str) != Some("null"))
                .collect();
            if nonnull.len() != 1 {
                return Err(format!("{hint}: unsupported anyOf"));
            }
            let (rust, ts) = self.representation(document, hint, nonnull[0])?;
            return Ok((format!("Option<{rust}>"), format!("{ts} | null")));
        }
        if schema.get("enum").is_some() {
            self.emit_named(document, hint, schema)?;
            return Ok((hint.to_string(), hint.to_string()));
        }
        if let Some(value) = schema.get("const") {
            return match value {
                Value::Number(_) if value.as_u64().is_some() => {
                    Ok(("u64".into(), value.to_string()))
                }
                Value::String(_) => Ok(("String".into(), value.to_string())),
                Value::Bool(_) => Ok(("bool".into(), value.to_string())),
                _ => Err(format!("{hint}: unsupported const")),
            };
        }
        match schema.get("type").and_then(Value::as_str) {
            Some("string") => Ok(("String".into(), "string".into())),
            Some("integer") => {
                let nonnegative = schema
                    .get("minimum")
                    .and_then(Value::as_i64)
                    .is_some_and(|v| v >= 0);
                Ok((
                    if nonnegative { "u64" } else { "i64" }.into(),
                    "number".into(),
                ))
            }
            Some("number") => Ok(("f64".into(), "number".into())),
            Some("boolean") => Ok(("bool".into(), "boolean".into())),
            Some("array") => {
                let item = schema
                    .get("items")
                    .ok_or_else(|| format!("{hint}: missing items"))?;
                let (rust, ts) = self.representation(document, &format!("{hint}Item"), item)?;
                Ok((format!("Vec<{rust}>"), format!("Array<{ts}>")))
            }
            Some("object") if schema.get("properties").is_some() => {
                self.emit_object(document, hint, schema)?;
                Ok((hint.to_string(), hint.to_string()))
            }
            Some("object") => {
                let additional = schema
                    .get("additionalProperties")
                    .ok_or_else(|| format!("{hint}: map must declare values"))?;
                let (rust, ts) =
                    self.representation(document, &format!("{hint}Value"), additional)?;
                Ok((
                    format!("std::collections::BTreeMap<String, {rust}>"),
                    format!("Record<string, {ts}>"),
                ))
            }
            None if schema.as_object().is_some_and(|v| v.is_empty()) => {
                Ok(("serde_json::Value".into(), "unknown".into()))
            }
            _ => Err(format!("{hint}: unsupported schema representation")),
        }
    }

    fn emit_union(&mut self, document: &str, name: &str, schema: &Value) -> Result<(), String> {
        let cases = schema
            .get("oneOf")
            .and_then(Value::as_array)
            .ok_or("missing oneOf")?;
        if cases.is_empty() {
            return Err(format!("{name}: empty union"));
        }
        let mut variants = String::new();
        let mut types = Vec::new();
        let mut kinds = BTreeSet::new();
        // The first candidate present as a constant on every case. A case set that mixes
        // discriminators (or has none in common) is rejected, never guessed.
        const DISCRIMINATORS: [&str; 7] =
            ["kind", "type", "action", "op", "mode", "operation", "stage"];
        let discriminator = DISCRIMINATORS
            .iter()
            .copied()
            .find(|name| {
                cases
                    .iter()
                    .all(|case| case.pointer(&format!("/properties/{name}/const")).is_some())
            })
            .ok_or_else(|| {
                format!(
                    "{name}: union requires a common constant kind, type, action, op, mode, operation or stage discriminator"
                )
            })?;
        for case in cases {
            let kind = case
                .pointer(&format!("/properties/{discriminator}/const"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    format!("{name}: union requires a constant {discriminator} discriminator")
                })?;
            let variant = pascal(kind);
            if variant.is_empty() || !kinds.insert(variant.clone()) {
                return Err(format!("{name}: duplicate union kind"));
            }
            if !case
                .get("required")
                .and_then(Value::as_array)
                .is_some_and(|required| required.iter().any(|field| field == discriminator))
            {
                return Err(format!("{name}: {discriminator} must be required"));
            }
            let mut fields = case.clone();
            fields["properties"]
                .as_object_mut()
                .ok_or("union properties")?
                .remove(discriminator);
            fields["required"]
                .as_array_mut()
                .ok_or("union required")?
                .retain(|field| field != discriminator);
            let case_name = format!("{name}{variant}");
            let (rust_fields, ts_fields) =
                self.emit_fields(document, &case_name, &fields, false)?;
            variants.push_str(&format!(
                "    #[serde(rename = {})]\n    {variant} {{\n{rust_fields}    }},\n",
                quoted(kind)
            ));
            self.ts.push_str(&format!(
                "export interface {case_name} {{\n  \"{discriminator}\": {};\n{ts_fields}}}\n\n",
                quoted(kind)
            ));
            types.push(case_name);
        }
        self.rust.push_str(&format!("#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]\n#[serde(tag = \"{discriminator}\", rename_all_fields = \"camelCase\", deny_unknown_fields)]\npub enum {name} {{\n{variants}}}\n\n"));
        self.ts
            .push_str(&format!("export type {name} = {};\n\n", types.join(" | ")));
        Ok(())
    }

    fn emit_fields(
        &mut self,
        document: &str,
        name: &str,
        schema: &Value,
        public: bool,
    ) -> Result<(String, String), String> {
        if !matches!(schema.get("additionalProperties"), Some(Value::Bool(_))) {
            return Err(format!("{name}: records must declare additionalProperties"));
        }
        let properties = schema
            .get("properties")
            .and_then(Value::as_object)
            .ok_or("missing properties")?;
        let required: BTreeSet<&str> = schema
            .get("required")
            .and_then(Value::as_array)
            .ok_or("record must declare required fields")?
            .iter()
            .map(|v| v.as_str().ok_or("required must contain strings"))
            .collect::<Result<_, _>>()?;
        if required.iter().any(|key| !properties.contains_key(*key)) {
            return Err(format!("{name}: required field absent from properties"));
        }
        let mut names = BTreeSet::new();
        let mut rust_fields = String::new();
        let mut ts_fields = String::new();
        // Build dependencies can enable serde_json/preserve_order through feature
        // unification. Contract output must not depend on that build graph.
        let ordered_properties: BTreeMap<_, _> = properties.iter().collect();
        for (key, property) in ordered_properties {
            let field = snake(key);
            if !names.insert(field.clone()) {
                return Err(format!("{name}: field name collision"));
            }
            let (mut rust, ts) =
                self.representation(document, &format!("{name}{}", pascal(key)), property)?;
            let required = required.contains(key.as_str());
            let nullable = rust.starts_with("Option<");
            let empty_collection = !required
                && property.get("type").and_then(Value::as_str) == Some("array")
                && property
                    .get("default")
                    .and_then(Value::as_array)
                    .is_some_and(Vec::is_empty);
            rust_fields.push_str(&format!("    #[serde(rename = {})]\n", quoted(key)));
            if empty_collection {
                // An explicitly schema-owned empty optional collection has no bindings when absent.
                // Vec's normal deserializer still refuses null and malformed supplied elements.
                rust_fields.push_str("    #[serde(default)]\n");
            } else if !required {
                if !nullable {
                    rust = format!("Option<{rust}>");
                    // Unreleased protocol: a missing key is unbound. Explicit null is not a second spelling.
                    rust_fields.push_str(
                        "    #[serde(default, skip_serializing_if = \"Option::is_none\", deserialize_with = \"deserialize_optional_deny_null\")]\n",
                    );
                } else {
                    rust_fields.push_str(
                        "    #[serde(default, skip_serializing_if = \"Option::is_none\")]\n",
                    );
                }
            } else if nullable {
                rust_fields.push_str(
                    "    #[serde(deserialize_with = \"deserialize_required_nullable\")]\n",
                );
            }
            let box_key = (name.to_owned(), key.clone());
            if self.rust_options.boxed_fields.contains(&box_key) {
                if property.get("const").is_some() {
                    return Err(format!(
                        "{name}.{key}: Rust boxing of a const field is unsupported"
                    ));
                }
                // Keep Option outermost: omission/null checks and skip_serializing_if keep
                // precisely their existing schema meaning; None never allocates a box.
                rust = match rust
                    .strip_prefix("Option<")
                    .and_then(|r| r.strip_suffix('>'))
                {
                    Some(inner) => format!("Option<Box<{inner}>>"),
                    None => format!("Box<{rust}>"),
                };
                self.used_box_options.insert(box_key);
            }
            if let Some(value) = property.get("const") {
                let function = format!("deserialize_{}_{}", snake(name), field);
                let mismatch = match value.as_bool() {
                    Some(true) => "!value".to_owned(),
                    Some(false) => "value".to_owned(),
                    None => format!("value != {value}"),
                };
                self.rust.push_str(&format!("fn {function}<'de, D: serde::Deserializer<'de>>(d: D) -> Result<{rust}, D::Error> {{\n    let value = <{rust} as serde::Deserialize>::deserialize(d)?;\n    if {mismatch} {{ return Err(serde::de::Error::custom(\"schema const mismatch\")); }}\n    Ok(value)\n}}\n\n"));
                rust_fields.push_str(&format!(
                    "    #[serde(deserialize_with = {})]\n",
                    quoted(&function)
                ));
            }
            rust_fields.push_str(&format!(
                "    {}{field}: {rust},\n",
                if public { "pub " } else { "" }
            ));
            ts_fields.push_str(&format!(
                "  {}{}: {ts};\n",
                quoted(key),
                if required { "" } else { "?" }
            ));
        }
        if schema.get("additionalProperties") == Some(&Value::Bool(true)) {
            if names.contains("additional_fields") {
                return Err(format!("{name}: extension field name collision"));
            }
            rust_fields.push_str(&format!("    #[serde(flatten)]\n    {}additional_fields: std::collections::BTreeMap<String, serde_json::Value>,\n",if public {"pub "} else {""}));
            ts_fields.push_str("  [key: string]: unknown;\n");
        }
        Ok((rust_fields, ts_fields))
    }

    fn emit_object(&mut self, document: &str, name: &str, schema: &Value) -> Result<(), String> {
        let (rust_fields, ts_fields) = self.emit_fields(document, name, schema, true)?;
        let closed = if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
            "#[serde(deny_unknown_fields)]\n"
        } else {
            ""
        };
        self.rust.push_str(&format!("#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]\n{closed}pub struct {name} {{\n{rust_fields}}}\n\n"));
        if ts_fields.is_empty() && !closed.is_empty() {
            // A closed object with no properties: `{}` in TS would accept any non-nullish value.
            self.ts
                .push_str(&format!("export type {name} = Record<string, never>;\n\n"));
        } else {
            self.ts
                .push_str(&format!("export interface {name} {{\n{ts_fields}}}\n\n"));
        }
        Ok(())
    }
}

pub fn generate(
    documents: &BTreeMap<String, Value>,
    document: &str,
    definitions: &[&str],
    prefix: &str,
) -> Result<GeneratedTypes, String> {
    generate_documents(documents, &[(document, definitions)], prefix)
}

pub fn generate_with_options(
    documents: &BTreeMap<String, Value>,
    document: &str,
    definitions: &[&str],
    prefix: &str,
    options: &RustRepresentationOptions,
) -> Result<GeneratedTypes, String> {
    generate_documents_with_options(documents, &[(document, definitions)], prefix, options)
}

/// Types for `document` whose references into `shared` documents resolve to the types those
/// documents' own generator calls emit (same names, never emitted again). The caller makes the
/// shared module's names visible to the Rust output (e.g. a `use` of that module).
pub fn generate_with_shared(
    documents: &BTreeMap<String, Value>,
    document: &str,
    definitions: &[&str],
    prefix: &str,
    shared: &[(&str, &[&str], &str)],
) -> Result<GeneratedTypes, String> {
    for p in std::iter::once(prefix).chain(shared.iter().map(|(_, _, p)| *p)) {
        if p.is_empty() || !p.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err("type prefix must be a nonempty identifier".into());
        }
    }
    let mut names = BTreeMap::new();
    let mut emitted = BTreeSet::new();
    for (shared_document, shared_definitions, shared_prefix) in shared {
        let mut owner = Generator {
            documents,
            prefix: shared_prefix,
            emitted: BTreeSet::new(),
            names: BTreeMap::new(),
            rust: String::new(),
            ts: String::new(),
            rust_options: RustRepresentationOptions::default(),
            used_enum_options: BTreeSet::new(),
            used_box_options: BTreeSet::new(),
        };
        for definition in shared_definitions.iter() {
            owner.definition(shared_document, definition)?;
        }
        // Only the shared document's own definitions are shared; what it references in other
        // documents keeps the caller's names.
        let own = format!("{shared_document}#");
        names.extend(
            owner
                .names
                .into_iter()
                .filter(|(_, id)| id.starts_with(&own)),
        );
        emitted.extend(owner.emitted.into_iter().filter(|id| id.starts_with(&own)));
    }
    let mut generator = Generator {
        documents,
        prefix,
        emitted,
        names,
        rust: String::new(),
        ts: String::new(),
        rust_options: RustRepresentationOptions::default(),
        used_enum_options: BTreeSet::new(),
        used_box_options: BTreeSet::new(),
    };
    run_roots(generator_header(&mut generator), &[(document, definitions)])
}

/// One type graph rooted at several documents: every schema identity is emitted once, so
/// definitions shared across the roots (via cross-document refs) never become two Rust types.
pub fn generate_documents(
    documents: &BTreeMap<String, Value>,
    roots: &[(&str, &[&str])],
    prefix: &str,
) -> Result<GeneratedTypes, String> {
    generate_documents_with_options(
        documents,
        roots,
        prefix,
        &RustRepresentationOptions::default(),
    )
}

pub fn generate_documents_with_options(
    documents: &BTreeMap<String, Value>,
    roots: &[(&str, &[&str])],
    prefix: &str,
    options: &RustRepresentationOptions,
) -> Result<GeneratedTypes, String> {
    if prefix.is_empty() || !prefix.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("type prefix must be a nonempty identifier".into());
    }
    let mut generator = Generator {
        documents,
        prefix,
        emitted: BTreeSet::new(),
        names: BTreeMap::new(),
        rust: String::new(),
        ts: String::new(),
        rust_options: options.clone(),
        used_enum_options: BTreeSet::new(),
        used_box_options: BTreeSet::new(),
    };
    run_roots(generator_header(&mut generator), roots)
}

fn generator_header<'g, 'a>(generator: &'g mut Generator<'a>) -> &'g mut Generator<'a> {
    generator.rust = "// @generated from JSON Schema. Do not edit.\n\nfn deserialize_required_nullable<'de, D, T>(d: D) -> Result<Option<T>, D::Error>\nwhere D: serde::Deserializer<'de>, T: serde::Deserialize<'de> {\n    <Option<T> as serde::Deserialize>::deserialize(d)\n}\n\nfn deserialize_optional_deny_null<'de, D, T>(d: D) -> Result<Option<T>, D::Error>\nwhere D: serde::Deserializer<'de>, T: serde::Deserialize<'de> {\n    match <Option<T> as serde::Deserialize>::deserialize(d)? {\n        Some(value) => Ok(Some(value)),\n        None => Err(serde::de::Error::custom(\"null is refused; omit the key\")),\n    }\n}\n\n".into();
    generator.ts = "// @generated from JSON Schema. Do not edit.\n\n".into();
    generator
}

fn run_roots(
    generator: &mut Generator<'_>,
    roots: &[(&str, &[&str])],
) -> Result<GeneratedTypes, String> {
    for (document, definitions) in roots {
        for definition in definitions.iter() {
            generator.definition(document, definition)?;
        }
    }
    if generator
        .rust_options
        .enum_variant_prefixes
        .keys()
        .any(|name| !generator.used_enum_options.contains(name))
        || generator
            .rust_options
            .boxed_fields
            .iter()
            .any(|field| !generator.used_box_options.contains(field))
    {
        return Err("Rust representation option did not match an emitted enum or field".into());
    }
    let mut rust = std::mem::take(&mut generator.rust);
    // Emit each serde helper only when a generated field uses it (no dead code in OUT_DIR).
    for helper in [
        "deserialize_required_nullable",
        "deserialize_optional_deny_null",
    ] {
        let used = rust.contains(&format!("deserialize_with = \"{helper}\""));
        if !used {
            let start = rust
                .find(&format!("fn {helper}<"))
                .expect("helper is emitted");
            let end = start + rust[start..].find("\n}\n\n").expect("helper body ends") + 4;
            rust.replace_range(start..end, "");
        }
    }
    Ok(GeneratedTypes {
        rust,
        typescript: std::mem::take(&mut generator.ts),
    })
}

#[cfg(test)]
mod video_span_contract_tests {
    use super::*;

    #[test]
    fn explicit_rust_representation_preserves_wire_types_and_refuses_invalid_options() {
        let doc = serde_json::json!({"definitions": {
            "error": {"type":"string","enum":["SC_SOURCE","SC_TARGET"]},
            "continuation": {"type":"object","additionalProperties":false,"required":["error"],
                "properties":{"error":{"$ref":"#/definitions/error"}}},
            "request": {"oneOf":[{"type":"object","additionalProperties":false,"required":["kind"],
                "properties":{"kind":{"const":"scenes"},"next":{"anyOf":[{"$ref":"#/definitions/continuation"},{"type":"null"}]}}}]}
        }});
        let docs = BTreeMap::from([("d.json".to_owned(), doc)]);
        let plain = generate(&docs, "d.json", &["request"], "Fixture").unwrap();
        let options = RustRepresentationOptions {
            enum_variant_prefixes: BTreeMap::from([("FixtureError".into(), "SC_".into())]),
            boxed_fields: BTreeSet::from([("FixtureRequestScenes".into(), "next".into())]),
        };
        let configured =
            generate_with_options(&docs, "d.json", &["request"], "Fixture", &options).unwrap();
        assert_eq!(
            configured.typescript, plain.typescript,
            "physical Rust choices cannot change the wire twin"
        );
        assert!(configured
            .rust
            .contains("next: Option<Box<FixtureContinuation>>"));
        assert!(configured
            .rust
            .contains("#[serde(rename = \"SC_SOURCE\")]\n    Source,"));
        assert!(configured
            .rust
            .contains("#[serde(default, skip_serializing_if = \"Option::is_none\")]"));
        assert!(!configured.rust.contains("Box<Option<"));

        for prefix in ["", "OTHER_", "SC_SOURCE"] {
            let mut invalid = options.clone();
            invalid
                .enum_variant_prefixes
                .insert("FixtureError".into(), prefix.into());
            assert!(
                generate_with_options(&docs, "d.json", &["request"], "Fixture", &invalid).is_err()
            );
        }
        let mut collision = docs.clone();
        collision.get_mut("d.json").unwrap()["definitions"]["error"]["enum"] =
            serde_json::json!(["SC_SAME_NAME", "SC_SAME-NAME"]);
        assert!(
            generate_with_options(&collision, "d.json", &["request"], "Fixture", &options).is_err()
        );
        let mut unknown = options.clone();
        unknown
            .boxed_fields
            .insert(("FixtureRequestScenes".into(), "misspelled".into()));
        assert!(generate_with_options(&docs, "d.json", &["request"], "Fixture", &unknown).is_err());
        let mut unknown = options;
        unknown
            .enum_variant_prefixes
            .insert("MissingEnum".into(), "SC_".into());
        assert!(generate_with_options(&docs, "d.json", &["request"], "Fixture", &unknown).is_err());
    }

    #[test]
    fn action_tagged_union_generates_and_mixed_discriminator_is_rejected() {
        let valid = serde_json::json!({
            "definitions": {"decision": {"oneOf": [
                {"type":"object","required":["action"],"additionalProperties":false,
                 "properties":{"action":{"const":"keep"}}},
                {"type":"object","required":["action","note"],"additionalProperties":false,
                 "properties":{"action":{"const":"needs_attention"},"note":{"type":"string"}}}
            ]}}
        });
        let mut docs = BTreeMap::from([("d.schema.json".to_owned(), valid)]);
        let generated = generate(&docs, "d.schema.json", &["decision"], "Sf").unwrap();
        assert!(generated.rust.contains("#[serde(tag = \"action\""));
        assert!(generated.rust.contains("NeedsAttention {"));
        assert!(generated
            .typescript
            .contains("\"action\": \"needs_attention\";"));

        docs.get_mut("d.schema.json").unwrap()["definitions"]["decision"]["oneOf"][1]
            ["properties"] = serde_json::json!({"kind":{"const":"x"},"note":{"type":"string"}});
        assert!(generate(&docs, "d.schema.json", &["decision"], "Sf").is_err());
    }

    #[test]
    fn multi_root_graph_emits_shared_definition_once() {
        let shared = serde_json::json!({"definitions": {"severity": {"type":"string","enum":["blocking","minor"]}}});
        let a = serde_json::json!({"definitions": {"finding": {"type":"object","additionalProperties":false,
            "required":["severity"],"properties":{"severity":{"$ref":"s.json#/definitions/severity"}}}}});
        let b = serde_json::json!({"definitions": {"check": {"type":"object","additionalProperties":false,
            "required":["severity"],"properties":{"severity":{"anyOf":[{"$ref":"s.json#/definitions/severity"},{"type":"null"}]}}}}});
        let docs = BTreeMap::from([
            ("s.json".to_owned(), shared),
            ("a.json".to_owned(), a),
            ("b.json".to_owned(), b),
        ]);
        let generated = generate_documents(
            &docs,
            &[("a.json", &["finding"]), ("b.json", &["check"])],
            "Sf",
        )
        .unwrap();
        assert_eq!(generated.rust.matches("pub enum SfSeverity").count(), 1);
        assert_eq!(
            generated
                .typescript
                .matches("export type SfSeverity =")
                .count(),
            1
        );
        assert!(generated
            .typescript
            .contains("\"severity\": SfSeverity | null;"));
    }

    #[test]
    fn type_tagged_union_generates_and_invalid_contract_is_rejected() {
        let valid = serde_json::json!({
            "definitions": {"intent": {"oneOf": [
                {"type":"object","required":["type"],"additionalProperties":false,
                 "properties":{"type":{"const":"video.span.archive"}}}
            ]}}
        });
        let mut docs = BTreeMap::from([("span.schema.json".to_owned(), valid)]);
        let generated = generate(&docs, "span.schema.json", &["intent"], "VideoSpan").unwrap();
        assert!(generated.rust.contains("#[serde(tag = \"type\""));
        assert!(generated
            .typescript
            .contains("\"type\": \"video.span.archive\""));

        docs.get_mut("span.schema.json").unwrap()["definitions"]["intent"]["oneOf"][0]
            ["required"] = serde_json::json!([]);
        assert!(generate(&docs, "span.schema.json", &["intent"], "VideoSpan").is_err());
    }

    #[test]
    fn boolean_const_generates_closed_rust_and_typescript_fields() {
        let valid = serde_json::json!({
            "definitions": {"policy": {
                "type": "object",
                "additionalProperties": false,
                "required": ["enabled"],
                "properties": {"enabled": {"const": true}}
            }}
        });
        let docs = BTreeMap::from([("stage.schema.json".to_owned(), valid)]);
        let generated = generate(&docs, "stage.schema.json", &["policy"], "Stage").unwrap();
        assert!(generated.rust.contains("pub enabled: bool"));
        assert!(generated.rust.contains("if !value"));
        assert!(generated.typescript.contains("\"enabled\": true"));
    }

    #[test]
    fn optional_non_null_field_refuses_json_null() {
        let schema = serde_json::json!({
            "definitions": {"readArgs": {
                "type": "object",
                "required": ["schemaVersion"],
                "additionalProperties": false,
                "properties": {
                    "schemaVersion": {"const": 1},
                    "cursor": {"type": "string", "minLength": 1},
                    "bindings": {"type": "array", "default": [], "items": {"type": "string"}},
                    "taskRef": {"anyOf": [{"type": "string"}, {"type": "null"}]}
                }
            }}
        });
        let docs = BTreeMap::from([("span.schema.json".to_owned(), schema)]);
        let generated = generate(&docs, "span.schema.json", &["readArgs"], "VideoSpan").unwrap();
        assert!(generated.typescript.contains("\"cursor\"?: string;"));
        assert!(generated
            .typescript
            .contains("\"bindings\"?: Array<string>;"));
        let bindings_at = generated
            .rust
            .find("pub bindings: Vec<String>")
            .expect("explicit empty optional collection stays a Vec");
        let binding_attr =
            &generated.rust[generated.rust[..bindings_at].rfind("#[").unwrap()..bindings_at];
        assert!(binding_attr.contains("serde(default)"));
        assert!(!generated.typescript.contains("\"cursor\"?: string | null;"));
        let cursor_at = generated.rust.find("pub cursor:").expect("cursor field");
        assert!(generated.rust[..cursor_at].contains("deserialize_optional_deny_null"));
        let task_at = generated.rust.find("pub task_ref:").expect("taskRef field");
        let task_attr =
            &generated.rust[generated.rust[..task_at].rfind("#[").expect("task attr")..task_at];
        assert!(
            !task_attr.contains("deserialize_optional_deny_null"),
            "schema-nullable taskRef must keep accepting null: {task_attr}"
        );
    }
}
#[test]
fn shared_document_identity_reuses_owner_names_without_reemitting_types() {
    let docs = BTreeMap::from([
        (
            "shared.json".into(),
            serde_json::json!({"definitions":{"value":{"type":"object","additionalProperties":false,"required":["text"],"properties":{"text":{"type":"string"}}}}}),
        ),
        (
            "consumer.json".into(),
            serde_json::json!({"definitions":{"request":{"type":"object","additionalProperties":false,"required":["value"],"properties":{"value":{"$ref":"shared.json#/definitions/value"}}}}}),
        ),
    ]);
    let generated = generate_with_shared(
        &docs,
        "consumer.json",
        &["request"],
        "Consumer",
        &[("shared.json", &["value"], "Shared")],
    )
    .unwrap();
    assert!(generated.rust.contains("pub value: SharedValue"));
    assert!(!generated.rust.contains("pub struct SharedValue"));
    assert!(generated.typescript.contains("\"value\": SharedValue"));
}
