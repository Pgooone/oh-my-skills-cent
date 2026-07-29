//! workflow-use：「使用工作流」= 生成 Sync Plan（ADR-0003）。
//!
//! 操作序列（全部纳入 SyncPlan 预览，影响范围先可见）：
//! 1. `download-to-library`：每个中心库缺失的 SkillRef 一条，执行时克隆解析
//!    来源仓库并复制到 `library_path/<slug>/`（两种输出形态都需要：打包内容
//!    与独立同步都以中心库为来源）；
//! 2. 标准同步 ops（**仅入口清单形态**）：复用 sync_plan 的既有同步/迁移路径
//!    （library → targets，copy 或 symlink）——清单模式依赖各 skill 独立安装
//!    于同级目录；打包形态自包含（ADR-0009），skill 内容经 output ops 拷入包内
//!    `skills/`，不独立同步（输出形态使用时二选一）；
//! 3. output ops（按 OutputForm 二选一，写入每个 target skills 根）：
//!    - 入口清单：`_workflow-<slug>/` = workflow.yaml 拷贝 + README 生成
//!      （分组 → 步骤 → 有序 skill 列表（D5）+ 同级目录指引）；
//!    - 打包 skill：`<workflow-slug>/` = SKILL.md 编排正文 + `skills/` 子目录
//!      结构化拷贝（ADR-0009）。
//! 占位步骤不进任何 op，只进 preconditions 的 warning 条目。

use crate::context::AppContext;
use crate::fs_ops::{ensure_dir, hash_dir, path_to_string, remove_entry};
use crate::models::{AgentTarget, SyncOperation, SyncPlan};
use crate::settings::{app_data_dir, load_settings};
use crate::skill_ops::normalize_github_url;
use crate::workflow::{SkillRef, StepSkill, Workflow, WORKFLOW_FILE, workflows_dir};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// 每步每个 skill 的就绪状态（详情页与预览共用）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StepSkillStatus {
    /// 中心库已有（library_path/<slug>/SKILL.md 存在）
    Ready,
    /// 中心库没有（使用时将下载）
    Missing,
    /// 占位步骤（使用时跳过，需人工补充）
    Placeholder(String),
}

/// 详情页展示用的每 skill 视图（与 StepSkill 对齐，扁平化便于 serde/前端消费）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StepSkillView {
    /// "ref" | "placeholder"
    pub kind: String,
    pub slug: Option<String>,
    pub source_url: Option<String>,
    pub skill_path: Option<String>,
    pub placeholder: Option<String>,
}

/// 使用工作流的输出形态（ADR-0004 双形态）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum OutputForm {
    /// 入口清单：`_workflow-<slug>/`（workflow.yaml + README）
    EntryManifest,
    /// 打包 skill：`<workflow-slug>/`（SKILL.md + skills/ 结构化拷贝）
    PackagedSkill,
}

/// 按步骤对齐计算每个 skill 的就绪状态（外层 steps、内层 skills）。
pub fn compute_statuses(
    ctx: &AppContext,
    workflow: &Workflow,
) -> Result<Vec<Vec<(StepSkillView, StepSkillStatus)>>, String> {
    let settings = load_settings(ctx)?;
    let library_path = PathBuf::from(&settings.library_path);
    Ok(compute_statuses_with_library(&library_path, workflow))
}

fn compute_statuses_with_library(
    library_path: &Path,
    workflow: &Workflow,
) -> Vec<Vec<(StepSkillView, StepSkillStatus)>> {
    workflow
        .steps
        .iter()
        .map(|step| {
            step.skills
                .iter()
                .map(|skill| match skill {
                    StepSkill::Ref(reference) => {
                        let view = StepSkillView {
                            kind: "ref".to_string(),
                            slug: Some(reference.slug.clone()),
                            source_url: Some(reference.source_url.clone()),
                            skill_path: reference.skill_path.clone(),
                            placeholder: None,
                        };
                        let status = if library_path
                            .join(&reference.slug)
                            .join("SKILL.md")
                            .exists()
                        {
                            StepSkillStatus::Ready
                        } else {
                            StepSkillStatus::Missing
                        };
                        (view, status)
                    }
                    StepSkill::Placeholder { placeholder } => {
                        let view = StepSkillView {
                            kind: "placeholder".to_string(),
                            slug: None,
                            source_url: None,
                            skill_path: None,
                            placeholder: Some(placeholder.clone()),
                        };
                        (view, StepSkillStatus::Placeholder(placeholder.clone()))
                    }
                })
                .collect()
        })
        .collect()
}

/// 使用工作流（生产入口）：加载已安装工作流（load 内含 validate，来源 URL
/// 在此把关 GitHub-only），生成 SyncPlan 预览并落盘，执行复用 apply_plan。
pub fn preview_use_workflow(
    ctx: &AppContext,
    slug: &str,
    targets: Vec<AgentTarget>,
    method: String,
    output_form: OutputForm,
) -> Result<SyncPlan, String> {
    let workflow = crate::workflow::load(ctx, slug)?;
    let yaml_path = workflows_dir(ctx).join(slug).join(WORKFLOW_FILE);
    let workflow_yaml = fs::read_to_string(&yaml_path).map_err(|error| {
        format!(
            "Unable to read workflow at {}: {error}",
            path_to_string(&yaml_path)
        )
    })?;
    preview_use_for(ctx, &workflow, &workflow_yaml, targets, &method, output_form)
}

