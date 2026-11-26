// @generated from JSON Schema. Do not edit.

export type VideoPromptPipelineMode = "optimize" | "compress_only" | "generate" | "project_existing" | "adapt_expand";

export interface VideoPromptProjectionMethodSelectionTargetDefault {
  "kind": "target_default";
}

export type VideoPromptPackageDigest = string;

export type VideoPromptStableId = string;

export interface VideoPromptProjectionMethodSelectionCustom {
  "kind": "custom";
  "packageDigest": VideoPromptPackageDigest;
  "stableId": VideoPromptStableId;
}

export type VideoPromptProjectionMethodSelection = VideoPromptProjectionMethodSelectionTargetDefault | VideoPromptProjectionMethodSelectionCustom;

export type VideoPromptSha256Hex = string;

export type VideoPromptCanonicalDocumentScope = "segment_node";

export interface VideoPromptCanonicalDocumentRef {
  "canonicalFingerprint": VideoPromptSha256Hex;
  "contentFingerprint": string;
  "scope": VideoPromptCanonicalDocumentScope;
  "segmentNodeId": string;
}

export interface VideoPromptSpanSourceRef {
  "sceneId": string;
  "snapshotRef": string;
  "spanId": string;
  "spanRevision": number;
  "storyId": string;
}

export type VideoPromptActivationId = string;

export type VideoPromptSkillName = string;

export type VideoPromptResourceRef = string;

export interface VideoPromptFrozenResource {
  "resourceRef": VideoPromptResourceRef;
  "sha256": VideoPromptSha256Hex;
}

export type VideoPromptSelectionKind = "target_default" | "custom";

export interface VideoPromptFrozenMethod {
  "activationId": VideoPromptActivationId;
  "bindingProfileId": string;
  "bindingProfileVersion": number;
  "bodySha256": VideoPromptSha256Hex;
  "name": VideoPromptSkillName;
  "packageDigest": VideoPromptPackageDigest;
  "resources": Array<VideoPromptFrozenResource>;
  "selectionKind": VideoPromptSelectionKind;
  "stableId": VideoPromptStableId;
}

export type VideoPromptExpressionLanguage = "zh" | "en";

export interface VideoPromptTargetSnapshot {
  "capsDigest": VideoPromptSha256Hex;
  "durationSeconds": number | null;
  "expressionLanguage": VideoPromptExpressionLanguage;
  "inputMode": string;
  "profileId": string;
  "profileVersion": number;
  "referenceUseIds": Array<string>;
}

export type VideoPromptProjectionReviewStatus = "not_run" | "completed" | "incomplete";

export interface VideoPromptProjectionReview {
  "blockingCount": number;
  "canonicalFingerprint"?: string;
  "invoked": boolean;
  "minorCount": number;
  "repairRounds": number;
  "status"?: VideoPromptProjectionReviewStatus;
}

export type VideoPromptExpansionDeclaration = "allow" | "preserve";

export type VideoPromptExpansionMode = VideoPromptExpansionDeclaration;

export type VideoPromptExpansionRole = "content" | "format" | "user";

export interface VideoPromptExpansionSource {
  "declaration": VideoPromptExpansionMode;
  "packageDigest": string;
  "resourceRefs": Array<string>;
  "role": VideoPromptExpansionRole;
  "ruleText": string;
  "sourceId": string;
  "stableId": string;
}

export interface VideoPromptExpansionPolicy {
  "mode": VideoPromptExpansionMode;
  "sources": Array<VideoPromptExpansionSource>;
  "userInstructions": string;
}

export type VideoPromptMotherDocumentOrigin = "authored" | "reused_cache" | "existing_canonical" | "adapted_expansion";

export interface VideoPromptNegativeResultUnsupported {
  "kind": "unsupported";
}

export interface VideoPromptNegativeResultClear {
  "kind": "clear";
}

export interface VideoPromptNegativeResultText {
  "kind": "text";
  "sourceRefs": Array<string>;
  "text": string;
}

