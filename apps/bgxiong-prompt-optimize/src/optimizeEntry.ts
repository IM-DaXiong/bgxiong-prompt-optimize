/**
 * Current standalone entry: basic text model + one optimize Skill.
 * Image and literary optimize hosts are listed together. Video target /
 * compress UIs stay out of this surface; their commands remain in api.ts
 * for the reconstructed backend, not as a user-facing mode.
 *
 * Future 扩写 is reserved here only. Do not add an 扩写 mode, page, or
 * parameter to the current shell UI.
 */

import { IMAGE_HOST, LITERARY_HOST, type SkillListItem } from "./types";

export const OPTIMIZE_SKILL_HOSTS = [IMAGE_HOST, LITERARY_HOST] as const;

export type OptimizeSkillRoute = {
  host: (typeof OPTIMIZE_SKILL_HOSTS)[number];
  skill: SkillListItem;
};

/** Reserved for a future 扩写 policy. Intentionally empty for this entry. */
export type PromptExpansionHook = {
  readonly unimplemented: true;
};

export const PROMPT_EXPANSION_HOOK: PromptExpansionHook = { unimplemented: true };

export function skillOptionValue(route: OptimizeSkillRoute): string {
  return `${route.host}::${route.skill.stableId}`;
}

export function parseSkillOptionValue(
  value: string,
  routes: OptimizeSkillRoute[],
): OptimizeSkillRoute | null {
  return routes.find((item) => skillOptionValue(item) === value) ?? null;
}

export function mergeOptimizeSkillRoutes(
  image: SkillListItem[],
  literary: SkillListItem[],
): OptimizeSkillRoute[] {
  const routes: OptimizeSkillRoute[] = [];
  const seen = new Set<string>();
  for (const skill of image) {
    const route: OptimizeSkillRoute = { host: IMAGE_HOST, skill };
    const key = skillOptionValue(route);
    if (seen.has(key)) continue;
    seen.add(key);
    routes.push(route);
  }
  for (const skill of literary) {
    const route: OptimizeSkillRoute = { host: LITERARY_HOST, skill };
    const key = skillOptionValue(route);
    if (seen.has(key)) continue;
    seen.add(key);
    routes.push(route);
  }
  return routes;
}

export function skillLabel(item: SkillListItem): string {
  const name = item.displayName?.trim() || item.name.trim() || item.stableId;
  return name;
}

/**
 * Copy must use the text currently in the result editor, never a stale
 * optimize-response snapshot kept on the side.
 */
export function resultClipboardPayload(editedResult: string): string {
  return editedResult;
}