/// preview 核心。与生产入口分离是为了让单元测试能传入内存构造的 Workflow
/// （来源 URL 为本地 fixture git 仓库），绕开 validate 的 GitHub-only 校验——
/// 与 workflow_registry 的 `*_from_source` 测试钩子同一模式。调用方必须保证
/// workflow 已通过 validate（或明确处于测试场景）。
pub(crate) fn preview_use_for(
    ctx: &AppContext,
    workflow: &Workflow,
    workflow_yaml: &str,
    targets: Vec<AgentTarget>,
    method: &str,
    output_form: OutputForm,
) -> Result<SyncPlan, String> {
    if method != "copy" && method != "symlink" {
        return Err(format!(
            "Unsupported sync method '{method}': expected 'copy' or 'symlink'"
        ));
    }

    let settings = load_settings(ctx)?;
    let library_path = PathBuf::from(&settings.library_path);
    let plan_id = format!("workflow-use-{}", Utc::now().timestamp_millis());
    let created_at = Utc::now().to_rfc3339();
    let mut operations = Vec::new();
    let mut preconditions = Vec::new();
    let mut blocked_conflicts = Vec::new();

    let refs = collect_unique_refs(workflow, &mut preconditions, &mut blocked_conflicts);

    // 1. download-to-library（每个中心库缺失的 ref 一条，slug 已去重）
    for reference in &refs {
        let target = library_path.join(&reference.slug);
        if target.join("SKILL.md").exists() {
            continue;
        }
        preconditions.push(format!(
            "将从 {} 下载 {} 到中心库",
            reference.source_url, reference.slug
        ));
        operations.push(download_operation(reference, &target));
    }

    // 中心库已有同名 slug 但 skill.lock 记录的来源与工作流引用不同 → blocked
    detect_lock_source_conflicts(&refs, &library_path, &mut blocked_conflicts);

    // 2. 标准同步 ops（library → targets，复用既有同步/迁移路径）。
    // 仅入口清单形态需要：清单模式依赖各 skill 独立安装于同级目录；打包形态
    // 自包含（ADR-0009），skill 内容经 output ops 拷入包内 skills/，不独立同步。
    if output_form == OutputForm::EntryManifest {
        for reference in &refs {
            let source = library_path.join(&reference.slug);
            let source_hash = if source.join("SKILL.md").exists() {
                Some(hash_dir(&source)?)
            } else {
                // 缺失 skill 将由 download-to-library 先执行补齐；此处无法比对内容
                None
            };
            crate::sync_plan::append_library_sync_for_workflow(
                &settings,
                &plan_id,
                &reference.slug,
                source_hash.as_deref(),
                &targets,
                method,
                &mut operations,
                &mut blocked_conflicts,
                &mut preconditions,
            );
        }
    }

    // 3. output ops（写入每个 target skills 根）
    let roots = crate::sync_plan::resolve_skill_roots_for_targets(&settings, &targets);
    if roots.is_empty() {
        push_blocked_once(
            &mut blocked_conflicts,
            "未能解析任何目标 Agent 的 skills 目录".to_string(),
        );
    } else {
        append_output_operations(
            ctx,
            workflow,
            workflow_yaml,
            &refs,
            &library_path,
            &roots,
            &plan_id,
            output_form,
            &mut operations,
            &mut blocked_conflicts,
        )?;
    }

    let risk_level = if blocked_conflicts.is_empty() {
        "low"
    } else {
        "blocked"
    };
    let plan = SyncPlan {
        plan_id,
        kind: "workflow-use".to_string(),
        risk_level: risk_level.to_string(),
        operations,
        preconditions,
        blocked_conflicts,
        created_at,
    };
    crate::sync_plan::save_plan_for_workflow(ctx, &plan)?;
    Ok(plan)
}

/// 按 output_form 生成暂存产物并追加 output ops。生成物（README/SKILL.md/
/// workflow.yaml 拷贝）在 preview 时落到 plans/<plan_id>-output/ 暂存区，
/// apply 时经既有 copy-to-target 分支写入 target——因此 output 无需新增
/// op_type；打包形态的 skills/ 内容在 apply 时从中心库拷贝（此时 download
/// 已先行执行，中心库必然齐备）。
#[allow(clippy::too_many_arguments)]
fn append_output_operations(
    ctx: &AppContext,
    workflow: &Workflow,
    workflow_yaml: &str,
    refs: &[SkillRef],
    library_path: &Path,
    roots: &[(String, PathBuf)],
    plan_id: &str,
    output_form: OutputForm,
    operations: &mut Vec<SyncOperation>,
    blocked_conflicts: &mut Vec<String>,
) -> Result<(), String> {
    let staging = app_data_dir(ctx)?
        .join("plans")
        .join(format!("{plan_id}-output"));
    if staging.exists() {
        remove_entry(&staging)?;
    }

    match output_form {
        OutputForm::EntryManifest => {
            let manifest_dir = staging.join("manifest");
            write_entry_manifest(workflow, workflow_yaml, &manifest_dir)?;
            for (agent_id, root) in roots {
                let target = root.join(format!("_workflow-{}", workflow.slug));
                if fs::symlink_metadata(&target).is_ok() {
                    blocked_conflicts.push(format!(
                        "{} 已存在，请先移除后再生成入口清单",
                        path_to_string(&target)
                    ));
                    continue;
                }
                operations.push(output_operation(
                    &manifest_dir,
                    &target,
                    agent_id,
                    &workflow.slug,
                    &format!("生成工作流入口清单 _workflow-{}", workflow.slug),
                ));
            }
        }
        OutputForm::PackagedSkill => {
            let packaged_dir = staging.join("packaged");
            write_packaged_skill(workflow, &packaged_dir)?;
            for (agent_id, root) in roots {
                let target = root.join(&workflow.slug);
                if fs::symlink_metadata(&target).is_ok() {
                    blocked_conflicts.push(format!(
                        "{} 已存在，请先移除后再生成打包 skill",
                        path_to_string(&target)
                    ));
                    continue;
                }
                operations.push(output_operation(
                    &packaged_dir,
                    &target,
                    agent_id,
                    &workflow.slug,
                    &format!("生成打包 skill {}（SKILL.md 编排入口）", workflow.slug),
                ));
                for reference in refs {
                    let from = library_path.join(&reference.slug);
                    let to = target.join("skills").join(&reference.slug);
                    operations.push(output_operation(
                        &from,
                        &to,
                        agent_id,
                        &reference.slug,
                        &format!("将 {} 打包进 {}/skills/", reference.slug, workflow.slug),
                    ));
                }
            }
        }
    }
    Ok(())
}