export type VideoPromptNegativeResult = VideoPromptNegativeResultUnsupported | VideoPromptNegativeResultClear | VideoPromptNegativeResultText;

export interface VideoPromptNegativeSourceSpan {
  "channel": "negative";
  "endByte": number;
  "sourceRefs": Array<string>;
  "startByte": number;
}

export interface VideoPromptPromptChannels {
  "negative": VideoPromptNegativeResult;
  "negativeSourceMap"?: Array<VideoPromptNegativeSourceSpan>;
  "positiveSourceRefs": Array<string>;
}

export interface VideoPromptProjectionReceipt {
  "actualExpanded"?: boolean;
  "canonicalDocumentRef": VideoPromptCanonicalDocumentRef | null;
  "expansionPolicy"?: VideoPromptExpansionPolicy;
  "expressionModelInvoked": boolean;
  "frozenMethod": VideoPromptFrozenMethod;
  "mode": VideoPromptPipelineMode;
  "motherDocumentOrigin": VideoPromptMotherDocumentOrigin;
  "motherReview"?: VideoPromptProjectionReview;
  "projectionFingerprint": VideoPromptSha256Hex;
  "promptChannels"?: VideoPromptPromptChannels;
  "review": VideoPromptProjectionReview;
  "reviewInvokedThisRun"?: boolean;
  "targetSnapshot": VideoPromptTargetSnapshot;
}

export interface VideoPromptReferenceMediaInventoryItemDeclaredIdentity {
  "entityId": string;
  "entityKind": string;
}

export type VideoPromptReferenceMediaInventoryItemTransportRole = "first_frame" | "last_frame" | "reference_image";

export interface VideoPromptReferenceMediaInventoryItem {
  "audioUsage"?: string;
  "bindingId"?: string;
  "declaredEntityKind"?: string;
  "declaredIdentity"?: VideoPromptReferenceMediaInventoryItemDeclaredIdentity;
  "itemId"?: string;
  "label": string;
  "mediaKind": string;
  "mediaRelativePath"?: string;
  "ordinalWithinKind"?: number;
  "sourceKind"?: string;
  "token": string;
  "transportRole"?: VideoPromptReferenceMediaInventoryItemTransportRole;
}

export interface VideoPromptProjectionQueryInput {
  "candidateCursor"?: string;
  "durationSeconds"?: number;
  "intentInputMode": string;
  "projectionMethodSelection": VideoPromptProjectionMethodSelection;
  "referenceMediaInventory": Array<VideoPromptReferenceMediaInventoryItem>;
  "videoModelId"?: string;
  "videoProviderKind": string;
}

export interface VideoPromptProjectionInventoryLabel {
  "itemId": string;
  "token": string | null;
}

export type VideoPromptProjectionQueryIssueCode = "VIDEO_PROMPT_TARGET_MISSING" | "PROMPT_PROJECTION_INVALID" | "PROMPT_PROJECTION_UNKNOWN_PROFILE" | "PROMPT_PROJECTION_NOT_APPLICABLE" | "VIDEO_PROMPT_INPUT_MODE_UNSUPPORTED" | "VIDEO_PROMPT_CAPS_DIGEST_MISSING" | "VIDEO_PROMPT_REFERENCE_ID_MISSING" | "VIDEO_PROMPT_PROJECTOR_MISSING" | "VIDEO_PROMPT_TARGET_SNAPSHOT_INVALID" | "PROJECTION_TARGET_PROJECTOR_NOT_REGISTERED" | "PROJECTION_DEFAULT_METHOD_NOT_INSTALLED" | "PROJECTION_CUSTOM_METHOD_NOT_INSTALLED" | "PROJECTION_CUSTOM_DIGEST_MISMATCH" | "PROJECTION_METHOD_RECORD_UNREADABLE" | "PROJECTION_METHOD_LIBRARY_UNAVAILABLE" | "PROJECTION_METHOD_LIBRARY_CHANGED" | "PROJECTION_METHOD_NOT_BOUND" | "PROJECTION_BINDING_OUT_OF_RANGE" | "PROJECTION_BINDING_DUPLICATE_PROFILE_VERSION" | "PROJECTION_BINDING_DUPLICATE_ITEM" | "PROJECTION_BINDING_UNKNOWN_PROFILE" | "PROJECTION_BINDING_INPUT_MODE_UNSUPPORTED" | "PROJECTION_BINDING_INVALID_RESOURCE_REF" | "PROJECTION_BINDING_RESOURCE_MISSING" | "PROJECTION_BINDING_MANIFEST_MISMATCH" | "PROJECTION_CANDIDATE_CURSOR_INVALID" | "PROJECTION_EXPANSION_DECLARATION_INVALID" | "PROJECTION_METHOD_DISABLED" | "PROJECTION_METHOD_HOST_NOT_ENABLED";

