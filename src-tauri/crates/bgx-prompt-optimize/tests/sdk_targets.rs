use bgx_prompt_optimize::sdk::{target_catalog, TargetKind};

#[test]
fn public_targets_use_registered_text_modes_and_exact_method_bindings() {
    let catalog = target_catalog().expect("registered prompt targets");
    let videos: Vec<_> = catalog
        .iter()
        .filter(|target| target.kind == TargetKind::Video)
        .collect();
    assert_eq!(videos.len(), 8);
    assert!(videos
        .iter()
        .all(|target| target.input_modes == ["text_to_video"]));
    let ltx = videos
        .iter()
        .find(|target| target.target_id == "video/ltx@1")
        .unwrap();
    assert_eq!(ltx.method_id.as_deref(), Some("ltx-2-5-prompt-writing"));
    let image = catalog
        .iter()
        .find(|target| target.target_id == "image/qwen-image-2.1")
        .unwrap();
    assert_eq!(
        image.method_id.as_deref(),
        Some("qwen-image-2-1-prompt-writing")
    );
}
