use bgx_skill_distribution::{
    archive::read_zip_bytes,
    catalog::{OFFICIAL_KEY, OFFICIAL_KEY_ID, parse_catalog},
    index::accept_sequence,
    package::verify_zip,
};

#[test]
fn unsigned_or_malformed_catalog_is_rejected() {
    let error = parse_catalog(br#"{"sequence":1}"#, OFFICIAL_KEY_ID, OFFICIAL_KEY)
        .err().expect("unsigned catalog");
    assert_eq!(error.code, "ONLINE_INDEX_INVALID");
}

#[test]
fn signed_sequence_cannot_go_back_or_be_reused_with_other_bytes() {
    assert!(accept_sequence(4, Some("old"), 3, "old").is_err());
    assert!(accept_sequence(4, Some("old"), 4, "new").is_err());
    assert!(accept_sequence(4, Some("old"), 4, "old").is_ok());
    assert!(accept_sequence(4, Some("old"), 5, "new").is_ok());
}

#[test]
fn invalid_archive_is_rejected_in_memory() {
    assert!(read_zip_bytes("invalid.zip", b"not a ZIP").is_err());
}

#[test]
#[ignore = "requires production index and package bytes in BGX_SKILL_PRODUCTION_FIXTURE_DIR"]
fn production_packages_share_catalog_and_zip_validation() {
    let root = std::path::PathBuf::from(std::env::var_os("BGX_SKILL_PRODUCTION_FIXTURE_DIR").unwrap());
    let envelope = std::fs::read(root.join("latest.json")).unwrap();
    let (catalog, _) = parse_catalog(&envelope, OFFICIAL_KEY_ID, OFFICIAL_KEY).unwrap();
    for name in ["qwen-image-2-1-prompt-writing", "kling-prompt-writing"] {
        let release = catalog.packages.iter().find(|row| row.package_id == name).unwrap();
        let mut archive = std::fs::read(root.join(&release.file)).unwrap();
        verify_zip(release, &archive).unwrap();
        archive[0] ^= 1;
        assert!(verify_zip(release, &archive).is_err());
    }
}