export interface VideoPromptProjectionQueryIssue {
  "code": VideoPromptProjectionQueryIssueCode;
  "detail": string;
}

export type VideoPromptProjectionBindingState = "bound" | "needsBinding" | "hostScopeMissing" | "skillDisabled" | "resourceMissing" | "profileVersionMismatch" | "inputModeUnsupported";

export type VideoPromptExpansionDeclarationStatus = "declared" | "undeclared" | "invalid";

export interface VideoPromptExpansionDeclarationView {
  "rawValue": string | null;
  "status": VideoPromptExpansionDeclarationStatus;
  "value": VideoPromptExpansionDeclaration | null;
}

export interface VideoPromptProjectionMethodCandidateIssue {
  "code": string;
  "detail": string;
}

export interface VideoPromptProjectionMethodCandidate {
  "compatibility": VideoPromptProjectionBindingState;
  "compatible": boolean;
  "displayName": string;
  "expansionDeclaration": VideoPromptExpansionDeclarationView;
  "issue": VideoPromptProjectionMethodCandidateIssue | null;
  "missingResourceRefs": Array<string>;
  "name": string;
  "packageDigest": VideoPromptPackageDigest;
  "resourceRefs": Array<string>;
  "stableId": VideoPromptStableId;
}

export type VideoPromptExistingProjectionMode = "project_existing" | "adapt_expand";

export interface VideoPromptProjectionQueryMethod {
  "compatibility": VideoPromptProjectionBindingState;
  "displayName": string;
  "expansionDeclaration": VideoPromptExpansionDeclarationView;
  "missingResourceRefs": Array<string>;
  "name": string;
  "packageDigest": VideoPromptPackageDigest;
  "resourceRefs": Array<string>;
  "stableId": VideoPromptStableId;
}

export interface VideoPromptProjectionQueryResult {
  "capsRevision": string | null;
  "defaultMethodName": string | null;
  "inventoryLabels": Array<VideoPromptProjectionInventoryLabel>;
  "issue": VideoPromptProjectionQueryIssue | null;
  "methodCandidates": Array<VideoPromptProjectionMethodCandidate>;
  "nextCandidateCursor": string | null;
  "recommendedExistingMode": VideoPromptExistingProjectionMode | null;
  "selectableMethods": Array<VideoPromptProjectionQueryMethod>;
  "selectedMethod": VideoPromptProjectionQueryMethod | null;
  "syntaxProfileId": string | null;
  "targetSnapshot": VideoPromptTargetSnapshot | null;
}

export interface VideoPromptVideoPromptChannelsSchemaJsonNegativeInputUnrecorded {
  "kind": "unrecorded";
}

export interface VideoPromptVideoPromptChannelsSchemaJsonNegativeInputInherit {
  "kind": "inherit";
}

export interface VideoPromptVideoPromptChannelsSchemaJsonNegativeInputClear {
  "kind": "clear";
}

export interface VideoPromptVideoPromptChannelsSchemaJsonNegativeInputText {
  "kind": "text";
  "text": string;
}

