# T30 offline contract regression matrix

Lane A exclusive evidence file for plan §6 / T30 **offline contract** rows that the core and `bgx-text-http` can close without host UI, installers, or paid model calls. This file does not replace `docs/dev/reports/prompt-optimize-standalone/ACCEPTANCE.md` (T32, D closes) or `docs/dev/reports/prompt-optimize-standalone/REGRESSION-MATRIX.md` (existing D file, untouched).

Recorded: 2026-11-16, Asia/Shanghai (UTC+8).

Tree: worktree `L:\00Dev\bgxiong-ai-story-prompt-optimize-A-20261116`, branch `codex/prompt-optimize-A-20261116`, clean fast-forward onto integration `9a03f482b41a68ed3a2d47a4ccefc6939631b757`, plus the T30 test commit on this branch.

Status rules:

- `PASS` only when a named command already ran and the named test passed. Source existing is not a pass.
- `NOT_RUN` for every new test in this commit, for `--test golden_video` (those 9 tests were never part of the 111 lib run), and for any host / install / real-model / user-path cell. D runs the batch centrally.
- Real-model and user-path columns stay `NOT_RUN` in this file; they are not claimed from offline fixtures.

Known 111-test evidence used below: centralized `cargo test -p bgx-prompt-optimize --lib` on the integrated tree at `9a03f482b4` (all 111 lib unit tests passed). This A tree did not re-run that suite.

Commands D should run for the new files (not executed here):

```text
cargo test -p bgx-prompt-optimize --test golden_video --test expression_closed_set_schema --test image_attach_contract --test run_video_optimize_contract --test crate_role_boundary
cargo test -p bgx-text-http --test crate_role_boundary
```

## 1. Newly added tests (this commit) — all NOT_RUN until D runs them

| ID | Case | Expected | Status | Evidence / command |
| --- | --- | --- | --- | --- |
| N01 | Expression `TextCall.schema` is `draft_schema_for(closed)` built before `complete` | `call.schema == Some(draft_schema_for(closed))`; shot/fact enums are the request closed set; `not-a-fact` is absent | NOT_RUN | `expression_closed_set_schema::run_video_expression_sends_closed_set_schema_on_text_call`. This path was the original `schema: None` gap. Lib unit `expression_schema::draft_schema_binds_closed_set_enums_and_does_not_allow_unknown_facts` only tests the generator, not injection into `TextCall`. |
| N02 | Expression success output fields | `draft` and `final_prompt` are `Some`, `rejected` is `None`; `final_prompt` equals `compiler_registry().projector(generic@1).compile(draft)` | NOT_RUN | `expression_closed_set_schema::run_video_expression_success_final_prompt_matches_compiler` |
| N03 | Expression closed-set rejection output fields | `rejected` is `Some` with `raw_reply` preserved; `draft` and `final_prompt` are `None`; schema was still sent | NOT_RUN | `expression_closed_set_schema::run_video_expression_rejects_fact_outside_closed_set_without_final_prompt`. Complements lib unit `video_job::expression_rejects_fact_ids_outside_the_closed_set` (that unit calls `parse_expression_reply` only). |
| N04 | Expression cancel before `complete` | `OPTIMIZE_CANCELLED`; completer not called | NOT_RUN | `expression_closed_set_schema::run_video_expression_cancel_before_complete_does_not_read_reply` |
| N05 | Unregistered projector (HappyHorse) | `VIDEO_PROMPT_PROJECTOR_MISSING`; message says it will not switch to generic; no complete | NOT_RUN | `expression_closed_set_schema::unregistered_projector_is_refused_without_generic_fallback` |
| N06 | Unsupported input mode | `VIDEO_PROMPT_INPUT_MODE_UNSUPPORTED`; no complete | NOT_RUN | `expression_closed_set_schema::unsupported_input_mode_is_refused_before_complete` |
| N07 | Image 0 attachments + output schema on `TextCall` | no `image_url` parts; `input_mode=text`; `schema == Some(output_schema())`; success fields `optimized_prompt` / `raw_reply` / `finish_reason` | NOT_RUN | `image_attach_contract::as_submitted_zero_images_is_text_only_and_sends_output_schema` |
| N08 | Image 3 attachments keep submission order | `TextCall` image URLs equal `encode_original` of images 1..3; policy `AsSubmitted`; `attached_image_count=3` | NOT_RUN | `image_attach_contract::as_submitted_three_images_keep_submission_order_on_text_call`. Lib tests only covered a single attached image. |
| N09 | Image 17 as-submitted reject before complete (public API) | `IMAGE_PROMPT_REFERENCE_LIMIT`; no complete | NOT_RUN | `image_attach_contract::as_submitted_seventeen_images_fail_before_complete`. Public-API twin of already-passed lib `run_image_as_submitted_rejects_over_cap_before_complete` / `vision::seventeen_images_channel_a_errors_and_channel_b_caps`. |
| N10 | Optional channel 17 keeps first `MAX_IPC_RELS` in order | URLs match first N originals; `vision_truncated`; policy `ModelCapability` | NOT_RUN | `image_attach_contract::optional_channel_keeps_first_max_ipc_rels_in_submission_order`. Lib optional test only covered `supports_vision=false` (skip). The vision unit cap test does not assert `declared_order`. |
| N11 | `run_video_optimize` stage order + expression schema + compiled prompt | step labels `video-content-optimize` then `video-expression`; expression `schema == draft_schema_for(closed)`; `final_prompt` equals compiler output | NOT_RUN | `run_video_optimize_contract::run_video_optimize_runs_content_then_expression_with_closed_set_schema`. No prior public-API test for this entry except compress-only lib tests. |
| N12 | `bgx-prompt-optimize` manifest stays `pure-core` | `[dependencies]` has no tauri/rusqlite/sqlx/reqwest/tokio/hyper/axum; keeps `bgx-video-prompt-contract` | NOT_RUN | `crate_role_boundary::prompt_optimize_manifest_stays_pure_core` |
| N13 | `bgx-text-http` manifest stays `http-client` | `[dependencies]` has no tauri/rusqlite/sqlx; keeps reqwest | NOT_RUN | `bgx-text-http` `--test crate_role_boundary::text_http_manifest_stays_http_client_without_db_or_host` |

