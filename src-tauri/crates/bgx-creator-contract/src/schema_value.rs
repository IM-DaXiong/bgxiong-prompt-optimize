//! 模块：Creator 工具与规则渐进发现；保持同一合同、执行内核与离线可验证边界。
//! CT04/C03: the one structural value checker (moved from `tool_schemas::check_value`).
//! Implemented keywords keep their previous semantics; the type check no longer lets an
//! unknown type name pass, and combinators report bounded per-branch reasons (D08), so a
//! `oneOf matched 0` names the field that was missing in every branch.
//!
//! Three explicit keyword classes (C03):
//! - [`STRUCTURAL_KEYWORDS`]: evaluated here.
//! - [`DELEGATED_KEYWORDS`]: declared, evaluated by the typed service validators
//!   (`StableId::parse`, `parse_resource_ref`, activation lookup ...). `pattern` uses look-around,
//!   so it is never handed to a regex engine that would silently accept it.
//! - [`ANNOTATION_KEYWORDS`]: no validation meaning.
//!
//! Any other keyword is rejected by [`verify_schema_keywords`]. No keyword or regex judges free
//! text produced by a model.

use crate::diagnostics::{ContractViolation, MAX_BRANCH_REASONS};
use serde_json::Value;

pub const STRUCTURAL_KEYWORDS: &[&str] = &[
    "type", "enum", "const", "minLength", "maxLength", "minimum", "maximum", "exclusiveMinimum",
    "exclusiveMaximum", "required", "properties", "additionalProperties", "items", "minItems",
    "maxItems", "uniqueItems", "allOf", "anyOf", "oneOf", "not", "if", "then", "else",
];
pub const DELEGATED_KEYWORDS: &[&str] = &[
    "pattern", "format", "propertyNames", "patternProperties", "minProperties",
    "maxProperties", "contains", "multipleOf",
];
pub const ANNOTATION_KEYWORDS: &[&str] = &[
    "title", "description", "$comment", "examples", "default", "deprecated", "readOnly",
    "writeOnly", "$id", "$schema",
];
pub const TYPE_NAMES: &[&str] = &["string", "integer", "number", "boolean", "object", "array", "null"];

/// Structural failure of one value (path + keyword + bounded branch reasons).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueError {
    pub instance_path: String,
    pub schema_path: String,
    pub branch_reasons: Vec<String>,
    pub message: String,
    from_discriminated_branch: bool,
}

impl ValueError {
    fn at(path: &str, keyword: &str, message: String) -> Self {
        Self {
            instance_path: path.to_string(),
            schema_path: keyword.to_string(),
            branch_reasons: Vec::new(),
            message,
            from_discriminated_branch: false,
        }
    }
}

impl std::fmt::Display for ValueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if self.from_discriminated_branch {
            write!(f, " [schemaPath={}]", self.schema_path)?;
        }
        if !self.branch_reasons.is_empty() {
            write!(f, " [branches: {}]", self.branch_reasons.join(" | "))?;
        }
        Ok(())
    }
}

fn type_matches(t: &str, v: &Value) -> Result<bool, String> {
    Ok(match t {
        "string" => v.is_string(),
        "integer" => v.is_i64() || v.is_u64(),
        "number" => v.is_number(),
        "boolean" => v.is_boolean(),
        "object" => v.is_object(),
        "array" => v.is_array(),
        "null" => v.is_null(),
        other => return Err(other.to_string()),
    })
}

#[derive(Clone, Copy)]
struct ValueDocument<'d, 'r, 'f> {
    document: &'d SchemaDocument<'r>,
    file: &'f str,
    root: &'r Value,
}

fn branch_reasons(
    branches: &[Value],
    v: &Value,
    path: &str,
    document: Option<ValueDocument<'_, '_, '_>>,
    references: &mut Vec<(String, String)>,
) -> Vec<String> {
    branches
        .iter()
        .enumerate()
        .filter_map(|(i, b)| check_value_in_document(b, v, path, document, references).err().map(|e| format!("[{i}] {e}")))
        .take(MAX_BRANCH_REASONS)
        .collect()
}

fn branch_properties<'a, 'r: 'a>(
    mut branch: &'a Value,
    document: Option<ValueDocument<'_, 'r, '_>>,
) -> Option<&'a serde_json::Map<String, Value>> {
    if let Some(properties) = branch.get("properties").and_then(Value::as_object) {
        return Some(properties);
    }
    let context = document?;
    let mut file = context.file.to_owned();
    let mut root = context.root;
    let mut visited = std::collections::BTreeSet::new();
    loop {
        let reference = branch.get("$ref")?.as_str()?;
        let target = resolve_ref(reference, &file, root, context.document).ok()?;
        if !visited.insert(target.key) {
            return None;
        }
        branch = target.target;
        if let Some(properties) = branch.get("properties").and_then(Value::as_object) {
            return Some(properties);
        }
        file = target.file;
        root = target.root;
    }
}

