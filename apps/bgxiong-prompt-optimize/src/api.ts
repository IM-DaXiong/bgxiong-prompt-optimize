import { invoke } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import type {
  OptimizeTarget,
  SkillCandidate,
  OptimizeOutput,
  ShellConfigInput,
  ShellConfigPublic,
  SkillListItem,
  TextChannel,
  VideoProfileDto,
  VideoTargetForm,
  ReferenceItem,
} from "./types";

export function listOptimizeTargets(): Promise<OptimizeTarget[]> {
  return invoke("list_optimize_targets");
}

export function checkPromptSkills(): Promise<SkillCandidate[]> {
  return invoke("check_prompt_skills");
}

export function installPromptSkill(candidateId: string, confirmedNewInstall: boolean): Promise<void> {
  return invoke("install_prompt_skill", { candidateId, confirmedNewInstall });
}

export function optimizePrompt(input: {
  targetId: string;
  sourcePrompt: string;
  expectedMethodDigest?: string;
}): Promise<{ optimizedPrompt: string }> {
  return invoke("optimize_prompt", { input });
}

export async function loadConfig(): Promise<ShellConfigPublic> {
  return invoke("load_config");
}

export async function saveConfig(input: ShellConfigInput): Promise<ShellConfigPublic> {
  const timeout = input.timeoutSecs.trim();
  const timeoutSecs = timeout === "" ? null : Number(timeout);
  if (timeout !== "" && (!Number.isFinite(timeoutSecs) || (timeoutSecs ?? 0) < 0)) {
    throw new Error("超时秒数请留空或填写非负数。");
  }
  const capRaw = input.maxOutputTokens.trim();
  const maxOutputTokens = capRaw === "" ? null : Number(capRaw);
  if (
    capRaw !== "" &&
    (!Number.isFinite(maxOutputTokens) || !Number.isInteger(maxOutputTokens) || (maxOutputTokens ?? 0) <= 0)
  ) {
    throw new Error("最大输出请留空或填写正整数。");
  }
  return invoke("save_config", {
    config: {
      providerKind: input.providerKind,
      baseUrl: emptyToNull(input.baseUrl),
      model: emptyToNull(input.model),
      apiKey: emptyToNull(input.apiKey),
      timeoutSecs: timeout === "" ? null : timeoutSecs,
      maxOutputTokens,
      skillLibraryRoot: emptyToNull(input.skillLibraryRoot),
      internalTemplateRoot: emptyToNull(input.internalTemplateRoot),
      adapterPackageRoot: emptyToNull(input.adapterPackageRoot),
      localModelPath: emptyToNull(input.localModelPath),
      language: input.language,
      supportsVision: input.supportsVision,
    },
  });
}

export async function listTextChannels(): Promise<TextChannel[]> {
  return invoke("list_text_channels");
}

export async function listSkills(host: string): Promise<SkillListItem[]> {
  return invoke("list_skills", { host });
}

export async function listVideoProfiles(): Promise<VideoProfileDto[]> {
  return invoke("list_video_profiles");
}

export async function pickImages(): Promise<string[]> {
  return invoke("pick_images");
}

export async function pickReferences(): Promise<string[]> {
  return invoke("pick_references");
}

export async function optimizeImage(input: {
  userText: string;
  methodId: string | null;
  language: "zh" | "en";
  imagePaths: string[];
}): Promise<OptimizeOutput> {
  return invoke("optimize_image", { input });
}

export async function optimizeVideo(input: {
  sourceText: string;
  changeRequest: string | null;
  target: VideoTargetForm;
  formatMethodId: string | null;
  contentMethodId: string | null;
  references: ReferenceItem[];
}): Promise<OptimizeOutput> {
  return invoke("optimize_video", { input });
}

export async function optimizeLiterary(input: {
  sourceText: string;
  creativeMethodId: string | null;
  language: "zh" | "en";
}): Promise<OptimizeOutput> {
  return invoke("optimize_literary", { input });
}

export async function compressVideoPrompt(input: {
  sourceText: string;
  targetMaxChars: number;
}): Promise<OptimizeOutput> {
  return invoke("compress_video_prompt", { input });
}

export async function cancelOptimize(jobId: string): Promise<void> {
  return invoke("cancel_optimize", { jobId });
}

export async function saveResult(text: string): Promise<void> {
  return invoke("save_result", { text });
}

export async function copyText(text: string): Promise<void> {
  await writeText(text);
}

function emptyToNull(value: string): string | null {
  const trimmed = value.trim();
  return trimmed === "" ? null : trimmed;
}

export function errorMessage(error: unknown): string {
  if (typeof error === "string" && error.trim() !== "") {
    return error;
  }
  if (error instanceof Error && error.message.trim() !== "") {
    return error.message;
  }
  return "操作失败。";
}
