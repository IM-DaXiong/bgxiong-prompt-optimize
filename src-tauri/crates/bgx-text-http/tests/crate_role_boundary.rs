//! T30 crate-role boundary for the shared text HTTP exits.
//! Outbound request-shape coverage already lives in src unit tests and is cited
//! in the prompt-optimize T30 matrix; this file does not rewrite those fixtures.

const CARGO: &str = include_str!("../Cargo.toml");

fn dependency_table(cargo: &str) -> &str {
    let start = cargo
        .find("[dependencies]")
        .expect("bgx-text-http must declare [dependencies]");
    let body = &cargo[start + "[dependencies]".len()..];
    match body.find("\n[") {
        Some(end) => &body[..end],
        None => body,
    }
}

#[test]
fn text_http_manifest_stays_http_client_without_db_or_host() {
    let deps = dependency_table(CARGO);
    for forbidden in ["tauri", "rusqlite", "sqlx", "libsqlite", "diesel"] {
        assert!(
            !deps
                .lines()
                .any(|line| line.trim_start().starts_with(forbidden)),
            "http-client crate must not depend on {forbidden}:\n{deps}"
        );
    }
    assert!(
        deps.lines()
            .any(|line| line.trim_start().starts_with("reqwest")),
        "http-client crate must keep reqwest:\n{deps}"
    );
}