export type VideoPromptVideoPromptChannelsSchemaJsonNegativeInput = VideoPromptVideoPromptChannelsSchemaJsonNegativeInputUnrecorded | VideoPromptVideoPromptChannelsSchemaJsonNegativeInputInherit | VideoPromptVideoPromptChannelsSchemaJsonNegativeInputClear | VideoPromptVideoPromptChannelsSchemaJsonNegativeInputText;

export type VideoPromptNegativeInput = VideoPromptVideoPromptChannelsSchemaJsonNegativeInput;

export type VideoPromptVideoPromptChannelsSchemaJsonNegativeOperation = "preserve" | "generate";

export type VideoPromptNegativeOperation = VideoPromptVideoPromptChannelsSchemaJsonNegativeOperation;

export type VideoPromptVideoPromptProjectionSchemaJsonExpansionMode = VideoPromptExpansionMode;

export type VideoPromptConflictCode = "capability_unsupported" | "mode_mismatch" | "reference_use_unsupported" | "mother_insufficient";

export type VideoPromptLocalId = string;

export interface VideoPromptProjectionConflict {
  "code": VideoPromptConflictCode;
  "factIds": Array<VideoPromptLocalId>;
  "referenceUseIds": Array<VideoPromptLocalId>;
}

export interface VideoPromptExpressionLine {
  "factIds": Array<VideoPromptLocalId>;
  "quoteIds"?: Array<VideoPromptLocalId>;
  "referenceUseIds"?: Array<VideoPromptLocalId>;
  "shotId": VideoPromptLocalId;
  "text": string;
}

export type VideoPromptProposedFactPolarity = "positive" | "negative";

export interface VideoPromptProposedFact {
  "entityRefIds": Array<string>;
  "factId": string;
  "kind": string;
  "polarity": VideoPromptProposedFactPolarity;
  "semanticSlot": string;
  "shotId": string;
  "sourceRef": string;
  "text": string;
}

export interface VideoPromptSpeakerVoiceBinding {
  "speakerId": VideoPromptLocalId;
  "subjectId"?: VideoPromptLocalId;
  "voiceReferenceUseId": VideoPromptLocalId;
}

export interface VideoPromptVideoPromptProjectionDraftSchemaJsonProjectionDraft {
  "conflicts": Array<VideoPromptProjectionConflict>;
  "lines": Array<VideoPromptExpressionLine>;
  "negative"?: VideoPromptNegativeResult;
  "proposedFacts"?: Array<VideoPromptProposedFact>;
  "speakerVoiceBindings"?: Array<VideoPromptSpeakerVoiceBinding>;
}

export type VideoPromptProjectionDraft = VideoPromptVideoPromptProjectionDraftSchemaJsonProjectionDraft;

export type VideoPromptReviewFindingChannel = "positive" | "negative";

export type VideoPromptReviewIssue = "fact_omitted" | "fact_added" | "fact_altered" | "quote_altered" | "reference_misused" | "order_changed" | "language_mismatch" | "method_rule_violated" | "target_syntax_violated" | "expansion_out_of_scope";

export type VideoPromptReviewSeverity = "blocking" | "minor";

export interface VideoPromptReviewFinding {
  "channel"?: VideoPromptReviewFindingChannel;
  "detail": string;
  "factIds": Array<VideoPromptLocalId>;
  "issue": VideoPromptReviewIssue;
  "lineIndex": number | null;
  "quoteIds": Array<VideoPromptLocalId>;
  "referenceUseIds": Array<VideoPromptLocalId>;
  "severity": VideoPromptReviewSeverity;
  "shotId": VideoPromptLocalId | null;
  "sourceRefs"?: Array<string>;
}

export interface VideoPromptVideoPromptProjectionDraftSchemaJsonProjectionReviewResult {
  "findings": Array<VideoPromptReviewFinding>;
}

export type VideoPromptProjectionReviewResult = VideoPromptVideoPromptProjectionDraftSchemaJsonProjectionReviewResult;