fn literal_tag(schema: &Value) -> Option<&Value> {
    schema.get("const").or_else(|| match schema.get("enum")?.as_array()?.as_slice() {
        [tag] => Some(tag),
        _ => None,
    })
}

/// Only promote an error when a literal on the actual object excludes every other branch.
/// Different tag fields pointing to different branches leave the original ambiguity intact.
fn discriminated_branch(
    branches: &[Value],
    value: &Value,
    document: Option<ValueDocument<'_, '_, '_>>,
) -> Option<usize> {
    let object = value.as_object()?;
    let properties: Vec<_> = branches.iter().map(|b| branch_properties(b, document)).collect::<Option<_>>()?;
    let mut selected = None;
    for (field, actual) in object {
        let tags: Option<Vec<_>> = properties.iter().map(|p| p.get(field).and_then(literal_tag)).collect();
        let Some(tags) = tags else { continue };
        let mut matches = tags.iter().enumerate().filter(|(_, tag)| **tag == actual);
        if let (Some((index, _)), None) = (matches.next(), matches.next()) {
            if selected.is_some_and(|previous| previous != index) {
                return None;
            }
            selected = Some(index);
        }
    }
    selected
}

/// Check `v` against an already resolved schema. `Err` names the instance path, the keyword
/// and, for combinators, why each branch was rejected.
pub fn check_value(schema: &Value, v: &Value, path: &str) -> Result<(), ValueError> {
    check_value_in_document(schema, v, path, None, &mut Vec::new())
}

/// Validate a document directly, resolving references only along the actual instance. This
/// accepts recursive resource trees without expanding their schemas or imposing a business
/// depth limit. A reference cycle at the same instance path is a malformed schema, not a
/// recursive child. All structural keywords use the same checker as [`check_value`].
pub fn check_document_value(
    document: &SchemaDocument<'_>,
    v: &Value,
    path: &str,
) -> Result<(), ValueError> {
    verify_schema_document(document)
        .map_err(|e| ValueError::at(path, "$schema", e))?;
    let context = ValueDocument { document, file: document.file, root: document.root };
    check_value_in_document(document.root, v, path, Some(context), &mut Vec::new())
}

