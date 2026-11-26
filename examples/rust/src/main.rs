use std::path::Path;

use bgx_prompt_optimize::sdk::{
    self,
    protocol::{OptimizeArgs, TextMode},
};
use bgx_prompt_optimize::{OptimizeError, TextCall, TextCompleter, TextReply};
use prompt_optimize_runtime::{config::ShellConfig, materials::StandaloneMaterials};

struct FixtureModel;

impl TextCompleter for FixtureModel {
    fn complete(&mut self, call: TextCall) -> Result<TextReply, OptimizeError> {
        assert!(call.schema.is_some());
        Ok(TextReply {
            content: r#"{"optimizedPrompt":"红色公交车驶过雨夜街口，街灯映着湿润路面。"}"#.into(),
            finish_reason: None,
            raw_json: None,
        })
    }
}

fn main() {
    assert_eq!(std::env::args().nth(1).as_deref(), Some("--self-test"));
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let resources = repository.join("src-tauri/resources");
    let config = ShellConfig {
        internal_template_root: Some(
            repository
                .join("src-tauri/crates/bgx-internal-prompts/resources/internal-prompt-skills")
                .to_string_lossy()
                .into_owned(),
        ),
        ..Default::default()
    };
    let materials = StandaloneMaterials::new(
        &config,
        &repository.join("target/example-data"),
        Some(&resources),
        None,
    )
    .expect("standalone materials");
    let output = sdk::optimize(
        "demo",
        OptimizeArgs {
            target_id: "image/qwen-image-2.1".into(),
            source_prompt: "雨夜街口的红色公交车。".into(),
            text_mode: TextMode::Callback,
            expected_method_digest: None,
        },
        "zh",
        &materials,
        &mut FixtureModel,
    )
    .expect("optimize image prompt");
    println!("{}", output.optimized_prompt);
}