## 2. Eight-profile goldens (T34 / A6 offline) — already written, not re-authored

Existing file `tests/golden_video.rs` already has one exact compiler golden per production projector plus the registry contract. A second weaker set was not added.

| ID | Case | Expected | Status | Evidence / command |
| --- | --- | --- | --- | --- |
| G00 | Registry is the official eight, no HappyHorse; LTX `default_method_name=None`; `kling.omni@2` has no `reference_to_video` | exact roster assertions in the test | NOT_RUN | `golden_video::golden_registry_has_eight_projectors_and_no_happyhorse`. Not part of the 111 `--lib` run. |
| G01 | H3 golden | exact `H3_GOLDEN` three-field CIR string | NOT_RUN | `golden_video::golden_h3` |
| G02 | Seedance golden | exact `SEEDANCE_GOLDEN` (`{你好，世界！}`) | NOT_RUN | `golden_video::golden_seedance` |
| G03 | Kling classic golden | exact `SHOT_BLOCK_ASCII_QUOTES` | NOT_RUN | `golden_video::golden_kling_classic` |
| G04 | Kling Omni legacy golden | same fixture text (dialects diverge only with tagged refs) | NOT_RUN | `golden_video::golden_kling_omni_legacy` |
| G05 | Kling Omni nextgen golden | same fixture text | NOT_RUN | `golden_video::golden_kling_omni_nextgen` |
| G06 | Wan golden | exact `SHOT_BLOCK_ASCII_QUOTES` | NOT_RUN | `golden_video::golden_wan` |
| G07 | LTX golden + quote span | exact sentence `她轻声说。 \"你好，世界！\" Then she turns.` and quote span bytes | NOT_RUN | `golden_video::golden_ltx_matches_existing_projector_assertion` |
| G08 | Generic golden | exact `SHOT_BLOCK_ASCII_QUOTES` | NOT_RUN | `golden_video::golden_generic` |

Per-projector compile / mode / quote / lock unit tests in `src/projectors/**` already passed in the 111 `--lib` run; they are supporting evidence, not a second golden set.

## 3. Already-passed lib coverage (cite only, not rewritten)

Status `PASS` refers only to `cargo test -p bgx-prompt-optimize --lib` at integration `9a03f482b4` (111 passed).