fn check_value_in_document(
    schema: &Value,
    v: &Value,
    path: &str,
    document: Option<ValueDocument<'_, '_, '_>>,
    references: &mut Vec<(String, String)>,
) -> Result<(), ValueError> {
    if let Some(reference) = schema.get("$ref") {
        let document = document.ok_or_else(|| {
            ValueError::at(path, "$ref", "unresolved schema reference requires a document".into())
        })?;
        let reference = reference.as_str().ok_or_else(|| {
            ValueError::at(path, "$ref", "schema reference must be a string".into())
        })?;
        let target = resolve_ref(reference, document.file, document.root, document.document)
            .map_err(|e| ValueError::at(path, "$ref", e))?;
        let key = (target.key.clone(), path.to_string());
        if references.contains(&key) {
            return Err(ValueError::at(path, "$ref", format!("SCHEMA_REF_CYCLE: {} at {path}", target.key)));
        }
        let nested = ValueDocument {
            file: &target.file,
            root: target.root,
            document: document.document,
        };
        references.push(key);
        let result = check_value_in_document(target.target, v, path, Some(nested), references);
        references.pop();
        result?;
        // Preserve validation-bearing siblings, just as resolve_schema_document does.
        let mut siblings = schema.as_object().expect("$ref object").clone();
        siblings.remove("$ref");
        return check_value_in_document(&Value::Object(siblings), v, path, Some(document), references);
    }
    if schema == &Value::Bool(false) {
        return Err(ValueError::at(path, "false", format!("{path}: forbidden by schema")));
    }
    if let Some(branches) = schema.get("allOf").and_then(Value::as_array) {
        for branch in branches {
            check_value_in_document(branch, v, path, document, references)?;
        }
    }
    if let Some(branches) = schema.get("anyOf").and_then(Value::as_array) {
        if !branches.iter().any(|branch| check_value_in_document(branch, v, path, document, references).is_ok()) {
            let mut e = ValueError::at(path, "anyOf", format!("{path}: must match an allowed variant"));
            e.branch_reasons = branch_reasons(branches, v, path, document, references);
            return Err(e);
        }
    }
    if let Some(forbidden) = schema.get("not") {
        if check_value_in_document(forbidden, v, path, document, references).is_ok() {
            return Err(ValueError::at(path, "not", format!("{path}: forbidden combination")));
        }
    }
    if let Some(condition) = schema.get("if") {
        let arm = if check_value_in_document(condition, v, path, document, references).is_ok() { "then" } else { "else" };
        if let Some(branch) = schema.get(arm) {
            check_value_in_document(branch, v, path, document, references)?;
        }
    }
    if let Some(Value::Array(branches)) = schema.get("oneOf") {
        let matched = branches.iter().filter(|b| check_value_in_document(b, v, path, document, references).is_ok()).count();
        if matched != 1 {
            let mut e = ValueError::at(
                path,
                "oneOf",
                format!("{path}: must match exactly one variant (matched {matched})"),
            );
            if matched == 0 {
                if let Some(index) = discriminated_branch(branches, v, document) {
                    if let Err(mut cause) = check_value_in_document(&branches[index], v, path, document, references) {
                        cause.from_discriminated_branch = true;
                        return Err(cause);
                    }
                }
                e.branch_reasons = branch_reasons(branches, v, path, document, references);
            }
            return Err(e);
        }
    }
    let types: Vec<&str> = match schema.get("type") {
        Some(Value::String(s)) => vec![s.as_str()],
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    if !types.is_empty() {
        let mut any = false;
        for t in &types {
            match type_matches(t, v) {
                Ok(true) => any = true,
                Ok(false) => {}
                Err(bad) => {
                    return Err(ValueError::at(path, "type", format!("{path}: unsupported schema type {bad}")))
                }
            }
        }
        if !any {
            return Err(ValueError::at(path, "type", format!("{path}: expected {}", types.join("|"))));
        }
    }
    if let Some(e) = schema.get("enum").and_then(Value::as_array) {
        if !e.contains(v) {
            return Err(ValueError::at(path, "enum", format!("{path}: not an allowed value")));
        }
    }
    if let Some(c) = schema.get("const") {
        if c != v {
            return Err(ValueError::at(path, "const", format!("{path}: must be {c}")));
        }
    }
    if let Value::String(s) = v {
        let n = s.chars().count() as u64;
        if let Some(min) = schema.get("minLength").and_then(Value::as_u64) {
            if n < min {
                return Err(ValueError::at(path, "minLength", format!("{path}: shorter than {min} characters")));
            }
        }
        if let Some(max) = schema.get("maxLength").and_then(Value::as_u64) {
            if n > max {
                return Err(ValueError::at(path, "maxLength", format!("{path}: longer than {max} characters")));
            }
        }
    }
    if let Some(x) = v.as_f64() {
        if let Some(min) = schema.get("exclusiveMinimum").and_then(Value::as_f64) {
            if x <= min {
                return Err(ValueError::at(path, "exclusiveMinimum", format!("{path}: must exceed {min}")));
            }
        }
        if let Some(max) = schema.get("exclusiveMaximum").and_then(Value::as_f64) {
            if x >= max {
                return Err(ValueError::at(path, "exclusiveMaximum", format!("{path}: must be below {max}")));
            }
        }
        if let Some(min) = schema.get("minimum").and_then(Value::as_f64) {
            if x < min {
                return Err(ValueError::at(path, "minimum", format!("{path}: below minimum {min}")));
            }
        }
        if let Some(max) = schema.get("maximum").and_then(Value::as_f64) {
            if x > max {
                return Err(ValueError::at(path, "maximum", format!("{path}: above maximum {max}")));
            }
        }
    }
    if let Value::Object(o) = v {
        for r in schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !o.contains_key(r) {
                return Err(ValueError::at(&format!("{path}.{r}"), "required", format!("{path}.{r}: missing")));
            }
        }
        let props = schema.get("properties").and_then(Value::as_object);
        // R04: `additionalProperties` is executed in every accepted form: `false` rejects an
        // unknown field, a sub-schema checks every unknown field's value, `true`/absent allows it.
        let additional = schema.get("additionalProperties");
        for (k, x) in o {
            match (props.and_then(|p| p.get(k)), additional) {
                (Some(ps), _) => check_value_in_document(ps, x, &format!("{path}.{k}"), document, references)?,
                (None, Some(Value::Bool(false))) => {
                    return Err(ValueError::at(
                        &format!("{path}.{k}"),
                        "additionalProperties",
                        format!("{path}.{k}: not allowed"),
                    ))
                }
                (None, Some(sub @ Value::Object(_))) => check_value_in_document(sub, x, &format!("{path}.{k}"), document, references)?,
                (None, _) => {}
            }
        }
    }
    if let Value::Array(a) = v {
        if schema.get("uniqueItems").and_then(Value::as_bool) == Some(true) {
            for (i,item) in a.iter().enumerate() {
                if a[..i].contains(item) {return Err(ValueError::at(path,"uniqueItems",format!("{path}: duplicate array item")));}
            }
        }
        if let Some(min) = schema.get("minItems").and_then(Value::as_u64) {
            if (a.len() as u64) < min {
                return Err(ValueError::at(path, "minItems", format!("{path}: fewer than {min} items")));
            }
        }
        if let Some(max) = schema.get("maxItems").and_then(Value::as_u64) {
            if a.len() as u64 > max {
                return Err(ValueError::at(path, "maxItems", format!("{path}: more than {max} items")));
            }
        }
        if let Some(items) = schema.get("items") {
            match items {
                // Draft-07 tuple validation: each prefix position has its own schema.
                // Missing positions and trailing elements are unconstrained by items itself.
                Value::Array(prefix) => {
                    for (i, (item_schema, x)) in prefix.iter().zip(a).enumerate() {
                        check_value_in_document(item_schema, x, &format!("{path}[{i}]"), document, references)?;
                    }
                }
                _ => {
                    for (i, x) in a.iter().enumerate() {
                        check_value_in_document(items, x, &format!("{path}[{i}]"), document, references)?;
                    }
                }
            }
        }
    }
    Ok(())
}

