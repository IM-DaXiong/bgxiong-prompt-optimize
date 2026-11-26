# Lane A handoff (schema + T34 goldens)

Integration HEAD referenced: `978e84b639c40341af0dc58d64667d6c88cbb24e`.
Contract: `docs/dev/reports/prompt-optimize-standalone/FROZEN-INTERFACE-20261116.md`.
This lane did not compile, did not edit `lib.rs` / `Cargo.toml` / host / `apps/text-http`.

## Types (align to D, no lib edit)

- `TextImageCountPolicy` and `TextCall.image_count_policy` are in `text_port.rs`.
- `run_image` sets the policy from `VisionChannel` (`from_vision_channel`).
- Literary, video expression, video content stages, and compress use `ModelCapability`.
- Never default `AsSubmitted` down to `ModelCapability`.

## Expression schema root-cause fix

`run_video_expression` previously set `TextCall.schema = None` and built the closed set only after `complete`. That left the host `draft_schema_for(closed)` enums out of the request.

Fix (core, not lib):

- New file `src/expression_schema.rs`, declared from `video_job.rs` via `#[path]` so D's `lib.rs` is untouched. `pub use draft_schema_for`.
- Official contracts are `include_str!` from `packages/app-contracts/v1/` (`video-prompt-projection-draft.schema.json`, `video-prompt-channels.schema.json`, `agent-skills.json`).
- `$ref` inliner follows `bgx_creator_contract::resolve_schema_document` (sibling `$ref` merge via `allOf` unless annotation / `x-*` keywords). `verify_schema_keywords` is not ported; shipped contract files are the source of truth.
- Closed set is built **before** `complete`. `TextCall.schema = Some(draft_schema_for(&closed))`.
- `parse_expression_reply` still does JSON + serde + `validate_projection_draft`. That is not a substitute for the transport schema.

`VideoExpressionRequest` still has no expansion / negative fields, so the run uses `ProjectionClosedSet::new(mother, target)` (policy and negative are `None`). The generator already implements those branches when a closed set is built with `with_expansion_policy` / `with_negative_context`. `mother_fact_relations()` remains empty, so expansion still omits `proposedFacts.items.allOf`.

## Report to D (lib / contract ownership only)

1. Long-term SSOT: move `draft_schema_for` + the `$ref` inliner into `bgx-video-prompt-contract`. That exceeds this lane's original scope; D should change the public registry and new-file ownership. Do not add a `bgx_creator_contract` dependency to the optimize crate.
2. After that move (or until then), re-export `draft_schema_for` from `bgx-prompt-optimize` `lib.rs` if host/C need it (`video_job` already `pub use`s it).
3. If a host expression run can carry expansion or a frozen negative, add optional `expansion_policy` and `negative_context` on `VideoExpressionRequest` and pass them into the closed set before schema generation. Do not invent a second policy table.
4. No further `Cargo.toml` type supplements are required for this fix.

## T34 goldens

The remaining seven profiles now have full expected final prompts on the shared fixture, derived from compile rules (not non-empty checks, not first-output copies). Ready for the integrator compile.

- generic@1, wan@1, kling.classic@1, kling.omni@1, kling.omni@2 share the `Shot N` + ASCII-quote block (Kling dialects only differ when tagged references exist).
- seedance@1 uses `{你好，世界！}` for the dialogue role.
- h3@1 uses the T2VA three-field CIR layout, `(S1) <d>[Chinese] …</d>`, and official empty soundscape / N/A music placeholders.
- ltx@1 stays the existing exact sentence.