| ID | Case | Expected | Status | Evidence |
| --- | --- | --- | --- | --- |
| L01 | Closed-set schema generator binds enums, forbids unknown facts, `proposedFacts.maxItems=0` | structural schema asserts | PASS | `expression_schema::draft_schema_binds_closed_set_enums_and_does_not_allow_unknown_facts` @ 9a03f482b4 `--lib` |
| L02 | `parse_expression_reply` rejects fact ids outside the closed set and keeps `raw_reply` | rejected code/detail + raw | PASS | `video_job::expression_rejects_fact_ids_outside_the_closed_set` @ 9a03f482b4 `--lib` |
| L03 | Image cancel before `complete` | `OPTIMIZE_CANCELLED`; completer not called | PASS | `run::run_image_cancel_before_complete_does_not_read_reply` @ 9a03f482b4 `--lib` |
| L04 | Image cancel after `complete` | `OPTIMIZE_CANCELLED`; prompt not returned | PASS | `run::run_image_cancel_after_complete_does_not_return_prompt` @ 9a03f482b4 `--lib` |
| L05 | Text-port cancel hook does not read a reply | `OPTIMIZE_CANCELLED` | PASS | `text_port::cancelled_callback_does_not_read_a_reply` @ 9a03f482b4 `--lib` |
| L06 | Image as-submitted attaches original bytes (1 image) | `input_mode=vision`, count 1 | PASS | `run::run_image_as_submitted_attaches_original_image` @ 9a03f482b4 `--lib` |
| L07 | Image optional + no vision skips attachments even when over cap | text mode, 0 attached | PASS | `run::run_image_optional_channel_caps_and_skips_without_vision` @ 9a03f482b4 `--lib` |
| L08 | Image as-submitted over cap fails before complete | Chinese 上限 message; no complete | PASS | `run::run_image_as_submitted_rejects_over_cap_before_complete` @ 9a03f482b4 `--lib` |
| L09 | Vision 0 / no-vision / cap 4-of-6 / cap cloud / IPC constant / 17 A error B cap / original bytes / encode errors | counts and error codes | PASS | `vision::*` including `zero_images_is_text_only`, `seventeen_images_channel_a_errors_and_channel_b_caps`, `ipc_max_rels_matches_app_contract` @ 9a03f482b4 `--lib` |
| L10 | Literary success field is `prompt`, not `optimizedPrompt` | field contract | PASS | `literary_job::literary_success_field_is_prompt_not_optimized_prompt` @ 9a03f482b4 `--lib` |
| L11 | Image truncated complete JSON is usable; incomplete JSON keeps raw | `optimized_prompt` / `IMAGE_PROMPT_OUTPUT_INVALID` + `raw_reply` | PASS | `run::run_image_truncated_complete_json_is_usable`, `run_image_truncated_incomplete_json_keeps_raw` @ 9a03f482b4 `--lib` |
| L12 | Empty user text is required | `IMAGE_PROMPT_USER_INPUT_REQUIRED` | PASS | `image_job::empty_user_text_is_required` @ 9a03f482b4 `--lib` |
| L13 | Method resources over budget are refused without truncation | `PROJECTION_RESOURCE_BUDGET_EXCEEDED` | PASS | `image_job::method_resources_are_refused_without_truncation_when_over_budget` @ 9a03f482b4 `--lib` (fixture typed at `3f120444c5`) |
| L14 | Digest form / body mismatch; resource path escape | digest and path errors | PASS | `skill_material::digest_form_and_body_mismatch`, `resource_paths_cannot_escape_the_package` @ 9a03f482b4 `--lib` |
| L15 | `compress_video_prompt` skips the model under target; does not load Skill or enter optimize | no completer / no skill | PASS | `video_run::compress_video_prompt_skips_the_model_when_already_under_target`, `compress_video_prompt_does_not_load_a_skill_or_enter_optimize` @ 9a03f482b4 `--lib` |
| L16 | Production roster is the eight real projectors | catalog unit | PASS | `projectors::catalog::production_roster_is_the_eight_real_projectors` @ 9a03f482b4 `--lib` |
| L17 | Official eight compile / mode / quote / lock contracts | per-projector unit names in `src/projectors/{generic,wan,seedance,kling,ltx,h3}` | PASS | those modules' `#[test]` functions @ 9a03f482b4 `--lib` |

`bgx-text-http` already has in-src unit coverage (adapter package/media, MiniMax thinking, Qwen native image key / video reject / fake HTTP URLs, OpenAI outbound / provider error / timeout 30s / URL resolution). Those tests were not part of the 111 prompt-optimize lib run, and this tree did not execute them. They are **not** marked PASS here. New tests were not copied over them; only the crate-role integration file was added (N13).

## 4. Plan §6 A1–A12 (offline / real / user-path)