/// C02 API: validate a value and return a typed violation.
pub fn validate_schema_value(
    contract_file: &str,
    schema_ref: &str,
    schema: &Value,
    value: &Value,
    path: &str,
) -> Result<(), ContractViolation> {
    check_value(schema, value, path)
        .map_err(|e| ContractViolation::new(contract_file, schema_ref, "").with_value_error(e))
}

/// Keywords whose argument must be a non-negative integer (R04).
const NON_NEGATIVE_INTEGER_KEYWORDS: &[&str] = &["minLength", "maxLength", "minItems", "maxItems", "minProperties", "maxProperties"];
/// Keywords whose argument must be a number (R04).
const NUMBER_KEYWORDS: &[&str] = &["minimum", "maximum", "exclusiveMinimum", "exclusiveMaximum"];

/// Resolution context of a schema *document* (a contract file): local `#/definitions/..` and
/// `#/$defs/..` refs resolve in `root`; `other.json#/definitions/..` refs resolve through
/// `external` (the set of contract files the consumer actually loads). R03.
pub struct SchemaDocument<'a> {
    pub file: &'a str,
    pub root: &'a Value,
    pub external: &'a dyn Fn(&str) -> Option<&'a Value>,
}

/// C03: reject a malformed schema: unknown keyword, unknown type name, empty combinator,
/// malformed `required`/`properties`/`enum`, a mistyped length/count/number argument (R04).
/// Applies to an already resolved schema: `$ref` and `definitions` are not accepted here.
pub fn verify_schema_keywords(schema: &Value, path: &str) -> Result<(), String> {
    verify_node(schema, path, None)
}

/// R03: verify a whole schema document (input or output contract) the way it is consumed:
/// every keyword as in [`verify_schema_keywords`], every `definitions`/`$defs` entry, and every
/// `$ref` must resolve to an existing definition (local or in a loaded contract file).
pub fn verify_schema_document(doc: &SchemaDocument<'_>) -> Result<(), String> {
    verify_node(doc.root, doc.file, Some(doc))
}

struct ResolvedRef<'a> {
    file: String,
    root: &'a Value,
    target: &'a Value,
    key: String,
}

fn resolve_ref<'a>(
    r: &str,
    current_file: &str,
    current_root: &'a Value,
    doc: &SchemaDocument<'a>,
) -> Result<ResolvedRef<'a>, String> {
    // `<file>` / `<file>#` is the whole document; `[file]#/definitions/<n>` / `#/$defs/<n>` a definition.
    let (file, pointer) = r.split_once('#').unwrap_or((r, ""));
    let (target_file, root): (String, &'a Value) = if file.is_empty() {
        (current_file.to_string(), current_root)
    } else {
        (
            file.to_string(),
            (doc.external)(file).ok_or_else(|| format!("SCHEMA_REF_FILE_MISSING: {r}"))?,
        )
    };
    let target = if pointer.is_empty() {
        // `#` (recursive self reference) or a whole other document.
        root
    } else {
        let (section, name) = pointer
            .strip_prefix("/definitions/")
            .map(|n| ("definitions", n))
            .or_else(|| pointer.strip_prefix("/$defs/").map(|n| ("$defs", n)))
            .ok_or_else(|| format!("SCHEMA_REF_UNSUPPORTED: {r}"))?;
        root.get(section)
            .and_then(|d| d.get(name))
            .ok_or_else(|| format!("SCHEMA_REF_TARGET_MISSING: {r}"))?
    };
    Ok(ResolvedRef {
        key: format!("{target_file}#{pointer}"),
        file: target_file,
        root,
        target,
    })
}

