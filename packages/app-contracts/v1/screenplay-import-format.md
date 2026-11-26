# External screenplay import format

This is a serialization contract, not a story-writing workflow. The external agent chooses the story, methods, Skills, assets, prompts and order of operations.

Use `files.screenplay.contract.get` with `kind=schema` for current fields, types, enums and required structural keys; `kind=example` returns one illustrative package, not required creative content. `kind=limits` returns actual transport/parse budgets. Internal authoring-stage budgets are not import limits.

## Container

Send UTF-8 text using the supported Fountain subset with BGX JSON blocks. A block is delimited by an exact opening marker such as `===BGX-META===` and a matching closing marker `===/BGX-META===`. Its body is a JSON object, not Markdown or prose.

- Start with exactly one BGX-META block. Its schemaVersion must match the current schema.
- Optional BGX-ROLE and BGX-PROP blocks go after META and before the first episode heading.
- Start an episode with a `# ` heading. Each Fountain scene heading is followed by its BGX-SCENE block, then that scene's BGX-SHOT blocks and supported scene text.
- ROLE and PROP blocks are declarations, not photo uploads. Existing media can be imported or connected separately with explicit identities.

## Partial content and references

Photos, props, Skills, style, image prompts, video prompts and duration are not universal prerequisites. Omit optional fields when information is unavailable. Declaring a role requires its identity but does not require a portrait image or portrait prompt. A shot may be imported before its final prompt or timing is known. No story chapter, scene or shot count is imposed by the import validator; real transport budgets still apply.

When supplied, references must resolve to declared objects. Role names and prop IDs are package references, not host database IDs. Scene and shot reference sets and dialogue speakers are validated against their declared scope; malformed or dangling references are returned as findings. Empty strings are not substitutes for enum values. The schema is authoritative for field-level rules.

The host does not infer missing asset associations, expand prompts, rewrite prose or call an LLM during import. Optional clip-graph nodes and references express only the caller's explicit choices. Creating an asset, binding it to a role and connecting a graph edge are separate operations.

## Preview and apply

`files.screenplay.preview` checks the supplied source and returns findings/preview information without importing it. Read the current schema of the existing file change prepare/apply tools for explicit targets and import options. A prepared changeSetId is not an applied import. Use the actual apply receipt and returned IDs to locate the imported content.

Name duplicates must be resolved from returned candidates and stable IDs. Save the import request/idempotency key and receipt in the agent's own operation notes. After a missing response, query the original request/change set before issuing another import. Generating media is a separate explicit operation using the selected model's current capabilities.