/// 收集去重后的 SkillRef（按首次出现顺序），同时产出占位 warning 与同 slug
/// 异源引用冲突。
fn collect_unique_refs(
    workflow: &Workflow,
    preconditions: &mut Vec<String>,
    blocked_conflicts: &mut Vec<String>,
) -> Vec<SkillRef> {
    let mut refs: Vec<SkillRef> = Vec::new();
    for step in &workflow.steps {
        for skill in &step.skills {
            match skill {
                StepSkill::Ref(reference) => {
                    if let Some(existing) = refs.iter().find(|r| r.slug == reference.slug) {
                        if existing.source_url != reference.source_url
                            || existing.skill_path != reference.skill_path
                        {
                            blocked_conflicts.push(format!(
                                "Skill '{}' 被多个不同来源引用（{} 与 {}）",
                                reference.slug, existing.source_url, reference.source_url
                            ));
                        }
                    } else {
                        refs.push(reference.clone());
                    }
                }
                StepSkill::Placeholder { placeholder } => {
                    preconditions.push(format!(
                        "步骤「{}」含占位 skill：{}（已跳过）",
                        step.name, placeholder
                    ));
                }
            }
        }
    }
    refs
}

/// 「中心库已有同名但内容不同」冲突的可判定近似：中心库已有该 slug，且
/// skill.lock 记录了它的来源，与工作流引用来源归一化后不一致 → blocked。
/// lock 中无记录（手工 adopt）时不做判定。
fn detect_lock_source_conflicts(
    refs: &[SkillRef],
    library_path: &Path,
    blocked_conflicts: &mut Vec<String>,
) {
    let lock = crate::skill_ops::read_skill_lock().unwrap_or_default();
    for reference in refs {
        if !library_path
            .join(&reference.slug)
            .join("SKILL.md")
            .exists()
        {
            continue;
        }
        let Some(entry) = lock.get(&reference.slug) else {
            continue;
        };
        let Some(lock_url) = entry.source_url.as_deref() else {
            continue;
        };
        let (Ok(locked), Ok(referenced)) = (
            normalize_github_url(lock_url),
            normalize_github_url(&reference.source_url),
        ) else {
            continue;
        };
        if locked != referenced {
            blocked_conflicts.push(format!(
                "中心库已有 {}（来自 {}），与工作流引用的来源 {} 不同",
                reference.slug, locked, referenced
            ));
        }
    }
}

fn write_entry_manifest(
    workflow: &Workflow,
    workflow_yaml: &str,
    dir: &Path,
) -> Result<(), String> {
    ensure_dir(dir)?;
    let yaml_file = dir.join(WORKFLOW_FILE);
    fs::write(&yaml_file, workflow_yaml).map_err(|error| {
        format!(
            "Unable to write workflow manifest at {}: {error}",
            path_to_string(&yaml_file)
        )
    })?;
    let readme_file = dir.join("README.md");
    fs::write(&readme_file, render_entry_manifest_readme(workflow)).map_err(|error| {
        format!(
            "Unable to write workflow README at {}: {error}",
            path_to_string(&readme_file)
        )
    })
}

fn write_packaged_skill(workflow: &Workflow, dir: &Path) -> Result<(), String> {
    ensure_dir(dir)?;
    let skill_file = dir.join("SKILL.md");
    fs::write(&skill_file, render_packaged_skill_markdown(workflow)).map_err(|error| {
        format!(
            "Unable to write packaged SKILL.md at {}: {error}",
            path_to_string(&skill_file)
        )
    })
}

/// 入口清单 README：分组 → 步骤 → 每步说明与**有序** skill 列表（D5），
/// 并指引各 skill 已独立安装于同级目录。
pub fn render_entry_manifest_readme(workflow: &Workflow) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}（{}）\n\n", workflow.name, workflow.slug));
    out.push_str(&format!("{}\n\n", workflow.description));
    out.push_str("> 本目录是工作流入口清单，由 Oh My Skills Cent 生成。\n");
    out.push_str("> 工作流的机读定义见同目录 `workflow.yaml`。\n");
    out.push_str(
        "> 各 skill 已独立安装于本目录的**同级目录**；按下列分组与步骤顺序，依次阅读对应的 `SKILL.md` 并遵循。\n\n",
    );
    append_orchestration(&mut out, workflow, &|slug, _| {
        format!("`../{slug}/SKILL.md`")
    });
    out
}

/// 打包 skill 的 SKILL.md：frontmatter（name/description）+ 编排正文
/// （分组 → 步骤 → 每步该做什么、按顺序读 `skills/` 下哪个）。
pub fn render_packaged_skill_markdown(workflow: &Workflow) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("name: {}\n", workflow.slug));
    out.push_str(&format!(
        "description: {}\n",
        yaml_double_quoted(&single_line(&workflow.description))
    ));
    out.push_str("---\n\n");
    out.push_str(&format!("# {}\n\n", workflow.name));
    out.push_str(&format!("{}\n\n", workflow.description));
    out.push_str(
        "本 skill 是工作流编排包（自包含目录）：先读本说明，再按各步骤的指引顺序阅读 `skills/` 子目录中对应的 skill 并遵循。\n\n",
    );
    append_orchestration(&mut out, workflow, &|slug, order| {
        format!("`skills/{slug}/SKILL.md`（第 {order} 个）")
    });
    out
}

/// 两形态共用的编排正文：按 groups 顺序遍历，组内按 steps 数组顺序列出
/// 命中该组的步骤；步骤编号全局连续（跨组递增，便于线序引用）；每步的
/// skills 按数组顺序编号（D5）。占位 skill 以「占位」醒目列出，不进编号。
/// 未被任何组覆盖的步骤归入末尾「未分组步骤」。
fn append_orchestration(
    out: &mut String,
    workflow: &Workflow,
    link: &dyn Fn(&str, usize) -> String,
) {
    let mut covered: Vec<bool> = vec![false; workflow.steps.len()];
    let mut step_number = 0;
    for group in &workflow.groups {
        out.push_str(&format!("## {}\n\n", group.name));
        for (index, step) in workflow.steps.iter().enumerate() {
            if step.group != group.id {
                continue;
            }
            covered[index] = true;
            step_number += 1;
            append_step(out, step, step_number, link);
        }
    }

    let orphans: Vec<&crate::workflow::WorkflowStep> = workflow
        .steps
        .iter()
        .zip(covered.iter())
        .filter(|(_, is_covered)| !**is_covered)
        .map(|(step, _)| step)
        .collect();
    if !orphans.is_empty() {
        out.push_str("## 未分组步骤\n\n");
        for step in orphans {
            step_number += 1;
            append_step(out, step, step_number, link);
        }
    }
}

