import { Check, ChevronDown, ChevronUp, Plus, Trash2, X } from "lucide-react";
import { useEffect, useState } from "react";
import type { SkillLockEntry, SkillRecord, StepSkill, Workflow, WorkflowStep } from "../../types";

/**
 * 工作流创建/编辑器：meta + groups 增删 + steps 增删与上移/下移排序 +
 * 每步 skills 选择器（中心库 inventory）+ 占位开关。零新依赖、零新样式。
 */
export function WorkflowEditor({
  initial,
  librarySkills,
  skillLocks,
  busy,
  onCancel,
  onSave
}: {
  /** null = 创建；否则编辑既有工作流（slug 不可改）。 */
  initial: Workflow | null;
  librarySkills: SkillRecord[];
  skillLocks: Record<string, SkillLockEntry>;
  busy: boolean;
  onCancel: () => void;
  /** 保存失败会 reject，编辑器就地展示错误并保持打开。 */
  onSave: (workflow: Workflow, readme?: string) => Promise<void>;
}) {
  const editing = Boolean(initial);
  const [draft, setDraft] = useState<Workflow>(() => initial ? cloneWorkflow(initial) : emptyWorkflow());
  const [readmeDraft, setReadmeDraft] = useState("");
  const [editorError, setEditorError] = useState<string | null>(null);

  useEffect(() => {
    const onEsc = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
    };
    document.addEventListener("keydown", onEsc);
    return () => document.removeEventListener("keydown", onEsc);
  }, [onCancel]);

  function update(patch: Partial<Workflow>) {
    setDraft((current) => ({ ...current, ...patch }));
  }

  function addGroup() {
    const id = nextGroupId(draft.groups.map((group) => group.id));
    update({ groups: [...draft.groups, { id, name: `分组 ${draft.groups.length + 1}` }] });
  }

  function updateGroup(index: number, patch: Partial<{ id: string; name: string }>) {
    setDraft((current) => ({
      ...current,
      groups: current.groups.map((group, i) => (i === index ? { ...group, ...patch } : group))
    }));
  }

  function removeGroup(index: number) {
    const target = draft.groups[index];
    if (draft.steps.some((step) => step.group === target.id)) {
      setEditorError(`分组「${target.name}」仍被步骤引用，请先调整步骤的分组。`);
      return;
    }
    setEditorError(null);
    update({ groups: draft.groups.filter((_, i) => i !== index) });
  }

  function addStep() {
    const fallbackGroup = draft.groups[0]?.id ?? "";
    update({
      steps: [
        ...draft.steps,
        { name: `步骤 ${draft.steps.length + 1}`, group: fallbackGroup, description: "", skills: [] }
      ]
    });
  }

  function updateStep(index: number, patch: Partial<WorkflowStep>) {
    setDraft((current) => ({
      ...current,
      steps: current.steps.map((step, i) => (i === index ? { ...step, ...patch } : step))
    }));
  }

  function moveStep(index: number, direction: -1 | 1) {
    const target = index + direction;
    if (target < 0 || target >= draft.steps.length) return;
    setDraft((current) => {
      const steps = [...current.steps];
      [steps[index], steps[target]] = [steps[target], steps[index]];
      return { ...current, steps };
    });
  }

  function removeStep(index: number) {
    update({ steps: draft.steps.filter((_, i) => i !== index) });
  }

  function addSkill(stepIndex: number) {
    const first = librarySkills[0];
    const next: StepSkill = first
      ? refFromLibrarySkill(first, skillLocks)
      : { sourceType: "github", sourceUrl: "", slug: "", skillPath: undefined };
    updateStepSkills(stepIndex, (skills) => [...skills, next]);
  }

  function updateSkill(stepIndex: number, skillIndex: number, next: StepSkill) {
    updateStepSkills(stepIndex, (skills) => skills.map((skill, i) => (i === skillIndex ? next : skill)));
  }

  function removeSkill(stepIndex: number, skillIndex: number) {
    updateStepSkills(stepIndex, (skills) => skills.filter((_, i) => i !== skillIndex));
  }

  function updateStepSkills(stepIndex: number, mutate: (skills: StepSkill[]) => StepSkill[]) {
    setDraft((current) => ({
      ...current,
      steps: current.steps.map((step, i) =>
        i === stepIndex ? { ...step, skills: mutate(step.skills) } : step
      )
    }));
  }

  async function submit() {
    setEditorError(null);
    const problem = validateDraft(draft);
    if (problem) {
      setEditorError(problem);
      return;
    }
    const normalized = normalizeDraft(draft);
    try {
      await onSave(normalized, readmeDraft.trim() ? readmeDraft : undefined);
    } catch (reason) {
      setEditorError(reason instanceof Error ? reason.message : String(reason));
    }
  }

  return (
    <div className="sheet-backdrop" onClick={onCancel}>
      <aside
        aria-label={editing ? "编辑工作流" : "新建工作流"}
        aria-modal="true"
        className="settings-sheet"
        onClick={(event) => event.stopPropagation()}
        role="dialog"
      >
        <header className="settings-header">
          <div className="settings-header-top">
            <h1>{editing ? `编辑 · ${initial?.slug ?? ""}` : "新建工作流"}</h1>
            <button className="settings-close" onClick={onCancel} title="关闭" type="button">
              <X size={16} />
            </button>
          </div>
        </header>

        <div className="settings-content">
          {editorError && (
            <div className="banner warning" role="alert">
              {editorError}
            </div>
          )}

          <div className="settings-list" role="list">
            <div className="settings-row settings-row-stack" role="listitem">
              <div className="settings-row-copy">
                <strong>名称</strong>
                <span>展示用名称。</span>
              </div>
              <input
                className="settings-path-input"
                onChange={(event) => update({ name: event.target.value })}
                placeholder="例如：代码评审工作流"
                spellCheck={false}
                value={draft.name}
              />
            </div>
            <div className="settings-row settings-row-stack" role="listitem">
              <div className="settings-row-copy">
                <strong>Slug</strong>
                <span>{editing ? "唯一标识，创建后不可修改。" : "小写字母、数字与连字符，创建后不可修改。"}</span>
              </div>
              <input
                className="settings-path-input"
                disabled={editing}
                onChange={(event) => update({ slug: event.target.value })}
                placeholder="例如：code-review-flow"
                spellCheck={false}
                value={draft.slug}
              />
            </div>
            <div className="settings-row settings-row-stack" role="listitem">
              <div className="settings-row-copy">
                <strong>描述</strong>
                <span>一句话说明这个工作流做什么。</span>
              </div>
              <input
                className="settings-path-input"
                onChange={(event) => update({ description: event.target.value })}
                spellCheck={false}
                value={draft.description}
              />
            </div>
            <div className="settings-row" role="listitem">
              <div className="settings-row-copy">
                <strong>版本</strong>
              </div>
              <input
                className="settings-label-input"
                onChange={(event) => update({ version: event.target.value })}
                spellCheck={false}
                value={draft.version}
              />
            </div>
            <div className="settings-row" role="listitem">
              <div className="settings-row-copy">
                <strong>作者（可选）</strong>
              </div>
              <input
                className="settings-label-input"
                onChange={(event) => update({ author: event.target.value })}
                spellCheck={false}
                value={draft.author ?? ""}
              />
            </div>
            <div className="settings-row" role="listitem">
              <div className="settings-row-copy">
                <strong>标签（逗号分隔）</strong>
              </div>
              <input
                className="settings-label-input"
                onChange={(event) => update({ tags: splitTags(event.target.value) })}
                placeholder="例如：review, tdd"
                spellCheck={false}
                value={draft.tags.join(", ")}
              />
            </div>
          </div>

          <section className="settings-block">
            <div className="settings-block-heading settings-block-heading-row">
              <div>
                <h2>分组</h2>
                <p>步骤按分组归类展示；id 仅小写字母、数字与连字符。</p>
              </div>
              <button className="settings-text-button" onClick={addGroup} type="button">
                <Plus size={14} />
                添加分组
              </button>
            </div>
            <div className="settings-list" role="list">
              {draft.groups.map((group, index) => (
                <div className="settings-row settings-custom-root-row" key={`${group.id}-${index}`} role="listitem">
                  <div className="settings-custom-root-fields">
                    <input
                      className="settings-label-input"
                      onChange={(event) => updateGroup(index, { name: event.target.value })}
                      placeholder="显示名称"
                      spellCheck={false}
                      value={group.name}
                    />
                    <input
                      className="settings-label-input"
                      onChange={(event) => updateGroup(index, { id: event.target.value })}
                      placeholder="id"
                      spellCheck={false}
                      value={group.id}
                    />
                  </div>
                  <button
                    className="meta-icon-button danger"
                    onClick={() => removeGroup(index)}
                    title="删除此分组"
                    type="button"
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              ))}
              {draft.groups.length === 0 && (
                <div className="settings-agent-empty settings-custom-empty">还没有分组，请先添加。</div>
              )}
            </div>
          </section>

          <section className="settings-block">
            <div className="settings-block-heading settings-block-heading-row">
              <div>
                <h2>步骤</h2>
                <p>按顺序执行；每步从中心库选择 Skill，或标记为占位待补充。</p>
              </div>
              <button className="settings-text-button" disabled={draft.groups.length === 0} onClick={addStep} type="button">
                <Plus size={14} />
                添加步骤
              </button>
            </div>

            {draft.steps.map((step, stepIndex) => (
              <div className="settings-list" key={stepIndex} role="list">
                <div className="settings-row" role="listitem">
                  <div className="settings-row-copy">
                    <strong>步骤 {stepIndex + 1}</strong>
                  </div>
                  <div className="button-pair compact">
                    <button
                      className="meta-icon-button"
                      disabled={stepIndex === 0}
                      onClick={() => moveStep(stepIndex, -1)}
                      title="上移"
                      type="button"
                    >
                      <ChevronUp size={14} />
                    </button>
                    <button
                      className="meta-icon-button"
                      disabled={stepIndex === draft.steps.length - 1}
                      onClick={() => moveStep(stepIndex, 1)}
                      title="下移"
                      type="button"
                    >
                      <ChevronDown size={14} />
                    </button>
                    <button
                      className="meta-icon-button danger"
                      onClick={() => removeStep(stepIndex)}
                      title="删除此步骤"
                      type="button"
                    >
                      <Trash2 size={14} />
                    </button>
                  </div>
                </div>
                <div className="settings-row settings-row-stack" role="listitem">
                  <div className="settings-row-copy">
                    <strong>步骤名称</strong>
                  </div>
                  <input
                    className="settings-path-input"
                    onChange={(event) => updateStep(stepIndex, { name: event.target.value })}
                    spellCheck={false}
                    value={step.name}
                  />
                </div>
                <div className="settings-row" role="listitem">
                  <div className="settings-row-copy">
                    <strong>所属分组</strong>
                  </div>
                  <div className="field">
                    <select
                      onChange={(event) => updateStep(stepIndex, { group: event.target.value })}
                      value={step.group}
                    >
                      {draft.groups.map((group) => (
                        <option key={group.id} value={group.id}>
                          {group.name}
                        </option>
                      ))}
                      {!draft.groups.some((group) => group.id === step.group) && (
                        <option value={step.group}>{step.group || "未分组"}</option>
                      )}
                    </select>
                  </div>
                </div>
                <div className="settings-row settings-row-stack" role="listitem">
                  <div className="settings-row-copy">
                    <strong>步骤说明（可选）</strong>
                  </div>
                  <input
                    className="settings-path-input"
                    onChange={(event) => updateStep(stepIndex, { description: event.target.value })}
                    spellCheck={false}
                    value={step.description}
                  />
                </div>

                {step.skills.map((skill, skillIndex) => (
                  <SkillEntryEditor
                    key={skillIndex}
                    librarySkills={librarySkills}
                    onChange={(next) => updateSkill(stepIndex, skillIndex, next)}
                    onRemove={() => removeSkill(stepIndex, skillIndex)}
                    skill={skill}
                    skillLocks={skillLocks}
                  />
                ))}

                <div className="settings-row" role="listitem">
                  <div className="settings-row-copy">
                    <span>为此步骤追加 Skill 引用或占位。</span>
                  </div>
                  <button className="settings-text-button" onClick={() => addSkill(stepIndex)} type="button">
                    <Plus size={14} />
                    添加 Skill
                  </button>
                </div>
              </div>
            ))}

            {draft.steps.length === 0 && (
              <div className="settings-agent-empty settings-custom-empty">还没有步骤。添加分组后即可添加步骤。</div>
            )}
          </section>

          <section className="settings-block">
            <div className="settings-block-heading">
              <h2>README（可选）</h2>
              <p>
                {editing
                  ? "留空表示保留既有 README 不变；填写则覆盖。"
                  : "随工作流一起保存的说明文档（Markdown）。"}
              </p>
            </div>
            <div className="settings-list" role="list">
              <div className="settings-row settings-row-stack" role="listitem">
                <textarea
                  className="settings-path-input"
                  onChange={(event) => setReadmeDraft(event.target.value)}
                  placeholder="# 使用说明"
                  rows={5}
                  spellCheck={false}
                  value={readmeDraft}
                />
              </div>
            </div>
          </section>
        </div>

        <footer className="sheet-actions">
          <button className="secondary-button" disabled={busy} onClick={onCancel} type="button">
            取消
          </button>
          <button className="primary-button" disabled={busy} onClick={() => void submit()} type="button">
            <Check size={16} />
            {editing ? "保存" : "创建"}
          </button>
        </footer>
      </aside>
    </div>
  );
}