/// Resolve every supported JSON Schema reference into a standalone schema value. Definitions and
/// document metadata are removed; all validation-bearing siblings of `$ref` are preserved through
/// `allOf`. Missing files/targets, unsupported pointers, cycles and unsupported schema keywords are
/// explicit errors. The returned value is accepted by [`verify_schema_keywords`] and can be passed
/// directly to [`check_value`].
pub fn resolve_schema_document(doc: &SchemaDocument<'_>) -> Result<Value, String> {
    verify_schema_document(doc)?;
    let mut stack = Vec::new();
    let resolved = resolve_node(doc.root, doc.file, doc.root, doc, &mut stack)?;
    verify_schema_keywords(&resolved, doc.file)?;
    Ok(resolved)
}

fn resolve_node<'a>(
    schema: &Value,
    current_file: &str,
    current_root: &'a Value,
    doc: &SchemaDocument<'a>,
    stack: &mut Vec<String>,
) -> Result<Value, String> {
    let object = match schema {
        Value::Bool(_) => return Ok(schema.clone()),
        Value::Object(object) => object,
        _ => {
            return Err(format!(
                "SCHEMA_INVALID: {current_file} must be an object or boolean"
            ))
        }
    };

    if let Some(reference) = object.get("$ref") {
        let reference = reference
            .as_str()
            .ok_or_else(|| "SCHEMA_REF_UNSUPPORTED: $ref must be a string".to_string())?;
        let target = resolve_ref(reference, current_file, current_root, doc)?;
        if stack.iter().any(|item| item == &target.key) {
            return Err(format!(
                "SCHEMA_REF_CYCLE: {} -> {}",
                stack.join(" -> "),
                target.key
            ));
        }
        stack.push(target.key.clone());
        let resolved_target = resolve_node(target.target, &target.file, target.root, doc, stack)?;
        stack.pop();

        let mut siblings = object.clone();
        siblings.remove("$ref");
        siblings.remove("definitions");
        siblings.remove("$defs");
        siblings.remove("schemaVersion");
        if siblings.is_empty() {
            return Ok(resolved_target);
        }
        let resolved_siblings = resolve_node(
            &Value::Object(siblings),
            current_file,
            current_root,
            doc,
            stack,
        )?;
        // An annotation does not constrain the referenced value. Keep its concrete
        // object/array type visible to native tool schemas instead of creating an
        // unnecessary allOf around a type and a description. Validation siblings
        // still take the intersection path below.
        if let (Some(target), Some(notes)) = (resolved_target.as_object(), resolved_siblings.as_object()) {
            if notes.keys().all(|key| ANNOTATION_KEYWORDS.contains(&key.as_str()) || key.starts_with("x-")) {
                let mut merged = target.clone();
                merged.extend(notes.clone());
                return Ok(Value::Object(merged));
            }
        }
        return Ok(serde_json::json!({"allOf": [resolved_target, resolved_siblings]}));
    }

    let mut resolved = serde_json::Map::new();
    for (key, value) in object {
        let next = match key.as_str() {
            "definitions" | "$defs" | "schemaVersion" => continue,
            "properties" => {
                let properties = value
                    .as_object()
                    .ok_or_else(|| format!("SCHEMA_PROPERTIES_INVALID: {current_file}/properties"))?;
                let mut out = serde_json::Map::new();
                for (name, property) in properties {
                    out.insert(
                        name.clone(),
                        resolve_node(property, current_file, current_root, doc, stack)?,
                    );
                }
                Value::Object(out)
            }
            "allOf" | "anyOf" | "oneOf" => Value::Array(
                value
                    .as_array()
                    .ok_or_else(|| format!("SCHEMA_COMBINATOR_EMPTY: {current_file}/{key}"))?
                    .iter()
                    .map(|branch| resolve_node(branch, current_file, current_root, doc, stack))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            "items" if value.is_array() => Value::Array(
                value.as_array().expect("tuple items").iter()
                    .map(|position| resolve_node(position, current_file, current_root, doc, stack))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            "not" | "if" | "then" | "else" | "items" => {
                resolve_node(value, current_file, current_root, doc, stack)?
            }
            "additionalProperties" if !value.is_boolean() => {
                resolve_node(value, current_file, current_root, doc, stack)?
            }
            _ => value.clone(),
        };
        resolved.insert(key.clone(), next);
    }
    Ok(Value::Object(resolved))
}

fn verify_node(schema: &Value, path: &str, doc: Option<&SchemaDocument<'_>>) -> Result<(), String> {
    let obj = match schema {
        Value::Bool(_) => return Ok(()),
        Value::Object(o) => o,
        _ => return Err(format!("SCHEMA_INVALID: {path} must be an object or boolean")),
    };
    for (k, v) in obj {
        let here = format!("{path}/{k}");
        // Document mode: refs/definitions anywhere, and the contract's own integer
        // `schemaVersion` metadata at the document root only.
        let document_keyword = doc.is_some_and(|d| {
            matches!(k.as_str(), "$ref" | "definitions" | "$defs")
                || (k == "schemaVersion" && path == d.file && v.is_u64())
        });
        let known = STRUCTURAL_KEYWORDS.contains(&k.as_str())
            || DELEGATED_KEYWORDS.contains(&k.as_str())
            || ANNOTATION_KEYWORDS.contains(&k.as_str())
            || document_keyword
            || k.starts_with("x-");
        if !known {
            return Err(format!("SCHEMA_KEYWORD_UNSUPPORTED: {here}"));
        }
        if NON_NEGATIVE_INTEGER_KEYWORDS.contains(&k.as_str()) && v.as_u64().is_none() {
            return Err(format!("SCHEMA_KEYWORD_ARGUMENT_INVALID: {here} must be a non-negative integer, got {v}"));
        }
        if NUMBER_KEYWORDS.contains(&k.as_str()) && !v.is_number() {
            return Err(format!("SCHEMA_KEYWORD_ARGUMENT_INVALID: {here} must be a number, got {v}"));
        }
        if k == "uniqueItems" && !v.is_boolean() {return Err(format!("SCHEMA_KEYWORD_ARGUMENT_INVALID: {here} must be boolean"));}
        match k.as_str() {
            "$ref" => {
                let r = v.as_str().ok_or_else(|| format!("SCHEMA_REF_UNSUPPORTED: {here} must be a string"))?;
                if let Some(d) = doc {
                    resolve_ref(r, d.file, d.root, d).map_err(|e| format!("{e} at {here}"))?;
                }
            }
            "definitions" | "$defs" => {
                let defs = v.as_object().ok_or_else(|| format!("SCHEMA_DEFINITIONS_INVALID: {here}"))?;
                for (name, s) in defs {
                    verify_node(s, &format!("{here}/{name}"), doc)?;
                }
            }
            "type" => {
                let names: Vec<&Value> = match v {
                    Value::String(_) => vec![v],
                    Value::Array(a) if !a.is_empty() => a.iter().collect(),
                    _ => return Err(format!("SCHEMA_TYPE_INVALID: {here}")),
                };
                for n in names {
                    let ok = n.as_str().is_some_and(|s| TYPE_NAMES.contains(&s));
                    if !ok {
                        return Err(format!("SCHEMA_TYPE_INVALID: {here} {n}"));
                    }
                }
            }
            "allOf" | "anyOf" | "oneOf" => {
                let a = v
                    .as_array()
                    .filter(|a| !a.is_empty())
                    .ok_or_else(|| format!("SCHEMA_COMBINATOR_EMPTY: {here}"))?;
                for (i, b) in a.iter().enumerate() {
                    verify_node(b, &format!("{here}/{i}"), doc)?;
                }
            }
            "items" if v.is_array() => {
                let positions = v.as_array().filter(|positions| !positions.is_empty())
                    .ok_or_else(|| format!("SCHEMA_ITEMS_INVALID: {here} tuple must be non-empty"))?;
                for (index, position) in positions.iter().enumerate() {
                    verify_node(position, &format!("{here}/{index}"), doc)?;
                }
            }
            "not" | "if" | "then" | "else" | "items" => verify_node(v, &here, doc)?,
            "additionalProperties" if !v.is_boolean() => verify_node(v, &here, doc)?,
            "additionalProperties" => {}
            "properties" => {
                let p = v.as_object().ok_or_else(|| format!("SCHEMA_PROPERTIES_INVALID: {here}"))?;
                for (name, s) in p {
                    verify_node(s, &format!("{here}/{name}"), doc)?;
                }
            }
            "required" => {
                let a = v.as_array().ok_or_else(|| format!("SCHEMA_REQUIRED_INVALID: {here}"))?;
                let mut seen = std::collections::BTreeSet::new();
                for r in a {
                    let s = r.as_str().ok_or_else(|| format!("SCHEMA_REQUIRED_INVALID: {here}"))?;
                    if !seen.insert(s) {
                        return Err(format!("SCHEMA_REQUIRED_DUPLICATE: {here} {s}"));
                    }
                }
            }
            "enum" => {
                let a = v
                    .as_array()
                    .filter(|a| !a.is_empty())
                    .ok_or_else(|| format!("SCHEMA_ENUM_INVALID: {here}"))?;
                for (i, x) in a.iter().enumerate() {
                    if a[..i].contains(x) {
                        return Err(format!("SCHEMA_ENUM_DUPLICATE: {here} {x}"));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn draft07_tuple_items_enforce_positions_and_compose_with_shared_item_shape() {
        let schema = json!({"type":"array", "minItems":2,"maxItems":2,
            "items":{"type":"object","required":["id","text"],"additionalProperties":false,
                "properties":{"id":{"type":"string"},"text":{"type":"string","minLength":1}}},
            "allOf":[{"items":[{"properties":{"id":{"const":"first"}}},
                {"properties":{"id":{"const":"second"}}}]}]});
        verify_schema_keywords(&schema, "tuple").expect("Draft-07 tuple is a schema");
        let good = json!([{"id":"first","text":"a"},{"id":"second","text":"b"}]);
        check_value(&schema, &good, "value").unwrap();
        let reversed = json!([{"id":"second","text":"b"},{"id":"first","text":"a"}]);
        let error = check_value(&schema, &reversed, "value").expect_err("order is frozen");
        assert_eq!(error.instance_path, "value[0].id");
        assert_eq!(error.schema_path, "const");
        let mut missing = good.clone();
        missing[1].as_object_mut().unwrap().remove("text");
        assert_eq!(check_value(&schema, &missing, "value").unwrap_err().instance_path, "value[1].text");
        assert!(check_value(&schema, &json!([]), "value").is_err());
        let mut extra = good.as_array().unwrap().clone();
        extra.push(json!({"id":"third","text":"c"}));
        assert!(check_value(&schema, &json!(extra), "value").is_err());
    }

    #[test]
    fn draft07_tuple_items_leave_short_and_trailing_values_to_explicit_count_bounds() {
        let schema = json!({"type":"array","items":[{"type":"string"}, false]});
        verify_schema_keywords(&schema, "tuple").unwrap();
        check_value(&schema, &json!([]), "value").expect("tuple does not require presence");
        check_value(&schema, &json!(["a"]), "value").unwrap();
        assert_eq!(check_value(&schema, &json!(["a",null]), "value").unwrap_err().instance_path, "value[1]");
        let trailing = json!({"items":[{"const":"first"}]});
        check_value(&trailing, &json!(["first", 99, null]), "value").expect("unconstrained trailing items");
        assert!(check_value(&trailing, &json!(["wrong"]), "value").is_err());
        assert!(check_value(&json!({"items":{"type":"integer"}}), &json!([1,"wrong"]), "value").is_err());
        assert!(check_value(&json!({"items":false}), &json!([1]), "value").is_err());
    }

    #[test]
    fn draft07_tuple_items_resolve_nested_local_and_external_definitions() {
        let root = json!({"type":"array","items":[{"$ref":"#/definitions/local"},
            {"$ref":"shared.json#/definitions/root"}],"definitions":{"local":{"const":"one"}}});
        let shared = json!({"definitions":{"root":{"type":"array","items":[{"$ref":"#/definitions/value"}]},
            "value":{"const":2}}});
        let external = |file: &str| (file == "shared.json").then_some(&shared);
        let doc = SchemaDocument { file:"tuple.json",root:&root,external:&external };
        let resolved = resolve_schema_document(&doc).expect("tuple refs resolve in their document");
        verify_schema_keywords(&resolved, "resolved").unwrap();
        check_value(&resolved, &json!(["one",[2]]), "value").unwrap();
        assert_eq!(check_value(&resolved, &json!(["one",[3]]), "value").unwrap_err().instance_path, "value[1][0]");
        assert!(resolved["items"][0].get("$ref").is_none());
    }

    #[test]
    fn draft07_tuple_items_reject_empty_or_invalid_position_schemas() {
        for schema in [json!({"items":[]}),json!({"items":[7]}),json!({"items":[{"typo":true}]})] {
            assert!(verify_schema_keywords(&schema, "bad").is_err(), "{schema}");
        }
    }

    #[test]
    fn schema_resolution_preserves_nested_arrays_nullable_combinators_and_closed_objects() {
        let root = json!({
            "$ref": "#/definitions/root",
            "description": "Closed model result",
            "definitions": {
                "root": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["lines"],
                    "properties": {
                        "lines": {
                            "$ref": "#/definitions/lineList",
                            "description": "Declared line list"
                        }
                    }
                },
                "lineList": {"type":"array", "minItems":1, "items":{"$ref":"#/definitions/line"}},
                "line": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["ids", "note", "mode"],
                    "properties": {
                        "ids": {"type": "array", "items": {"type": "string"}},
                        "note": {"type": ["string", "null"]},
                        "mode": {"oneOf": [{"const": "a"}, {"const": "b"}]}
                    }
                }
            }
        });
        let external = |_: &str| -> Option<&Value> { None };
        let doc = SchemaDocument {
            file: "root.json",
            root: &root,
            external: &external,
        };
        let resolved = resolve_schema_document(&doc).expect("schema resolves");
        assert!(resolved.get("$ref").is_none());
        assert!(resolved.get("definitions").is_none());
        assert_eq!(resolved["type"], "object");
        assert_eq!(resolved["properties"]["lines"]["type"], "array");
        assert_eq!(resolved["properties"]["lines"]["minItems"], 1);

        let equivalent = json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["lines"],
            "properties": {
                "lines": {
                    "type": "array",
                    "minItems": 1,
                    "items": root["definitions"]["line"].clone()
                }
            }
        });
        let cases = [
            json!({"lines": [{"ids": [], "note": null, "mode": "a"}]}),
            json!({"lines": [{"ids": ["x"], "note": "ok", "mode": "b"}]}),
            json!({"lines": []}),
            json!({"lines": ""}),
            json!({"lines": {"item": []}}),
            json!({"lines": [{"ids": [], "note": null, "mode": "c"}]}),
            json!({"lines": [{"ids": [], "note": null, "mode": "a", "extra": 1}]}),
            json!({}),
        ];
        for (index, value) in cases.iter().enumerate() {
            assert_eq!(
                check_value(&resolved, value, "value").is_ok(),
                check_value(&equivalent, value, "value").is_ok(),
                "case {index}"
            );
        }
        assert_eq!(resolved["required"], equivalent["required"]);
        assert_eq!(
            resolved["additionalProperties"],
            equivalent["additionalProperties"]
        );
    }

    #[test]
    fn schema_resolution_keeps_external_document_context_for_nested_local_refs() {
        let root = json!({"$ref": "shared.json#/definitions/root"});
        let shared = json!({
            "definitions": {
                "root": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["line"],
                    "properties": {"line": {"$ref": "#/definitions/line"}}
                },
                "line": {"type": "string", "minLength": 1}
            }
        });
        let external = |file: &str| (file == "shared.json").then_some(&shared);
        let doc = SchemaDocument {
            file: "root.json",
            root: &root,
            external: &external,
        };

        let resolved = resolve_schema_document(&doc).expect("external schema resolves");
        check_value(&resolved, &json!({"line": "kept"}), "value")
            .expect("nested local ref uses the external document root");
        assert!(check_value(&resolved, &json!({"line": ""}), "value").is_err());
        assert!(check_value(&resolved, &json!({"line": "kept", "extra": true}), "value")
            .is_err());
    }

    #[test]
    fn schema_resolution_reports_missing_cycle_and_unsupported_keyword() {
        for (root, expected) in [
            (
                json!({"$ref": "#/definitions/missing", "definitions": {}}),
                "SCHEMA_REF_TARGET_MISSING",
            ),
            (
                json!({
                    "$ref": "#/definitions/a",
                    "definitions": {
                        "a": {"$ref": "#/definitions/b"},
                        "b": {"$ref": "#/definitions/a"}
                    }
                }),
                "SCHEMA_REF_CYCLE",
            ),
            (
                json!({"$ref": "#/definitions/a", "definitions": {"a": {"typo": true}}}),
                "SCHEMA_KEYWORD_UNSUPPORTED",
            ),
        ] {
            let external = |_: &str| -> Option<&Value> { None };
            let doc = SchemaDocument {
                file: "bad.json",
                root: &root,
                external: &external,
            };
            let error = resolve_schema_document(&doc).expect_err("invalid document must fail");
            assert!(error.contains(expected), "{error}");
        }
    }
}