fn append_step(
    out: &mut String,
    step: &crate::workflow::WorkflowStep,
    step_number: usize,
    link: &dyn Fn(&str, usize) -> String,
) {
    out.push_str(&format!("### 步骤 {}：{}\n\n", step_number, step.name));
    if !step.description.trim().is_empty() {
        out.push_str(&format!("{}\n\n", step.description));
    }
    let mut order = 0;
    for skill in &step.skills {
        match skill {
            StepSkill::Ref(reference) => {
                order += 1;
                out.push_str(&format!(
                    "{}. `{}` — 见 {}\n",
                    order,
                    reference.slug,
                    link(&reference.slug, order)
                ));
            }
            StepSkill::Placeholder { placeholder } => {
                out.push_str(&format!("- **占位**：{}（需人工指定后补充）\n", placeholder));
            }
        }
    }
    if order == 0 && step.skills.is_empty() {
        out.push_str("（本步骤无关联 skill）\n");
    }
    out.push('\n');
}

/// frontmatter description 必须单行：折行合并为空格分隔。
fn single_line(value: &str) -> String {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// 双引号包裹并转义，保证任意 description 都是合法 YAML 标量。
fn yaml_double_quoted(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn download_operation(reference: &SkillRef, target: &Path) -> SyncOperation {
    let seed = format!(
        "download-to-library:{}:{}:{}",
        reference.slug,
        reference.source_url,
        path_to_string(target)
    );
    SyncOperation {
        id: stable_id(&seed),
        op_type: "download-to-library".to_string(),
        status: "planned".to_string(),
        source_path: Some(reference.source_url.clone()),
        target_path: Some(path_to_string(target)),
        backup_path: None,
        message: format!("从 {} 下载 {} 到中心库", reference.source_url, reference.slug),
        agent_id: None,
        skill_id: Some(reference.slug.clone()),
        skill_path: reference.skill_path.clone(),
    }
}

fn output_operation(
    source: &Path,
    target: &Path,
    agent_id: &str,
    skill_id: &str,
    message: &str,
) -> SyncOperation {
    let seed = format!(
        "workflow-output:{}:{}:{}",
        agent_id,
        path_to_string(source),
        path_to_string(target)
    );
    SyncOperation {
        id: stable_id(&seed),
        op_type: "copy-to-target".to_string(),
        status: "planned".to_string(),
        source_path: Some(path_to_string(source)),
        target_path: Some(path_to_string(target)),
        backup_path: None,
        message: message.to_string(),
        agent_id: Some(agent_id.to_string()),
        skill_id: Some(skill_id.to_string()),
        skill_path: None,
    }
}

fn stable_id(seed: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(seed.as_bytes());
    format!("{:x}", hasher.finalize())[..16].to_string()
}

fn push_blocked_once(blocked_conflicts: &mut Vec<String>, message: String) {
    if !blocked_conflicts
        .iter()
        .any(|existing| existing == &message)
    {
        blocked_conflicts.push(message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::{WorkflowGroup, WorkflowStep};
    use std::process::Command;
    use std::sync::Mutex;

    fn test_ctx(temp: &tempfile::TempDir) -> AppContext {
        AppContext::new(temp.path().join("data"), temp.path().join("home"))
    }

    fn skill_ref(slug: &str, source_url: &str, skill_path: Option<&str>) -> StepSkill {
        StepSkill::Ref(SkillRef {
            source_type: "github".to_string(),
            source_url: source_url.to_string(),
            slug: slug.to_string(),
            skill_path: skill_path.map(|path| path.to_string()),
        })
    }

    const GITHUB_SOURCE: &str = "https://github.com/mattpocock/skills.git";

    /// missing×2 + installed×1 + placeholder×1 的混合 case 工作流。
    fn mixed_workflow(missing_a_url: &str, missing_b_url: &str) -> Workflow {
        Workflow {
            name: "混合流程".to_string(),
            slug: "mixed-flow".to_string(),
            version: "0.1.0".to_string(),
            description: "覆盖 ready / missing / 占位 的混合 case".to_string(),
            author: None,
            tags: Vec::new(),
            icon: None,
            groups: vec![
                WorkflowGroup {
                    id: "prep".to_string(),
                    name: "准备".to_string(),
                },
                WorkflowGroup {
                    id: "exec".to_string(),
                    name: "执行".to_string(),
                },
            ],
            steps: vec![
                WorkflowStep {
                    name: "收集材料".to_string(),
                    group: "prep".to_string(),
                    description: "先收齐上下文".to_string(),
                    skills: vec![
                        skill_ref("ready-skill", GITHUB_SOURCE, Some("skills/eng/ready-skill")),
                        skill_ref("missing-a", missing_a_url, Some("skills/cat/missing-a")),
                    ],
                },
                WorkflowStep {
                    name: "动手修复".to_string(),
                    group: "exec".to_string(),
                    description: String::new(),
                    skills: vec![
                        skill_ref("missing-b", missing_b_url, None),
                        StepSkill::Placeholder {
                            placeholder: "待指定修复类 skill".to_string(),
                        },
                    ],
                },
            ],
        }
    }

    fn library_root(ctx: &AppContext) -> PathBuf {
        ctx.home_dir().join(".oh-my-skills").join("skills")
    }

    fn write_skill(dir: &Path, slug: &str) {
        let skill_dir = dir.join(slug);
        ensure_dir(&skill_dir).expect("skill dir");
        fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\nname: {slug}\ndescription: {slug} desc\n---\nBody {slug}\n"),
        )
        .expect("skill md");
    }

    // ---- env 隔离：agent 检测走真实 HOME/USERPROFILE/PATH，preview 测试用
    // tempdir 伪造（claude stub 提供 installed 证据；home 隔离 skill.lock 与
    // agent skills 根）。所有操纵进程 env 的测试经 ENV_LOCK 串行。
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        saved: Vec<(String, Option<std::ffi::OsString>)>,
    }

    impl EnvGuard {
        fn install(home: &Path, stub_bin: &Path) -> EnvGuard {
            let mut guard = EnvGuard { saved: Vec::new() };
            guard.set("HOME", home.as_os_str());
            guard.set("USERPROFILE", home.as_os_str());

            let old_path = std::env::var_os("PATH");
            guard.saved.push(("PATH".to_string(), old_path.clone()));
            let mut dirs = vec![stub_bin.to_path_buf()];
            if let Some(old) = &old_path {
                dirs.extend(std::env::split_paths(old));
            }
            let joined = std::env::join_paths(dirs).expect("join PATH");
            std::env::set_var("PATH", joined);
            guard
        }

        fn set(&mut self, key: &str, value: &std::ffi::OsStr) {
            self.saved.push((key.to_string(), std::env::var_os(key)));
            std::env::set_var(key, value);
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            let saved: Vec<_> = self.saved.drain(..).collect();
            for (key, value) in saved.into_iter().rev() {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    fn write_claude_stub(stub_bin: &Path) {
        ensure_dir(stub_bin).expect("stub bin");
        fs::write(stub_bin.join("claude"), "stub").expect("claude stub");
        fs::write(stub_bin.join("claude.exe"), "stub").expect("claude.exe stub");
    }

    fn claude_target() -> Vec<AgentTarget> {
        vec![AgentTarget {
            agent_id: "claude-code".to_string(),
            scope: Some("global".to_string()),
            project_path: None,
        }]
    }

    fn op_types(plan: &SyncPlan) -> Vec<&str> {
        plan.operations
            .iter()
            .map(|operation| operation.op_type.as_str())
            .collect()
    }

    // ---- 缺失计算（三分支）------------------------------------------------

    #[test]
    fn statuses_cover_ready_missing_and_placeholder() {
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        write_skill(&library_root(&ctx), "ready-skill");

        let workflow = mixed_workflow(GITHUB_SOURCE, GITHUB_SOURCE);
        let statuses = compute_statuses(&ctx, &workflow).expect("statuses");

        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].len(), 2);
        assert_eq!(statuses[0][0].1, StepSkillStatus::Ready);
        assert_eq!(statuses[0][0].0.kind, "ref");
        assert_eq!(statuses[0][0].0.slug.as_deref(), Some("ready-skill"));
        assert_eq!(
            statuses[0][0].0.skill_path.as_deref(),
            Some("skills/eng/ready-skill")
        );
        assert_eq!(statuses[0][1].1, StepSkillStatus::Missing);
        assert_eq!(statuses[1].len(), 2);
        assert_eq!(statuses[1][0].1, StepSkillStatus::Missing);
        assert_eq!(
            statuses[1][1].1,
            StepSkillStatus::Placeholder("待指定修复类 skill".to_string())
        );
        assert_eq!(statuses[1][1].0.kind, "placeholder");
        assert_eq!(
            statuses[1][1].0.placeholder.as_deref(),
            Some("待指定修复类 skill")
        );
    }

    // ---- 生成器（纯函数）--------------------------------------------------

    #[test]
    fn entry_manifest_readme_lists_skills_in_order_with_sibling_guidance() {
        let workflow = mixed_workflow(GITHUB_SOURCE, GITHUB_SOURCE);
        let readme = render_entry_manifest_readme(&workflow);

        assert!(readme.contains("# 混合流程（mixed-flow）"));
        assert!(readme.contains("## 准备"));
        assert!(readme.contains("## 执行"));
        assert!(readme.contains("### 步骤 1：收集材料"));
        assert!(readme.contains("### 步骤 2：动手修复"));
        assert!(readme.contains("workflow.yaml"));

        // D5：skills 数组顺序即编号顺序；同级目录指引
        let first = readme.find("1. `ready-skill` — 见 `../ready-skill/SKILL.md`");
        let second = readme.find("2. `missing-a` — 见 `../missing-a/SKILL.md`");
        assert!(first.is_some() && second.is_some(), "readme:\n{readme}");
        assert!(first.unwrap() < second.unwrap());
        assert!(readme.contains("1. `missing-b` — 见 `../missing-b/SKILL.md`"));

        // 占位步骤醒目标注，不进编号
        assert!(readme.contains("**占位**：待指定修复类 skill"));
        assert!(!readme.contains("2. `占位"));
    }

    #[test]
    fn packaged_skill_markdown_has_frontmatter_and_structured_guidance() {
        let mut workflow = mixed_workflow(GITHUB_SOURCE, GITHUB_SOURCE);
        workflow.description = "多行\n描述：含冒号 \"引号\"".to_string();
        let markdown = render_packaged_skill_markdown(&workflow);

        // frontmatter：name 为 slug；description 单行化且双引号包裹（YAML 安全）
        assert!(markdown.starts_with("---\nname: mixed-flow\ndescription: \""));
        let header_end = markdown.find("---\n\n").expect("frontmatter end");
        let frontmatter = &markdown[..header_end];
        assert!(frontmatter.contains("多行 描述：含冒号 \\\"引号\\\"\""));

        // 编排正文指向 skills/ 子目录结构化拷贝
        assert!(markdown.contains("1. `ready-skill` — 见 `skills/ready-skill/SKILL.md`"));
        assert!(markdown.contains("2. `missing-a` — 见 `skills/missing-a/SKILL.md`"));
        assert!(markdown.contains("**占位**：待指定修复类 skill"));
    }

    #[test]
    fn orchestration_collects_orphan_steps_into_ungrouped_section() {
        let mut workflow = mixed_workflow(GITHUB_SOURCE, GITHUB_SOURCE);
        workflow.steps.push(WorkflowStep {
            name: "游离步骤".to_string(),
            group: "no-such-group".to_string(),
            description: String::new(),
            skills: vec![skill_ref("ready-skill", GITHUB_SOURCE, None)],
        });

        let readme = render_entry_manifest_readme(&workflow);
        assert!(readme.contains("## 未分组步骤"), "readme:\n{readme}");
        // 步骤编号全局连续：前两组各 1 步，游离步骤编为步骤 3
        assert!(readme.contains("### 步骤 3：游离步骤"));
    }

    // ---- models 兼容与入参校验 ---------------------------------------------

    #[test]
    fn legacy_plan_json_without_skill_path_still_deserializes() {
        let json = "{\"id\":\"a\",\"opType\":\"copy-to-target\",\"status\":\"planned\",\
            \"sourcePath\":null,\"targetPath\":null,\"backupPath\":null,\
            \"message\":\"m\",\"agentId\":null,\"skillId\":null}";
        let operation: crate::models::SyncOperation =
            serde_json::from_str(json).expect("legacy op json");
        assert_eq!(operation.skill_path, None);
    }

    #[test]
    fn preview_rejects_unknown_sync_method() {
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        let workflow = mixed_workflow(GITHUB_SOURCE, GITHUB_SOURCE);
        let yaml = workflow.to_yaml().expect("yaml");
        let error = preview_use_for(
            &ctx,
            &workflow,
            &yaml,
            Vec::new(),
            "hardlink",
            OutputForm::EntryManifest,
        )
        .expect_err("unknown method must fail");
        assert!(error.contains("hardlink"), "error: {error}");
    }

    // ---- preview 操作序列 ---------------------------------------------------

    #[test]
    fn preview_plans_downloads_first_then_sync_then_output() {
        // 容忍 poison：一个 env 测试 panic 不应让其余 env 测试连锁失败
        let _lock = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        let stub_bin = temp.path().join("bin");
        write_claude_stub(&stub_bin);
        let _env = EnvGuard::install(ctx.home_dir(), &stub_bin);

        write_skill(&library_root(&ctx), "ready-skill");
        let workflow = mixed_workflow(GITHUB_SOURCE, GITHUB_SOURCE);
        let yaml = workflow.to_yaml().expect("yaml");

        let plan = preview_use_for(
            &ctx,
            &workflow,
            &yaml,
            claude_target(),
            "copy",
            OutputForm::EntryManifest,
        )
        .expect("preview");

        assert!(
            plan.blocked_conflicts.is_empty(),
            "blocked: {:?}",
            plan.blocked_conflicts
        );
        assert_eq!(plan.kind, "workflow-use");
        assert_eq!(plan.risk_level, "low");

        let types = op_types(&plan);
        // 操作序列：downloads 在前 → 标准同步 ops → output ops
        assert_eq!(types.len(), 9, "types: {types:?}");
        assert_eq!(types[0], "download-to-library");
        assert_eq!(types[1], "download-to-library");
        assert!(!types[2..].contains(&"download-to-library"));

        // downloads 按 skill 首次出现顺序（missing-a 在步骤 1，missing-b 在步骤 2）；
        // ready-skill 已在中心库，不产生 download op
        let first = &plan.operations[0];
        assert_eq!(first.skill_id.as_deref(), Some("missing-a"));
        assert_eq!(first.source_path.as_deref(), Some(GITHUB_SOURCE));
        assert_eq!(first.skill_path.as_deref(), Some("skills/cat/missing-a"));
        assert_eq!(first.status, "planned");
        assert_eq!(plan.operations[1].skill_id.as_deref(), Some("missing-b"));
        assert_eq!(plan.operations[1].skill_path, None);

        // 同步段：每个 skill 一组 create-root + copy-to-target（root 尚不存在）
        assert_eq!(
            types[2..8],
            [
                "create-root",
                "copy-to-target",
                "create-root",
                "copy-to-target",
                "create-root",
                "copy-to-target"
            ]
        );

        // output op：入口清单写入 target 根的 _workflow-<slug>/
        let output = plan.operations.last().expect("output op");
        assert_eq!(output.op_type, "copy-to-target");
        let output_target = output.target_path.as_deref().expect("output target");
        assert!(
            output_target.ends_with(&PathBuf::from("_workflow-mixed-flow").to_string_lossy().to_string()),
            "output target: {output_target}"
        );
        // 产物在 preview 时已生成到暂存区
        let staging_source = PathBuf::from(output.source_path.as_deref().expect("source"));
        assert!(staging_source.join(WORKFLOW_FILE).is_file());
        assert!(staging_source.join("README.md").is_file());

        // 占位步骤不进 op，但进 preconditions warning
        assert!(
            plan.preconditions
                .iter()
                .any(|item| item.contains("占位") && item.contains("待指定修复类 skill")),
            "preconditions: {:?}",
            plan.preconditions
        );
        assert!(
            plan.preconditions
                .iter()
                .any(|item| item.contains("下载 missing-a"))
        );
    }

    #[test]
    fn packaged_form_plans_downloads_then_output_without_standalone_sync() {
        // 容忍 poison：一个 env 测试 panic 不应让其余 env 测试连锁失败
        let _lock = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        let stub_bin = temp.path().join("bin");
        write_claude_stub(&stub_bin);
        let _env = EnvGuard::install(ctx.home_dir(), &stub_bin);

        write_skill(&library_root(&ctx), "ready-skill");
        let workflow = mixed_workflow(GITHUB_SOURCE, GITHUB_SOURCE);
        let yaml = workflow.to_yaml().expect("yaml");

        let plan = preview_use_for(
            &ctx,
            &workflow,
            &yaml,
            claude_target(),
            "copy",
            OutputForm::PackagedSkill,
        )
        .expect("preview");

        assert!(
            plan.blocked_conflicts.is_empty(),
            "blocked: {:?}",
            plan.blocked_conflicts
        );

        let types = op_types(&plan);
        // 操作序列：downloads 在前 → 打包 output ops；无标准同步段
        // （无 create-root / 无指向独立 skill 的 copy-to-target）
        assert_eq!(types.len(), 6, "types: {types:?}");
        assert_eq!(types[0], "download-to-library");
        assert_eq!(types[1], "download-to-library");
        assert!(!types.contains(&"create-root"));
        assert_eq!(types[2..], ["copy-to-target", "copy-to-target", "copy-to-target", "copy-to-target"]);

        // 反向断言（防回归）：不存在指向各 agent skills 根直下独立 skill 的
        // copy-to-target；打包形态的全部 copy 都必须落在 <root>/mixed-flow 内
        let root = ctx.home_dir().join(".claude").join("skills");
        for slug in ["ready-skill", "missing-a", "missing-b"] {
            let standalone = root.join(slug);
            assert!(
                !plan.operations.iter().any(|operation| {
                    operation.op_type == "copy-to-target"
                        && operation.target_path.as_deref().map(PathBuf::from) == Some(standalone.clone())
                }),
                "standalone sync op for '{slug}' must not exist in packaged form"
            );
        }
        for operation in &plan.operations[2..] {
            let target = operation.target_path.as_deref().expect("output target");
            assert!(
                target.contains("mixed-flow"),
                "packaged output must stay inside the package dir: {target}"
            );
        }

        // 骨架 op 在前、各 skill 打包拷贝在后；打包内容来源为中心库
        let skeleton = &plan.operations[2];
        assert_eq!(skeleton.skill_id.as_deref(), Some("mixed-flow"));
        let library = library_root(&ctx);
        let last = plan.operations.last().expect("last op");
        assert_eq!(
            last.source_path.as_deref(),
            Some(path_to_string(&library.join("missing-b")).as_str())
        );
        assert!(
            last.target_path
                .as_deref()
                .expect("target")
                .contains(&path_to_string(&PathBuf::from("mixed-flow").join("skills").join("missing-b")))
        );

        // 占位步骤不进 op，但进 preconditions warning
        assert!(
            plan.preconditions
                .iter()
                .any(|item| item.contains("占位") && item.contains("待指定修复类 skill")),
            "preconditions: {:?}",
            plan.preconditions
        );
    }

    #[test]
    fn duplicate_slug_with_different_source_is_blocked() {
        // 容忍 poison：一个 env 测试 panic 不应让其余 env 测试连锁失败
        let _lock = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        let stub_bin = temp.path().join("bin");
        write_claude_stub(&stub_bin);
        let _env = EnvGuard::install(ctx.home_dir(), &stub_bin);

        let mut workflow = mixed_workflow(GITHUB_SOURCE, GITHUB_SOURCE);
        workflow.steps[1].skills.push(skill_ref(
            "ready-skill",
            "https://github.com/another/repo.git",
            None,
        ));
        let yaml = workflow.to_yaml().expect("yaml");

        let plan = preview_use_for(
            &ctx,
            &workflow,
            &yaml,
            claude_target(),
            "copy",
            OutputForm::EntryManifest,
        )
        .expect("preview");

        assert!(
            plan.blocked_conflicts
                .iter()
                .any(|item| item.contains("ready-skill") && item.contains("不同来源")),
            "blocked: {:?}",
            plan.blocked_conflicts
        );
        assert_eq!(plan.risk_level, "blocked");
    }

    #[test]
    fn packaged_form_allows_workflow_slug_matching_skill_slug() {
        // 容忍 poison：一个 env 测试 panic 不应让其余 env 测试连锁失败
        let _lock = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        let stub_bin = temp.path().join("bin");
        write_claude_stub(&stub_bin);
        let _env = EnvGuard::install(ctx.home_dir(), &stub_bin);

        // 打包形态跳过独立同步后，workflow slug 与 skill slug 同名不再撞目录：
        // 包写入 <root>/<wf-slug>/，同名 skill 内容进包内 skills/<skill-slug>/
        write_skill(&library_root(&ctx), "ready-skill");
        let mut workflow = mixed_workflow(GITHUB_SOURCE, GITHUB_SOURCE);
        workflow.slug = "ready-skill".to_string();
        let yaml = workflow.to_yaml().expect("yaml");

        let plan = preview_use_for(
            &ctx,
            &workflow,
            &yaml,
            claude_target(),
            "copy",
            OutputForm::PackagedSkill,
        )
        .expect("preview");

        assert!(
            plan.blocked_conflicts.is_empty(),
            "blocked: {:?}",
            plan.blocked_conflicts
        );
        let root = ctx.home_dir().join(".claude").join("skills");
        let packaged_target = root.join("ready-skill");
        let nested_skill = root.join("ready-skill").join("skills").join("ready-skill");
        // PathBuf 组件级比较：agent 根经 expand_home 解析，Windows 上字符串
        // 分隔符可能与 join 拼接的表示不同
        assert!(plan.operations.iter().any(|operation| {
            operation.target_path.as_deref().map(PathBuf::from) == Some(packaged_target.clone())
        }));
        assert!(plan.operations.iter().any(|operation| {
            operation.target_path.as_deref().map(PathBuf::from) == Some(nested_skill.clone())
        }));
    }

    #[test]
    fn lock_record_with_different_source_blocks_ready_skill() {
        // 容忍 poison：一个 env 测试 panic 不应让其余 env 测试连锁失败
        let _lock = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        let stub_bin = temp.path().join("bin");
        write_claude_stub(&stub_bin);
        let _env = EnvGuard::install(ctx.home_dir(), &stub_bin);

        // 中心库已有 ready-skill，但 skill.lock 记录的来源与工作流引用不同
        write_skill(&library_root(&ctx), "ready-skill");
        let lock_dir = ctx.home_dir().join(".agents");
        ensure_dir(&lock_dir).expect("lock dir");
        fs::write(
            lock_dir.join(".skill-lock.json"),
            "{\"skills\":{\"ready-skill\":{\"sourceUrl\":\"https://github.com/other/skills.git\"}}}",
        )
        .expect("lock file");

        let workflow = mixed_workflow(GITHUB_SOURCE, GITHUB_SOURCE);
        let yaml = workflow.to_yaml().expect("yaml");
        let plan = preview_use_for(
            &ctx,
            &workflow,
            &yaml,
            claude_target(),
            "copy",
            OutputForm::EntryManifest,
        )
        .expect("preview");

        assert!(
            plan.blocked_conflicts
                .iter()
                .any(|item| item.contains("ready-skill") && item.contains("来源")),
            "blocked: {:?}",
            plan.blocked_conflicts
        );
    }

    // ---- 端到端：preview → apply -------------------------------------------

    fn git(repo: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(repo)
            .status()
            .expect("git must run");
        assert!(status.success(), "git {args:?} failed with {status}");
    }

    /// 本地 git 仓库当下载来源：missing-a 走自定义 skillPath（skills/cat/…），
    /// missing-b 走内建候选（skills/<slug>）。
    fn fixture_skill_repo(temp: &tempfile::TempDir) -> PathBuf {
        let repo = temp.path().join("repo");
        ensure_dir(&repo).expect("repo dir");
        git(&repo, &["init"]);

        let missing_a = repo.join("skills").join("cat").join("missing-a");
        ensure_dir(&missing_a.join("references")).expect("missing-a dirs");
        fs::write(
            missing_a.join("SKILL.md"),
            "---\nname: missing-a\ndescription: Missing A\n---\nBody A\n",
        )
        .expect("missing-a SKILL.md");
        fs::write(missing_a.join("references").join("deep.md"), "deep ref\n")
            .expect("missing-a reference");

        let missing_b = repo.join("skills").join("missing-b");
        ensure_dir(&missing_b).expect("missing-b dir");
        fs::write(
            missing_b.join("SKILL.md"),
            "---\nname: missing-b\ndescription: Missing B\n---\nBody B\n",
        )
        .expect("missing-b SKILL.md");

        git(&repo, &["add", "-A"]);
        git(
            &repo,
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=test",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                "fixture",
            ],
        );
        repo
    }

    fn collect_files(dir: &Path) -> Vec<String> {
        let mut files = Vec::new();
        for entry in walkdir::WalkDir::new(dir).sort_by_file_name() {
            let entry = entry.expect("walk entry");
            if entry.file_type().is_file() {
                files.push(
                    entry
                        .path()
                        .strip_prefix(dir)
                        .expect("strip prefix")
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
        files
    }

    /// 递归 diff：相对文件清单一致且逐文件字节一致。
    fn assert_dirs_equal(expected: &Path, actual: &Path) {
        let expected_files = collect_files(expected);
        let actual_files = collect_files(actual);
        assert_eq!(
            expected_files, actual_files,
            "file lists differ ({expected:?} vs {actual:?})"
        );
        for relative in &expected_files {
            let left = fs::read(expected.join(relative)).expect("read expected");
            let right = fs::read(actual.join(relative)).expect("read actual");
            assert_eq!(left, right, "file {relative} differs");
        }
    }

    #[test]
    fn entry_manifest_end_to_end_applies_to_target_root() {
        // 容忍 poison：一个 env 测试 panic 不应让其余 env 测试连锁失败
        let _lock = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        let stub_bin = temp.path().join("bin");
        write_claude_stub(&stub_bin);
        let _env = EnvGuard::install(ctx.home_dir(), &stub_bin);

        let repo = fixture_skill_repo(&temp);
        let repo_url = path_to_string(&repo);
        write_skill(&library_root(&ctx), "ready-skill");

        let workflow = mixed_workflow(&repo_url, &repo_url);
        let yaml = workflow.to_yaml().expect("yaml");
        let plan = preview_use_for(
            &ctx,
            &workflow,
            &yaml,
            claude_target(),
            "copy",
            OutputForm::EntryManifest,
        )
        .expect("preview");
        assert!(plan.blocked_conflicts.is_empty());

        let result = crate::sync_plan::apply_plan(&ctx, plan.plan_id.clone()).expect("apply");
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

        // 缺失 skill 已由 download-to-library 补齐进中心库
        let library = library_root(&ctx);
        assert!(library.join("missing-a").join("SKILL.md").is_file());
        assert!(library.join("missing-b").join("SKILL.md").is_file());

        let root = ctx.home_dir().join(".claude").join("skills");
        // 独立 skill 同步（copy 形态）
        assert!(root.join("ready-skill").join("SKILL.md").is_file());
        assert!(root.join("missing-a").join("SKILL.md").is_file());

        // 入口清单：workflow.yaml 原样拷贝 + README 生成（有序列表 + 同级指引）
        let manifest = root.join("_workflow-mixed-flow");
        let copied_yaml = fs::read_to_string(manifest.join(WORKFLOW_FILE)).expect("yaml copy");
        assert_eq!(copied_yaml, yaml);
        let readme = fs::read_to_string(manifest.join("README.md")).expect("readme");
        assert!(readme.contains("1. `ready-skill` — 见 `../ready-skill/SKILL.md`"));
        assert!(readme.contains("2. `missing-a` — 见 `../missing-a/SKILL.md`"));
        assert!(readme.contains("**占位**：待指定修复类 skill"));
    }

    #[test]
    fn packaged_skill_end_to_end_with_local_git_fixture() {
        // 容忍 poison：一个 env 测试 panic 不应让其余 env 测试连锁失败
        let _lock = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        let stub_bin = temp.path().join("bin");
        write_claude_stub(&stub_bin);
        let _env = EnvGuard::install(ctx.home_dir(), &stub_bin);

        let repo = fixture_skill_repo(&temp);
        let repo_url = path_to_string(&repo);
        write_skill(&library_root(&ctx), "ready-skill");

        let workflow = mixed_workflow(&repo_url, &repo_url);
        let yaml = workflow.to_yaml().expect("yaml");
        let plan = preview_use_for(
            &ctx,
            &workflow,
            &yaml,
            claude_target(),
            "copy",
            OutputForm::PackagedSkill,
        )
        .expect("preview");
        assert!(plan.blocked_conflicts.is_empty());

        let result = crate::sync_plan::apply_plan(&ctx, plan.plan_id.clone()).expect("apply");
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

        let library = library_root(&ctx);
        let root = ctx.home_dir().join(".claude").join("skills");
        let packaged = root.join("mixed-flow");

        // 打包 skill：SKILL.md 编排入口 + skills/ 子目录结构化拷贝（递归 diff）
        let skill_md = fs::read_to_string(packaged.join("SKILL.md")).expect("packaged SKILL.md");
        assert!(skill_md.starts_with("---\nname: mixed-flow\ndescription: \""));
        assert!(skill_md.contains("`skills/ready-skill/SKILL.md`"));

        for slug in ["ready-skill", "missing-a", "missing-b"] {
            assert_dirs_equal(
                &library.join(slug),
                &packaged.join("skills").join(slug),
            );
        }
        // 自定义 skillPath 的子目录文件也被完整拷贝
        assert!(
            packaged
                .join("skills")
                .join("missing-a")
                .join("references")
                .join("deep.md")
                .is_file()
        );
        // 打包形态自包含：目标目录只出现打包目录，无独立 skill 副本
        for slug in ["ready-skill", "missing-a", "missing-b"] {
            assert!(
                !root.join(slug).exists(),
                "standalone copy of '{slug}' must not exist in packaged form"
            );
        }
        // 入口清单形态的产物不应出现
        assert!(!root.join("_workflow-mixed-flow").exists());
    }
}