| ID | Scene | Offline contract | Real text model | User path |
| --- | --- | --- | --- | --- |
| A1 | Clean Windows shell, no `story.db` | NOT_RUN — shell and install are C/T32. Core fixtures N01–N13 and G00–G08 do not replace a shell command. | NOT_RUN — no credential or call authorization in this task | NOT_RUN |
| A2 | Host workbench image E01–E05 | Offline IPC/host is D. Core image contracts: L06–L13 PASS (`--lib` @ 9a03f482b4); N07–N10 NOT_RUN | NOT_RUN | NOT_RUN |
| A3 | Free canvas image/video and batch | Host/vitest is D/C. Not claimed. | NOT_RUN | NOT_RUN |
| A4 | Storyboard single and batch | Host CAS is D. Literary success field L10 PASS (`--lib` @ 9a03f482b4) | NOT_RUN | NOT_RUN |
| A5 | Cast portrait, scene concept board | Host is D. Not claimed. | NOT_RUN | NOT_RUN |
| A6 | Eight projectors, host and shell text identical | G00–G08 exist as exact goldens, status NOT_RUN until `cargo test -p bgx-prompt-optimize --test golden_video`. Supporting compile units L16–L17 PASS (`--lib` @ 9a03f482b4) | NOT_RUN | NOT_RUN |
| A7 | `compress_only` | L15 PASS (`--lib` @ 9a03f482b4) | NOT_RUN | NOT_RUN — “does not load Skill” is covered offline in L15; UI path is D |
| A8 | Same Skill digest both ends | NOT_RUN — T26/T29 shell and installer, not this tree | n/a | NOT_RUN |
| A9 | Attachments 0, 1, 16, 17 | 0/1/17 and optional first-N: L06–L09 PASS (`--lib` @ 9a03f482b4) plus N07–N10 NOT_RUN. Exact 16-as-submitted through `run_image` is implied by N09 (`MAX_IPC_RELS` accepted in `reject_if_over_submitted_cap`) and L09; no separate 16-image public `run_image` test was added. | NOT_RUN | NOT_RUN |
| A10 | Missing key, cancel, timeout | Core cancel: L03–L05 PASS (`--lib` @ 9a03f482b4); N04 NOT_RUN. Timeout is a port error (`OPTIMIZE_TIMEOUT`); core does not sleep. Shell missing-key and delayed fake server are C/D. | NOT_RUN — do not use a paid endpoint for timeout | NOT_RUN |
| A11 | Two-node concurrency | NOT_RUN — batch serialisation stays on the host; core serves one completion per call | NOT_RUN | NOT_RUN |
| A12 | Resource paths have no absolute drive letter | N12/N13 crate-role NOT_RUN. Install resource list is T29/T32, D/C. Path-escape unit L14 PASS (`--lib` @ 9a03f482b4) | n/a | NOT_RUN |

## 5. Production gaps found (report only — not fixed in this tree)

These are not marked PASS. Lane A did not change production `src` / `lib.rs` / `Cargo.toml`, B’s new H3 modules, or D’s tree.

1. **Official `video-expression` input names vs core payload.** `run_video_expression` only inserts input `payload`. Official `internal-prompt-skills/video-expression/request.json` asks for `taskContext` and `outputContract`. `render_request` has no alias; the first-task fallback retries the same inputs. If a host packs the official package unchanged, render fails with `missing-input`. Literary already supplies the official names. N01 uses a payload-only fixture so the schema contract can be reached. Fix ownership: D (public run / host packing) or a later A body change after D agrees on the input map.
2. **`run_video_optimize` defers projector identity checks until `run_video_expression`.** Source/content stages can already call the model for an unregistered profile (for example HappyHorse). N05 proves the expression entry itself refuses before complete. A “reject missing projector before any stage complete” test was not written as a passing contract, because current production would not satisfy it. Ownership: D, or A `video_run.rs` body if D assigns it.
3. **Non-expression stages still send `TextCall.schema = None`** (`complete_stage` in `video_run.rs`). T19 required closed-set schema on expression; that is what N01/N11 assert. If official contracts also have schemas for source/content/review, they are not injected. Ownership: D to decide; do not treat source existence as coverage.
4. **H3 原稿规范化 is still not migrated** into this core. That remains lane B. This tree did not touch B’s new H3 modules.
5. **`draft_schema_for` still lives in this crate**, not in `bgx-video-prompt-contract`. Previously reported; D owns the public registry / file move.
6. **`verify_schema_keywords` is still not ported** into `expression_schema`; official shipped JSON remains SSOT. Previously reported.
7. **Host / main-window compile is still blocked** by SDK / canvas dependency versions. Not this lane.
8. **T32 clean-install and real-model columns** remain `NOT_RUN`. Offline fixtures do not stand in for them.

## 6. What this tree explicitly did not do

- Did not edit production `src/`, `lib.rs`, or `Cargo.toml` of either crate.
- Did not edit existing test modules, including `tests/golden_video.rs`.
- Did not edit `ACCEPTANCE.md` or the existing `REGRESSION-MATRIX.md`.
- Did not write in the D worktree, did not push, did not merge `main`.
- Did not re-run the full compile or the 111 lib tests on this tree.
