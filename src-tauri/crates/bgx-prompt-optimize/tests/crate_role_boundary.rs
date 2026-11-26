//! T30 crate-role boundary: parse the manifest dependency table, not the prose.

const CARGO: &str = include_str!("../Cargo.toml");

fn dependency_table(cargo: &str) -> &str {
    let start = cargo
        .find("[dependencies]")
        .expect("bgx-prompt-optimize must declare [dependencies]");
    let body = &cargo[start + "[dependencies]".len()..];
    match body.find("\n[") {
        Some(end) => &body[..end],
        None => body,
    }
}

#[test]
fn prompt_optimize_manifest_stays_pure_core() {
    let deps = dependency_table(CARGO);
    for forbidden in [
        "tauri", "rusqlite", "sqlx", "reqwest", "tokio", "hyper", "axum",
    ] {
        assert!(
            !deps
                .lines()
                .any(|line| line.trim_start().starts_with(forbidden)),
            "pure-core crate must not depend on {forbidden}:\n{deps}"
        );
    }
    assert!(
        deps.contains("bgx-video-prompt-contract"),
        "core must keep the official video prompt contract dependency"
    );
}
