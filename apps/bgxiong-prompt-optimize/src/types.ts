export type Capability = "image" | "literary" | "video" | "compress";

export type OptimizeTarget = {
  targetId: string;
  label: string;
  kind: "image" | "video" | "literary";
  availability: "ready" | "installable" | "unavailable";
  methodId: string | null;
  methodDigest: string | null;
};

export type SkillCandidate = {
  candidateId: string;
  packageId: string;
  displayName: string;
  version: string;
  needsConfirm: boolean;
};

export type ShellConfigPublic = {
  providerKind: string;
  baseUrl: string | null;
  model: string | null;
  savedModels: string[];
  hasApiKey: boolean;
  timeoutSecs: number | null;
  maxOutputTokens: number | null;
  skillLibraryRoot: string | null;
  internalTemplateRoot: string | null;
  adapterPackageRoot: string | null;
  localModelPath: string | null;
  language: string | null;
  supportsVision: boolean;
};

export type ShellConfigInput = {
  providerKind: string;
  baseUrl: string;
  model: string;
  apiKey: string;
  timeoutSecs: string;
  /** Official catalog caps_json.maxOutputTokens; required for MiniMax when no catalog cache is available. */
  maxOutputTokens: string;
  skillLibraryRoot: string;
  internalTemplateRoot: string;
  adapterPackageRoot: string;
  localModelPath: string;
  language: "zh" | "en";
  supportsVision: boolean;
};

export type TextChannel = {
  providerKind: string;
  label: string;
  needsBaseUrl: boolean;
  group: "cloud" | "local" | "adapter" | string;
};

export type SkillListItem = {
  stableId: string;
  name: string;
  packageDigest: string;
  hosts: string[];
  displayName: string | null;
};

export type VideoInputModeDto = {
  id: string;
  requiredMedia: string[];
};

export type VideoProfileDto = {
  profileId: string;
  profileVersion: number;
  displayKey: string;
  defaultMethodName: string | null;
  inputModes: VideoInputModeDto[];
};

export type VideoTargetForm = {
  profileId: string;
  profileVersion: number;
  inputMode: string;
  durationSeconds: number | null;
  expressionLanguage: "zh" | "en";
  referenceUseIds: string[];
};

export type ReferenceItem = {
  path: string;
  mediaKind: string;
  usage: string;
};

export type OptimizeOutput = {
  kind: string;
  text: string;
  rawReply: string | null;
  jobId: string;
  profileId: string | null;
  profileVersion: number | null;
};

export const IMAGE_HOST = "optimize-image-prompt";
export const LITERARY_HOST = "optimize-storyboard-shot";
export const VIDEO_FORMAT_HOST = "video-prompt-projection";
export const VIDEO_CONTENT_HOST = "optimize-segment";

export const REFERENCE_USAGES: { id: string; label: string; mediaKind: string }[] = [
  { id: "first_frame", label: "首帧", mediaKind: "first_frame" },
  { id: "last_frame", label: "尾帧", mediaKind: "last_frame" },
  { id: "reference_image", label: "参考图", mediaKind: "reference_image" },
  { id: "voice_timbre", label: "音色", mediaKind: "reference_audio" },
  { id: "audio_copy", label: "音轨复制", mediaKind: "reference_audio" },
  { id: "reference_video", label: "参考视频", mediaKind: "reference_video" },
];