function SkillEntryEditor({
  skill,
  librarySkills,
  skillLocks,
  onChange,
  onRemove
}: {
  skill: StepSkill;
  librarySkills: SkillRecord[];
  skillLocks: Record<string, SkillLockEntry>;
  onChange: (next: StepSkill) => void;
  onRemove: () => void;
}) {
  const isPlaceholder = "placeholder" in skill;

  function togglePlaceholder() {
    if (isPlaceholder) {
      const first = librarySkills[0];
      onChange(
        first
          ? refFromLibrarySkill(first, skillLocks)
          : { sourceType: "github", sourceUrl: "", slug: "", skillPath: undefined }
      );
    } else {
      onChange({ placeholder: skill.slug ? `待补充：${skill.slug} 的替代` : "待补充" });
    }
  }

  function selectLibrarySkill(slug: string) {
    if (isPlaceholder) return;
    const target = librarySkills.find((item) => item.slug === slug);
    if (!target) {
      onChange({ ...skill, slug });
      return;
    }
    const lock = skillLocks[target.slug];
    onChange({
      ...skill,
      slug: target.slug,
      sourceUrl: lock?.sourceUrl ?? skill.sourceUrl,
      skillPath: lock?.skillPath ?? skill.skillPath
    });
  }

  return (
    <>
      <div className="settings-row" role="listitem">
        <div className="settings-row-copy">
          <strong>{isPlaceholder ? "占位" : "Skill 引用"}</strong>
          <span>{isPlaceholder ? "占位步骤在使用时跳过，需人工补充。" : "从中心库选择；来源 URL 取自 skill.lock，可修正。"}</span>
        </div>
        <div className="button-pair compact">
          <button
            aria-checked={isPlaceholder}
            className={`settings-toggle ${isPlaceholder ? "on" : ""}`}
            onClick={togglePlaceholder}
            role="switch"
            title="占位开关"
            type="button"
          >
            <i />
          </button>
          <button className="meta-icon-button danger" onClick={onRemove} title="移除此条目" type="button">
            <Trash2 size={14} />
          </button>
        </div>
      </div>
      {isPlaceholder ? (
        <div className="settings-row settings-row-stack" role="listitem">
          <input
            className="settings-path-input"
            onChange={(event) => onChange({ placeholder: event.target.value })}
            placeholder="占位说明，例如：待指定修复类 skill"
            spellCheck={false}
            value={skill.placeholder}
          />
        </div>
      ) : (
        <div className="settings-row settings-row-stack" role="listitem">
          <div className="field">
            <select onChange={(event) => selectLibrarySkill(event.target.value)} value={skill.slug}>
              <option value="">选择中心库 Skill…</option>
              {librarySkills.map((item) => (
                <option key={item.id} value={item.slug}>
                  {item.displayName}（{item.slug}）
                </option>
              ))}
              {skill.slug && !librarySkills.some((item) => item.slug === skill.slug) && (
                <option value={skill.slug}>{skill.slug}（不在中心库）</option>
              )}
            </select>
          </div>
          <input
            className="settings-path-input"
            onChange={(event) => onChange({ ...skill, sourceUrl: event.target.value })}
            placeholder="来源 URL，例如 https://github.com/owner/repo.git"
            spellCheck={false}
            value={skill.sourceUrl}
          />
          <input
            className="settings-path-input"
            onChange={(event) => onChange({ ...skill, skillPath: event.target.value || undefined })}
            placeholder="仓库内目录（可选），例如 skills/category/slug"
            spellCheck={false}
            value={skill.skillPath ?? ""}
          />
        </div>
      )}
    </>
  );
}

