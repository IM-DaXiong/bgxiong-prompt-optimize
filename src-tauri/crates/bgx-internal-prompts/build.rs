use std::path::Path;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    for name in ["contracts/package.schema.json", "contracts/frozen-rules.schema.json"] {
        let path = Path::new(&manifest).join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        let value: serde_json::Value = serde_json::from_str(&text)
            .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()));
        let version = value
            .pointer("/properties/formatVersion/const")
            .or_else(|| value.pointer("/$defs/requestPackage/properties/formatVersion/const"))
            .and_then(|item| item.as_u64());
        if version != Some(2) {
            panic!("{name} formatVersion const must be 2");
        }
    }
}
