import { describe, expect, it } from "vitest";
import type { SkillInstallation, SkillLockEntry, SkillRecord } from "../types";
import { isRegistrySource, isRegistryTracked, skillsShUpdateSource } from "./skillUtils";

const REGISTRY_URL = "https://github.com/Pgooone/oh-my-skills-skills.git";

function skill(overrides: Partial<SkillRecord>): SkillRecord {
  return {
    id: "test-skill",
    slug: "test-skill",
    displayName: "Test Skill",
    canonicalStatus: "imported",
    installations: [],
    missingAgents: [],
    issues: [],
    conflict: false,
    ...overrides
  };
}

function lock(overrides: Partial<SkillLockEntry> = {}): SkillLockEntry {
  return {
    source: REGISTRY_URL,
    sourceType: "github",
    sourceUrl: REGISTRY_URL,
    skillPath: "skills/test-skill",
    installedAt: "2026-08-01T00:00:00Z",
    ...overrides
  };
}

function installation(overrides: Partial<SkillInstallation> = {}): SkillInstallation {
  return {
    id: "inst-1",
    agentId: "claude-code",
    agentLabel: "Claude Code",
    scope: "global",
    rootPath: "/Users/me/.agents",
    entryPath: "/Users/me/.agents/skills/test-skill",
    isSymlink: false,
    brokenSymlink: false,
    status: "ok",
    issues: [],
    ...overrides
  };
}

describe("skillsShUpdateSource", () => {
  it("lock 命中且 canonicalStatus==imported 时以 canonicalPath 作 entryPath（W4 兜底）", () => {
    const canonicalPath = "/Users/me/.oh-my-skills/skills/test-skill";
    const source = skillsShUpdateSource(
      skill({ canonicalPath, installations: [] }),
      { "test-skill": lock() }
    );

    expect(source).not.toBeNull();
    expect(source!.installation.entryPath).toBe(canonicalPath);
    expect(source!.sourceUrl).toBe(REGISTRY_URL);
  });

  it("存在非中心库引用的 .agents/skills 实目录时沿用旧候选", () => {
    const canonicalPath = "/Users/me/.oh-my-skills/skills/test-skill";
    const source = skillsShUpdateSource(
      skill({
        canonicalPath,
        installations: [
          installation({
            rootPath: "/Users/me/.agents",
            entryPath: "/Users/me/.agents/skills/test-skill"
          })
        ]
      }),
      { "test-skill": lock() }
    );

    expect(source).not.toBeNull();
    expect(source!.installation.entryPath).toBe("/Users/me/.agents/skills/test-skill");
  });

  it("无 lock 条目时返回 null", () => {
    const source = skillsShUpdateSource(
      skill({ canonicalPath: "/Users/me/.oh-my-skills/skills/test-skill" }),
      {}
    );
    expect(source).toBeNull();
  });
});

describe("isRegistrySource 更新分流判定", () => {
  it("registry 来源 → 判定为注册表（走 check_registry_skill_updates / update_registry_skill）", () => {
    expect(isRegistrySource(REGISTRY_URL, REGISTRY_URL)).toBe(true);
  });

  it("skills.sh 来源 → 判定为既有（走 check_skills_sh_update / update_skills_sh_skill）", () => {
    expect(isRegistrySource("https://github.com/nextcaicai/caicai-skills.git", REGISTRY_URL)).toBe(false);
  });

  it("双侧归一化差异不改变判定（ssh 形态 / 尾斜杠 / 裸 slug）", () => {
    expect(isRegistrySource("git@github.com:Pgooone/oh-my-skills-skills", `${REGISTRY_URL}/`)).toBe(true);
    expect(isRegistrySource("Pgooone/oh-my-skills-skills", REGISTRY_URL)).toBe(true);
  });

  it("settings 未配置注册表 URL 时全部走既有 command", () => {
    expect(isRegistrySource(REGISTRY_URL, undefined)).toBe(false);
  });
});

describe("isRegistryTracked", () => {
  it("lock 来源归一化等于注册表 URL 时判定为注册表跟踪", () => {
    expect(
      isRegistryTracked(skill({ canonicalStatus: "imported" }), { "test-skill": lock() }, REGISTRY_URL)
    ).toBe(true);
  });

  it("lock 来源为其他仓库时不判定", () => {
    expect(
      isRegistryTracked(
        skill({ canonicalStatus: "imported" }),
        { "test-skill": lock({ sourceUrl: "https://github.com/nextcaicai/caicai-skills.git" }) },
        REGISTRY_URL
      )
    ).toBe(false);
  });

  it("无 lock 条目时不判定", () => {
    expect(isRegistryTracked(skill({}), {}, REGISTRY_URL)).toBe(false);
  });
});