function emptyWorkflow(): Workflow {
  return {
    name: "",
    slug: "",
    version: "0.1.0",
    description: "",
    author: undefined,
    tags: [],
    icon: undefined,
    groups: [{ id: "default", name: "默认分组" }],
    steps: []
  };
}

function cloneWorkflow(workflow: Workflow): Workflow {
  return {
    ...workflow,
    tags: [...workflow.tags],
    groups: workflow.groups.map((group) => ({ ...group })),
    steps: workflow.steps.map((step) => ({
      ...step,
      skills: step.skills.map((skill) => ("placeholder" in skill ? { ...skill } : { ...skill }))
    }))
  };
}

function refFromLibrarySkill(skill: SkillRecord, skillLocks: Record<string, SkillLockEntry>): StepSkill {
  const lock = skillLocks[skill.slug];
  return {
    sourceType: lock?.sourceType ?? "github",
    sourceUrl: lock?.sourceUrl ?? "",
    slug: skill.slug,
    skillPath: lock?.skillPath
  };
}

function nextGroupId(existing: string[]) {
  let index = existing.length + 1;
  let id = `group-${index}`;
  while (existing.includes(id)) {
    index += 1;
    id = `group-${index}`;
  }
  return id;
}

function splitTags(text: string) {
  return text
    .split(/[,，]/)
    .map((item) => item.trim())
    .filter(Boolean);
}

function validateDraft(draft: Workflow): string | null {
  if (!draft.name.trim()) return "请填写名称。";
  if (!/^[a-z0-9-]+$/.test(draft.slug)) return "Slug 只能包含小写字母、数字与连字符。";
  if (!draft.version.trim()) return "请填写版本号。";
  if (draft.groups.length === 0) return "请至少保留一个分组。";
  const groupIds = new Set(draft.groups.map((group) => group.id));
  for (const group of draft.groups) {
    if (!/^[a-z0-9-]+$/.test(group.id)) return `分组 id「${group.id}」只能包含小写字母、数字与连字符。`;
    if (!group.name.trim()) return "每个分组都需要显示名称。";
  }
  for (const [index, step] of draft.steps.entries()) {
    if (!step.name.trim()) return `步骤 ${index + 1} 需要名称。`;
    if (!groupIds.has(step.group)) return `步骤「${step.name}」引用了不存在的分组「${step.group}」。`;
    for (const skill of step.skills) {
      if ("placeholder" in skill) {
        if (!skill.placeholder.trim()) return `步骤「${step.name}」的占位说明不能为空。`;
      } else {
        if (!skill.slug.trim()) return `步骤「${step.name}」有未选择 Skill 的引用。`;
        if (!skill.sourceUrl.trim()) return `步骤「${step.name}」的 ${skill.slug} 缺少来源 URL。`;
      }
    }
  }
  return null;
}

function normalizeDraft(draft: Workflow): Workflow {
  return {
    ...draft,
    name: draft.name.trim(),
    version: draft.version.trim(),
    description: draft.description.trim(),
    author: draft.author?.trim() ? draft.author.trim() : undefined,
    icon: draft.icon?.trim() ? draft.icon.trim() : undefined,
    tags: draft.tags.map((tag) => tag.trim()).filter(Boolean),
    groups: draft.groups.map((group) => ({ id: group.id.trim(), name: group.name.trim() })),
    steps: draft.steps.map((step) => ({
      ...step,
      name: step.name.trim(),
      description: step.description.trim(),
      skills: step.skills.map((skill) =>
        "placeholder" in skill
          ? { placeholder: skill.placeholder.trim() }
          : {
              sourceType: skill.sourceType || "github",
              sourceUrl: skill.sourceUrl.trim(),
              slug: skill.slug.trim(),
              skillPath: skill.skillPath?.trim() ? skill.skillPath.trim() : undefined
            }
      )
    }))
  };
}
