//! M4 workflow-share：胖包导出/导入（DD §4）。
//!
//! 导出 = origin 现拉（门-F-17 语义）：逐 Ref skill 经
//! `checkout_skill_from_clone_source` 抓取当下内容打进 `skills/<slug>/`，
//! 非本地中心库快照。任一 skill 抓取失败 → 清理临时目录与全部 clone 根，
//! Err 列出全部失败项，不产半成品（Q6）。
//!
//! 导入校验链（R7 / 门-F4，任一不过即 Err，不落半成品）：base64 解码前长度
//! 预检 → ≤ 50MB → zip 可读 → 逐条目路径安检（绝对路径 / `..` / 非 UTF-8 /
//! 反斜杠 / 冒号拒绝）→ 解压合计 ≤ 200MB → 必含 workflow.yaml → parse +
//! `Workflow::validate` → slug 合法且 `workflows/<slug>` 不存在 → 落
//! yaml/README → 包内 source.json 存在则还原 workflow-sources/（contentHash
//! 复核由 check_one 天然完成：不一致按 Modified 自然呈现，不算错误）。
//! 包内 `skills/` 不装入中心库（有意取舍：有应用者使用时按 sourceUrl 现拉；
//! 无应用者手动放 agent skills 目录）。

use crate::context::AppContext;
use crate::fs_ops::{copy_dir_recursive, ensure_dir, path_to_string, remove_entry};
use crate::workflow::{SkillRef, StepSkill, Workflow, WORKFLOW_FILE, WORKFLOW_README, workflows_dir};
use crate::workflow_update::SourceMeta;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

/// 压缩包上限 50MB（R7）。
pub const MAX_ARCHIVE_BYTES: usize = 50 * 1024 * 1024;
/// 解压合计上限 200MB（防炸弹，R7）。
pub const MAX_UNPACKED_BYTES: u64 = 200 * 1024 * 1024;
/// base64 字符串上限（门-F4 解码前预检）：50MB 字节的标准 base64 编码长度。
pub const MAX_ARCHIVE_BASE64_LEN: usize = (MAX_ARCHIVE_BYTES + 2) / 3 * 4;

/// `export_workflow_package` 的返回体（wire 形态 {filename, base64}，DD §8.4）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPackage {
    pub filename: String,
    pub base64: String,
}

/// `import_workflow_package` 的返回体（{slug, hadSource}，DD §4.2）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub slug: String,
    pub had_source: bool,
}

/// manifest.json：包内容清点（DD §4.1）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ShareManifest {
    pub workflow_slug: String,
    pub skills: Vec<ManifestSkill>,
    /// 含占位 skill 的 step 名列表。
    pub placeholders: Vec<String>,
    pub exported_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestSkill {
    pub slug: String,
    pub source_url: String,
    #[serde(default)]
    pub skill_path: Option<String>,
}

// ---------------------------------------------------------------------------
// 导出
// ---------------------------------------------------------------------------

/// 生产入口：load + read_source 后组装。逐 Ref skill 经 normalize（GitHub-only）
/// 再 checkout 现拉；任一失败不产半成品。返回 (filename, zip 字节)。
pub fn export_package(ctx: &AppContext, slug: &str) -> Result<(String, Vec<u8>), String> {
    let workflow = crate::workflow::load(ctx, slug)?;
    let source = crate::workflow_update::read_source(ctx, slug);
    export_assembled(ctx, &workflow, source.as_ref(), |reference| {
        let clone_url = crate::skill_ops::normalize_github_url(&reference.source_url)?;
        crate::skill_ops::checkout_skill_from_clone_source(
            ctx,
            &reference.slug,
            &clone_url,
            reference.skill_path.as_deref(),
        )
    })
}

/// `export_workflow_package` 的核心返回（base64 编码单点，双壳共用）。
pub fn export_package_base64(ctx: &AppContext, slug: &str) -> Result<ExportPackage, String> {
    let (filename, bytes) = export_package(ctx, slug)?;
    Ok(ExportPackage {
        filename,
        base64: STANDARD.encode(bytes),
    })
}

/// 组装执行器（fetch 可注入）。生产 fetch = normalize + checkout；单元测试注入
/// 逐字 clone fetch（与 `checkout_skill_from_clone_source` 文档的测试钩子同一
/// 模式），把本地 fixture git 仓库当 skill 来源，走真实 clone 链路。
pub(crate) fn export_assembled(
    ctx: &AppContext,
    workflow: &Workflow,
    source: Option<&SourceMeta>,
    fetch: impl FnMut(&SkillRef) -> Result<PathBuf, String>,
) -> Result<(String, Vec<u8>), String> {
    // 清理约定（门-F13）：成功与失败两路都清——临时根 + 本次导出在 updates/
    // 新增的 clone 根。checkout 失败分支无返回 PathBuf 可向上回溯，故以开始前
    // 快照差集清扫（只删本次新增，不碰 updates/ 既有内容）。
    let updates_dir = ctx.data_dir().join("updates");
    let updates_before = entry_names(&updates_dir);
    let temp_root = ctx
        .data_dir()
        .join("tmp")
        .join(format!("export-{}", Utc::now().timestamp_millis()));

    let result = assemble_package(ctx, workflow, source, fetch, &temp_root);

    if temp_root.exists() {
        let _ = remove_entry(&temp_root);
    }
    sweep_new_entries(&updates_dir, &updates_before);
    result
}

fn assemble_package(
    ctx: &AppContext,
    workflow: &Workflow,
    source: Option<&SourceMeta>,
    mut fetch: impl FnMut(&SkillRef) -> Result<PathBuf, String>,
    temp_root: &Path,
) -> Result<(String, Vec<u8>), String> {
    // 1. 安装目录逐字节入临时根（yaml/README 保真——导入后 hash 与 source.json
    //    的 contentHash 一致正是靠逐字节链路）。
    copy_dir_recursive(&workflows_dir(ctx).join(&workflow.slug), temp_root)?;

    // 2. manifest.json
    let manifest = build_manifest(workflow);
    let text = serde_json::to_string_pretty(&manifest)
        .map_err(|error| format!("Unable to serialize export manifest: {error}"))?;
    fs::write(temp_root.join("manifest.json"), text).map_err(|error| {
        format!(
            "Unable to write export manifest at {}: {error}",
            path_to_string(&temp_root.join("manifest.json"))
        )
    })?;

    // 3. source.json（可选：无来源快照的本地工作流不带）
    if let Some(source) = source {
        let text = serde_json::to_string_pretty(source)
            .map_err(|error| format!("Unable to serialize source metadata: {error}"))?;
        fs::write(temp_root.join("source.json"), text).map_err(|error| {
            format!(
                "Unable to write source metadata at {}: {error}",
                path_to_string(&temp_root.join("source.json"))
            )
        })?;
    }

    // 4. 逐 Ref skill origin 现拉 → skills/<slug>/。失败收集后继续，Err 列全部
    //    失败项（Q6：不产半成品——临时根由 export_assembled 统一清理）。
    let mut failures: Vec<String> = Vec::new();
    for reference in unique_refs(workflow) {
        // Workflow::validate 不校验 ref.slug，而它要进 zip 内路径，这里补防。
        if !is_valid_skill_slug(&reference.slug) {
            failures.push(format!(
                "{}: invalid skill slug (must be non-empty and match [a-z0-9-]+)",
                reference.slug
            ));
            continue;
        }
        match fetch(reference) {
            Ok(resolved) => {
                let target = temp_root.join("skills").join(&reference.slug);
                if let Err(error) = copy_dir_recursive(&resolved, &target) {
                    failures.push(format!("{}: {error}", reference.slug));
                }
            }
            Err(error) => failures.push(format!("{}: {error}", reference.slug)),
        }
    }
    if !failures.is_empty() {
        return Err(format!(
            "Unable to export workflow '{}': {} skill(s) failed to fetch: {}",
            workflow.slug,
            failures.len(),
            failures.join("; ")
        ));
    }

    // 5. zip 成字节
    let bytes = zip_dir(temp_root)?;
    Ok((format!("{}-workflow.zip", workflow.slug), bytes))
}

/// 去重后的 Ref 列表（同一 skill 被多个 step 引用时只抓一份、manifest 只列一条）。
fn unique_refs(workflow: &Workflow) -> Vec<&SkillRef> {
    let mut seen = BTreeSet::new();
    let mut refs = Vec::new();
    for step in &workflow.steps {
        for skill in &step.skills {
            let StepSkill::Ref(reference) = skill else {
                continue;
            };
            let key = (
                reference.slug.clone(),
                reference.source_url.clone(),
                reference.skill_path.clone(),
            );
            if seen.insert(key) {
                refs.push(reference);
            }
        }
    }
    refs
}

fn build_manifest(workflow: &Workflow) -> ShareManifest {
    let skills = unique_refs(workflow)
        .into_iter()
        .map(|reference| ManifestSkill {
            slug: reference.slug.clone(),
            source_url: reference.source_url.clone(),
            skill_path: reference.skill_path.clone(),
        })
        .collect();
    let placeholders = workflow
        .steps
        .iter()
        .filter(|step| {
            step.skills
                .iter()
                .any(|skill| matches!(skill, StepSkill::Placeholder { .. }))
        })
        .map(|step| step.name.clone())
        .collect();
    ShareManifest {
        workflow_slug: workflow.slug.clone(),
        skills,
        placeholders,
        exported_at: Utc::now().to_rfc3339(),
    }
}

fn zip_dir(root: &Path) -> Result<Vec<u8>, String> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for entry in WalkDir::new(root).follow_links(false).sort_by_file_name() {
        let entry =
            entry.map_err(|error| format!("Unable to walk {}: {error}", path_to_string(root)))?;
        let rel = entry.path().strip_prefix(root).map_err(|error| {
            format!("Unable to pack {}: {error}", path_to_string(entry.path()))
        })?;
        if rel.as_os_str().is_empty() {
            continue;
        }
        let name = forward_slash_path(rel);
        if entry.file_type().is_dir() {
            writer
                .add_directory(format!("{name}/"), options)
                .map_err(|error| format!("Unable to add directory '{name}' to archive: {error}"))?;
        } else if entry.file_type().is_file() {
            writer
                .start_file(&name, options)
                .map_err(|error| format!("Unable to add file '{name}' to archive: {error}"))?;
            let bytes = fs::read(entry.path()).map_err(|error| {
                format!("Unable to read {}: {error}", path_to_string(entry.path()))
            })?;
            writer
                .write_all(&bytes)
                .map_err(|error| format!("Unable to pack '{name}': {error}"))?;
        } else {
            return Err(format!(
                "Export package does not support non-regular entries (e.g. symlink): {}",
                path_to_string(entry.path())
            ));
        }
    }
    let cursor = writer
        .finish()
        .map_err(|error| format!("Unable to finish archive: {error}"))?;
    Ok(cursor.into_inner())
}

// ---------------------------------------------------------------------------
// 导入
// ---------------------------------------------------------------------------

/// base64 预检 + 解码（门-F4：解码**前**先查字符串长度上限；双壳共用单点）。
pub fn decode_archive_base64(archive_base64: &str) -> Result<Vec<u8>, String> {
    if archive_base64.len() > MAX_ARCHIVE_BASE64_LEN {
        return Err(format!(
            "Archive base64 is too long: {} chars (limit {MAX_ARCHIVE_BASE64_LEN}, ≈ 50MB archive)",
            archive_base64.len()
        ));
    }
    STANDARD
        .decode(archive_base64)
        .map_err(|error| format!("Archive is not valid base64: {error}"))
}

/// `import_workflow_package` 的核心入口（base64 形态，双壳共用）。
pub fn import_package_base64(
    ctx: &AppContext,
    archive_base64: &str,
) -> Result<ImportResult, String> {
    let bytes = decode_archive_base64(archive_base64)?;
    import_package(ctx, &bytes)
}

/// 校验链（DD §4.2，顺序即防线顺序）：≤ 50MB → zip 可读 → 逐条目路径安检 +
/// 解压合计 ≤ 200MB → 必含 workflow.yaml → parse + validate → slug 不冲突 →
/// 落 yaml/README → source.json 还原。任一不过即 Err，不落半成品。
pub fn import_package(ctx: &AppContext, bytes: &[u8]) -> Result<ImportResult, String> {
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(format!(
            "Archive exceeds the 50MB limit: {} bytes",
            bytes.len()
        ));
    }
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| format!("Archive is not a readable zip: {error}"))?;

    // 逐条目：路径安检 + 声明尺寸合计（不解压即先拦的防炸弹层）。
    let mut declared_total: u64 = 0;
    let mut has_workflow_yaml = false;
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|error| format!("Unable to read archive entry {index}: {error}"))?;
        check_entry_name(file.name_raw())?;
        declared_total = declared_total.saturating_add(file.size());
        if declared_total > MAX_UNPACKED_BYTES {
            return Err(
                "Archive unpacks to more than the 200MB limit (declared sizes)".to_string(),
            );
        }
        if file.name_raw() == WORKFLOW_FILE.as_bytes() {
            has_workflow_yaml = true;
        }
    }
    if !has_workflow_yaml {
        return Err("Archive is missing workflow.yaml".to_string());
    }

    // 实际只消费三件套，读取走预算制 take（声明尺寸撒谎时的第二道防炸弹层）。
    let mut budget = MAX_UNPACKED_BYTES;
    let yaml_bytes = read_entry(&mut archive, WORKFLOW_FILE, &mut budget)?;
    let yaml_text = std::str::from_utf8(&yaml_bytes)
        .map_err(|_| "Packaged workflow.yaml is not valid UTF-8".to_string())?;
    let workflow = Workflow::from_yaml(yaml_text)?;
    if let Err(errors) = workflow.validate() {
        return Err(format!(
            "Packaged workflow failed validation: {}",
            errors.join("; ")
        ));
    }

    let target = workflows_dir(ctx).join(&workflow.slug);
    if target.exists() {
        return Err(format!(
            "Workflow '{}' already exists; remove it before importing",
            workflow.slug
        ));
    }

    let readme = read_optional_entry(&mut archive, WORKFLOW_README, &mut budget)?;
    let source_json = match read_optional_entry(&mut archive, "source.json", &mut budget)? {
        Some(raw) => {
            let text = std::str::from_utf8(&raw)
                .map_err(|_| "Packaged source.json is not valid UTF-8".to_string())?;
            serde_json::from_str::<SourceMeta>(text)
                .map_err(|error| format!("Packaged source.json is invalid: {error}"))?;
            Some(raw)
        }
        None => None,
    };
    let had_source = source_json.is_some();

    // 落盘：全部校验通过后才写第一字节；写入中途失败整体回收（不落半成品）。
    let source_file = ctx
        .data_dir()
        .join("workflow-sources")
        .join(format!("{}.json", workflow.slug));
    let install = (|| -> Result<(), String> {
        ensure_dir(&target)?;
        fs::write(target.join(WORKFLOW_FILE), &yaml_bytes).map_err(|error| {
            format!(
                "Unable to write workflow at {}: {error}",
                path_to_string(&target.join(WORKFLOW_FILE))
            )
        })?;
        if let Some(readme) = &readme {
            fs::write(target.join(WORKFLOW_README), readme).map_err(|error| {
                format!(
                    "Unable to write workflow README at {}: {error}",
                    path_to_string(&target.join(WORKFLOW_README))
                )
            })?;
        }
        if let Some(raw) = &source_json {
            if let Some(parent) = source_file.parent() {
                ensure_dir(parent)?;
            }
            fs::write(&source_file, raw).map_err(|error| {
                format!(
                    "Unable to restore source metadata at {}: {error}",
                    path_to_string(&source_file)
                )
            })?;
        }
        Ok(())
    })();
    if let Err(error) = install {
        let _ = remove_entry(&target);
        let _ = fs::remove_file(&source_file);
        return Err(error);
    }

    Ok(ImportResult {
        slug: workflow.slug,
        had_source,
    })
}

/// 桌面专用（不注册 web，R4/D7）：把 `export_workflow_package` 响应的 base64
/// 落用户选定路径。不走导入的 50MB 预检——导出包不受导入上限约束。
pub fn save_export_to_path(path: &str, archive_base64: &str) -> Result<(), String> {
    let bytes = STANDARD
        .decode(archive_base64)
        .map_err(|error| format!("Archive is not valid base64: {error}"))?;
    let target = crate::fs_ops::expand_home(path);
    fs::write(&target, &bytes).map_err(|error| {
        format!(
            "Unable to write export to {}: {error}",
            path_to_string(&target)
        )
    })
}

/// 条目路径安检（R7）：非 UTF-8 / 空名 / 反斜杠 / 冒号（Windows 盘符）/
/// 非 Normal 组件（绝对路径、`..`、`.`）一律拒绝。
fn check_entry_name(raw: &[u8]) -> Result<(), String> {
    let name = std::str::from_utf8(raw)
        .map_err(|_| "Archive entry name is not valid UTF-8".to_string())?;
    if name.is_empty() {
        return Err("Archive entry name is empty".to_string());
    }
    if name.contains('\\') || name.contains(':') {
        return Err(format!("Archive entry '{name}' contains an unsafe character"));
    }
    let safe = Path::new(name)
        .components()
        .all(|component| matches!(component, Component::Normal(_)));
    if !safe {
        return Err(format!("Archive entry '{name}' has an unsafe path"));
    }
    Ok(())
}

/// 预算制读取：实际解压合计不得突破 200MB（声明尺寸撒谎时的兜底）。
fn read_entry(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
    budget: &mut u64,
) -> Result<Vec<u8>, String> {
    let file = archive
        .by_name(name)
        .map_err(|error| format!("Unable to read '{name}' from archive: {error}"))?;
    let mut data = Vec::new();
    file.take(budget.saturating_add(1))
        .read_to_end(&mut data)
        .map_err(|error| format!("Unable to unpack '{name}': {error}"))?;
    if data.len() as u64 > *budget {
        return Err("Archive unpacks to more than the 200MB limit".to_string());
    }
    *budget -= data.len() as u64;
    Ok(data)
}

fn read_optional_entry(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
    budget: &mut u64,
) -> Result<Option<Vec<u8>>, String> {
    match archive.by_name(name) {
        Ok(file) => {
            let mut data = Vec::new();
            file.take(budget.saturating_add(1))
                .read_to_end(&mut data)
                .map_err(|error| format!("Unable to unpack '{name}': {error}"))?;
            if data.len() as u64 > *budget {
                return Err("Archive unpacks to more than the 200MB limit".to_string());
            }
            *budget -= data.len() as u64;
            Ok(Some(data))
        }
        Err(zip::result::ZipError::FileNotFound) => Ok(None),
        Err(error) => Err(format!("Unable to read '{name}' from archive: {error}")),
    }
}

// ---------------------------------------------------------------------------
// 共享小工具
// ---------------------------------------------------------------------------

/// 与 workflow_update::is_valid_slug 同规则（[a-z0-9-]+，模块内同规则拷贝，
/// DD §7 先例）。用于 Ref skill slug——它要进 zip 内路径而 validate 不校验。
fn is_valid_skill_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// 与 fs_ops::hash_relative_path 同规则的相对路径正斜杠化（原函数私有）。
fn forward_slash_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn entry_names(dir: &Path) -> BTreeSet<OsString> {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name())
                .collect()
        })
        .unwrap_or_default()
}

/// 清扫 dir 下不在 before 快照里的条目（本次操作新增的残留），尽力而为。
fn sweep_new_entries(dir: &Path, before: &BTreeSet<OsString>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        if !before.contains(&entry.file_name()) {
            let _ = remove_entry(&entry.path());
        }
    }
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

/// 测试共享 fixture（本模块单测与 web 端点 oneshot 复用）：本地 fixture skill
/// git 仓库 + fixture 工作流 → 真 clone → 真导出 → 真导入。导出 fetch 注入
/// 逐字 clone（`checkout_skill_from_clone_source` 的文档测试钩子），fixture
/// 工作流 yaml 的 sourceUrl 保持 GitHub 形态以过 `Workflow::validate`——导出
/// 字节链路（yaml/README/manifest/skills 拷贝/zip）与导入校验链全是生产代码。
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::workflow_update::test_support::{commit_fixture, git};

    pub(crate) const SHARE_REGISTRY_URL: &str = "https://github.com/fixture/workflows.git";
    pub(crate) const SHARE_README: &str = "# 分享流程 README\n";
    /// 两个 step 引用同一 skill（覆盖去重），第二个 step 带占位（覆盖 manifest
    /// placeholders）。
    pub(crate) const SHARE_YAML: &str = "name: 分享流程\n\
         slug: share-flow\n\
         version: 0.1.0\n\
         description: 分享回测\n\
         groups:\n  - id: doing\n    name: 执行\n  - id: review\n    name: 评审\n\
         steps:\n  - name: 步骤一\n    group: doing\n    skills:\n\
         \x20     - sourceType: github\n\
         \x20       sourceUrl: https://github.com/fixture/skills.git\n\
         \x20       slug: grill-me\n\
         \x20       skillPath: skills/productivity/grill-me\n\
         \x20 - name: 步骤二\n    group: review\n    skills:\n\
         \x20     - sourceType: github\n\
         \x20       sourceUrl: https://github.com/fixture/skills.git\n\
         \x20       slug: grill-me\n\
         \x20       skillPath: skills/productivity/grill-me\n\
         \x20     - placeholder: 待补充修复 skill\n";
    /// 一个正常 ref + 两个必然抓取失败的 ref（skillPath 不存在）。
    pub(crate) const FAIL_YAML: &str = "name: 失败流程\n\
         slug: fail-flow\n\
         version: 0.1.0\n\
         description: 失败回测\n\
         groups:\n  - id: doing\n    name: 执行\n\
         steps:\n  - name: 步骤一\n    group: doing\n    skills:\n\
         \x20     - sourceType: github\n\
         \x20       sourceUrl: https://github.com/fixture/skills.git\n\
         \x20       slug: grill-me\n\
         \x20       skillPath: skills/productivity/grill-me\n\
         \x20     - sourceType: github\n\
         \x20       sourceUrl: https://github.com/fixture/skills.git\n\
         \x20       slug: missing-a\n\
         \x20       skillPath: skills/missing/a\n\
         \x20     - sourceType: github\n\
         \x20       sourceUrl: https://github.com/fixture/skills.git\n\
         \x20       slug: missing-b\n\
         \x20       skillPath: skills/missing/b\n";
    pub(crate) const GRILL_ME_SKILL_MD: &str =
        "---\nname: grill-me\ndescription: fixture skill\n---\n\nfixture body\n";
    pub(crate) const GRILL_ME_HELPER: &str = "helper fixture\n";

    pub(crate) fn test_ctx(temp: &tempfile::TempDir) -> AppContext {
        AppContext::new(temp.path().join("data"), temp.path().join("home"))
    }

    /// 本地 fixture skill 源仓库（grill-me 已提交）。TempDir 须调用方持有存活。
    pub(crate) fn fixture_skill_repo() -> tempfile::TempDir {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = temp.path().join("repo");
        fs::create_dir_all(repo.join("skills/productivity/grill-me/scripts")).expect("skill dir");
        git(&repo, &["init"]);
        fs::write(
            repo.join("skills/productivity/grill-me/SKILL.md"),
            GRILL_ME_SKILL_MD,
        )
        .expect("SKILL.md");
        fs::write(
            repo.join("skills/productivity/grill-me/scripts/helper.txt"),
            GRILL_ME_HELPER,
        )
        .expect("helper");
        commit_fixture(&repo, "skill fixture");
        temp
    }

    pub(crate) fn repo_source(fixture: &tempfile::TempDir) -> String {
        path_to_string(&fixture.path().join("repo"))
    }

    /// 安装 share-flow：落 yaml/README + record_source（真实来源快照）。
    pub(crate) fn install_share_workflow(ctx: &AppContext) {
        let dir = workflows_dir(ctx).join("share-flow");
        ensure_dir(&dir).expect("install dir");
        fs::write(dir.join(WORKFLOW_FILE), SHARE_YAML).expect("yaml");
        fs::write(dir.join(WORKFLOW_README), SHARE_README).expect("readme");
        crate::workflow_update::record_source(ctx, "share-flow", SHARE_REGISTRY_URL, "share-flow")
            .expect("record source");
    }

    pub(crate) fn install_workflow_yaml(ctx: &AppContext, slug: &str, yaml: &str) {
        let dir = workflows_dir(ctx).join(slug);
        ensure_dir(&dir).expect("install dir");
        fs::write(dir.join(WORKFLOW_FILE), yaml).expect("yaml");
    }

    /// 真导出：逐字 clone fetch 走真实 checkout 链路（clone → resolve → 拷贝）。
    pub(crate) fn export_with_verbatim_fetch(
        ctx: &AppContext,
        slug: &str,
        repo_source: &str,
    ) -> Result<(String, Vec<u8>), String> {
        let workflow = crate::workflow::load(ctx, slug)?;
        let source = crate::workflow_update::read_source(ctx, slug);
        export_assembled(ctx, &workflow, source.as_ref(), |reference| {
            crate::skill_ops::checkout_skill_from_clone_source(
                ctx,
                &reference.slug,
                repo_source,
                reference.skill_path.as_deref(),
            )
        })
    }

    /// 断言用：data_dir 下 updates/ 与 tmp/ 无残留（目录不存在或为空）。
    pub(crate) fn assert_no_residue(ctx: &AppContext) {
        for name in ["updates", "tmp"] {
            let dir = ctx.data_dir().join(name);
            if !dir.exists() {
                continue;
            }
            let remaining: Vec<_> = fs::read_dir(&dir)
                .expect("read dir")
                .filter_map(Result::ok)
                .collect();
            assert!(remaining.is_empty(), "{name}/ 残留: {remaining:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow_update::SourceMeta;
    use test_support::*;

    // -- 导出正例：真导出 → 字节/清单/无残留 → 真导入 ---------------------------

    #[test]
    fn export_then_import_roundtrip() {
        let fixture = fixture_skill_repo();
        let repo_source = repo_source(&fixture);
        let export_temp = tempfile::tempdir().expect("temp dir");
        let ctx1 = test_ctx(&export_temp);
        install_share_workflow(&ctx1);
        let installed_dir = workflows_dir(&ctx1).join("share-flow");

        // 真导出（真实 clone 链路）。
        let (filename, bytes) =
            export_with_verbatim_fetch(&ctx1, "share-flow", &repo_source).expect("export");
        assert_eq!(filename, "share-flow-workflow.zip");
        assert_no_residue(&ctx1);

        // zip 内容：yaml/README 逐字节、source.json、manifest、skills/<slug>/ 全量。
        let mut archive = ZipArchive::new(Cursor::new(bytes.as_slice())).expect("open zip");
        let mut yaml_in_zip = Vec::new();
        archive
            .by_name(WORKFLOW_FILE)
            .expect("yaml entry")
            .read_to_end(&mut yaml_in_zip)
            .expect("read yaml");
        assert_eq!(yaml_in_zip, SHARE_YAML.as_bytes());
        let mut readme_in_zip = Vec::new();
        archive
            .by_name(WORKFLOW_README)
            .expect("readme entry")
            .read_to_end(&mut readme_in_zip)
            .expect("read readme");
        assert_eq!(readme_in_zip, SHARE_README.as_bytes());

        let mut skill_md = Vec::new();
        archive
            .by_name("skills/grill-me/SKILL.md")
            .expect("skill entry")
            .read_to_end(&mut skill_md)
            .expect("read skill");
        assert_eq!(skill_md, GRILL_ME_SKILL_MD.as_bytes());
        let mut helper = Vec::new();
        archive
            .by_name("skills/grill-me/scripts/helper.txt")
            .expect("helper entry")
            .read_to_end(&mut helper)
            .expect("read helper");
        assert_eq!(helper, GRILL_ME_HELPER.as_bytes());

        let mut source_raw = Vec::new();
        archive
            .by_name("source.json")
            .expect("source entry")
            .read_to_end(&mut source_raw)
            .expect("read source");
        let packaged_source: SourceMeta =
            serde_json::from_slice(&source_raw).expect("parse source");
        assert_eq!(packaged_source.registry_url, SHARE_REGISTRY_URL);
        assert_eq!(packaged_source.path, "share-flow");

        let mut manifest_raw = Vec::new();
        archive
            .by_name("manifest.json")
            .expect("manifest entry")
            .read_to_end(&mut manifest_raw)
            .expect("read manifest");
        let manifest: ShareManifest =
            serde_json::from_slice(&manifest_raw).expect("parse manifest");
        assert_eq!(manifest.workflow_slug, "share-flow");
        assert_eq!(manifest.skills.len(), 1, "重复引用应去重");
        assert_eq!(manifest.skills[0].slug, "grill-me");
        assert_eq!(
            manifest.skills[0].source_url,
            "https://github.com/fixture/skills.git"
        );
        assert_eq!(
            manifest.skills[0].skill_path.as_deref(),
            Some("skills/productivity/grill-me")
        );
        assert_eq!(manifest.placeholders, vec!["步骤二".to_string()]);
        assert!(!manifest.exported_at.is_empty());

        // base64 往返字节一致（decode_archive_base64 过完整预检链）。
        let encoded = STANDARD.encode(&bytes);
        let decoded = decode_archive_base64(&encoded).expect("decode roundtrip");
        assert_eq!(decoded, bytes);

        // 真导入到干净 data_dir。
        let import_temp = tempfile::tempdir().expect("temp dir");
        let ctx2 = test_ctx(&import_temp);
        let result = import_package(&ctx2, &bytes).expect("import");
        assert_eq!(
            result,
            ImportResult {
                slug: "share-flow".to_string(),
                had_source: true
            }
        );

        // workflow 可读、逐字节一致、source.json 还原且 contentHash 与安装目录一致。
        let loaded = crate::workflow::load(&ctx2, "share-flow").expect("load imported");
        assert_eq!(loaded.slug, "share-flow");
        assert_eq!(loaded.steps.len(), 2);
        let imported_dir = workflows_dir(&ctx2).join("share-flow");
        assert_eq!(
            fs::read(imported_dir.join(WORKFLOW_FILE)).expect("imported yaml"),
            fs::read(installed_dir.join(WORKFLOW_FILE)).expect("origin yaml")
        );
        assert_eq!(
            fs::read(imported_dir.join(WORKFLOW_README)).expect("imported readme"),
            SHARE_README.as_bytes()
        );
        let restored = crate::workflow_update::read_source(&ctx2, "share-flow").expect("source");
        assert_eq!(restored.registry_url, SHARE_REGISTRY_URL);
        assert_eq!(restored.path, "share-flow");
        assert_eq!(
            restored.content_hash,
            crate::fs_ops::hash_dir(&imported_dir).expect("hash"),
            "导入后 hash 应与还原的 contentHash 一致（逐字节链路）"
        );
        // 包内 skills/ 不装入中心库（有意取舍）。
        assert!(!ctx2.home_dir().join(".oh-my-skills").exists() || {
            !ctx2
                .home_dir()
                .join(".oh-my-skills")
                .join("skills")
                .join("grill-me")
                .exists()
        });

        // 已存在冲突（负例第 10 条）。
        let error = import_package(&ctx2, &bytes).expect_err("reimport must fail");
        assert!(error.contains("already exists"), "error: {error}");
    }

    // -- 导出失败：任一 skill 失败不产半成品，Err 列全部失败项 --------------------

    #[test]
    fn export_failure_produces_no_partial_package() {
        let fixture = fixture_skill_repo();
        let repo_source = repo_source(&fixture);
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        install_workflow_yaml(&ctx, "fail-flow", FAIL_YAML);

        let error = export_with_verbatim_fetch(&ctx, "fail-flow", &repo_source)
            .expect_err("export must fail");
        // 两个失败项都列出；成功的 grill-me 不出现在失败清单。
        assert!(error.contains("missing-a"), "error: {error}");
        assert!(error.contains("missing-b"), "error: {error}");
        assert!(error.contains("2 skill(s) failed"), "error: {error}");
        assert!(!error.contains("grill-me:"), "error: {error}");

        // 不产半成品：updates/ 与 tmp/ 无残留（含失败 checkout 留下的 clone 根）。
        assert_no_residue(&ctx);
    }

    #[test]
    fn export_rejects_missing_or_bad_slug() {
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        let error = export_package(&ctx, "not-installed").expect_err("missing must fail");
        assert!(error.contains("not installed"), "error: {error}");
        for slug in ["..", "../settings", "a/b", "", "UPPER"] {
            let error = export_package(&ctx, slug).expect_err("bad slug must fail");
            assert!(error.contains("Invalid workflow slug"), "slug '{slug}': {error}");
        }
    }

    // -- 导入负例 10 条（逐条拒绝）----------------------------------------------

    /// 造包助手：Stored（不压缩），条目名/内容原样写入。
    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (name, content) in entries {
            writer.start_file(*name, options).expect("start file");
            writer.write_all(content).expect("write");
        }
        writer.finish().expect("finish").into_inner()
    }

    const GOOD_YAML: &str = "name: 小包\nslug: mini-flow\nversion: 0.1.0\ndescription: 负例\n";

    #[test]
    fn rejects_overlong_base64_before_decoding() {
        // 门-F4：解码前长度预检。超限 +1 → 长度拒绝（而非 50MB 字节拒绝）。
        let overlong = "A".repeat(MAX_ARCHIVE_BASE64_LEN + 1);
        let error = decode_archive_base64(&overlong).expect_err("overlong must fail");
        assert!(error.contains("too long"), "error: {error}");

        // 恰好上限的合法 base64 通过预检；解码后字节超 50MB 由 import 字节上限拦。
        let at_limit = "A".repeat(MAX_ARCHIVE_BASE64_LEN);
        let decoded = decode_archive_base64(&at_limit).expect("at-limit passes pre-check");
        assert!(decoded.len() > MAX_ARCHIVE_BYTES);
        let error = import_package_base64(&test_ctx(&tempfile::tempdir().expect("t")), &at_limit)
            .expect_err("import must fail on 50MB");
        assert!(error.contains("50MB"), "error: {error}");

        // 非法 base64。
        let error = decode_archive_base64("not-base64!!!").expect_err("invalid base64");
        assert!(error.contains("not valid base64"), "error: {error}");
    }

    #[test]
    fn rejects_traversal_entry() {
        let ctx = test_ctx(&tempfile::tempdir().expect("t"));
        let bytes = zip_of(&[("../evil.yaml", b"x"), (WORKFLOW_FILE, GOOD_YAML.as_bytes())]);
        let error = import_package(&ctx, &bytes).expect_err("traversal must fail");
        assert!(error.contains("unsafe path"), "error: {error}");
        assert!(!workflows_dir(&ctx).join("mini-flow").exists(), "不落半成品");
    }

    #[test]
    fn rejects_absolute_path_entry() {
        let ctx = test_ctx(&tempfile::tempdir().expect("t"));
        let bytes = zip_of(&[("/abs/evil.yaml", b"x"), (WORKFLOW_FILE, GOOD_YAML.as_bytes())]);
        let error = import_package(&ctx, &bytes).expect_err("absolute must fail");
        assert!(error.contains("unsafe path"), "error: {error}");
    }

    #[test]
    fn rejects_non_utf8_entry_name() {
        let ctx = test_ctx(&tempfile::tempdir().expect("t"));
        // 名字节补丁：条目名 "z"（1 字节，内容无 'z'）→ 0xFF，本地头与中央目录各一处。
        let mut bytes = zip_of(&[("z", b"hello"), (WORKFLOW_FILE, GOOD_YAML.as_bytes())]);
        let positions: Vec<usize> = bytes
            .iter()
            .enumerate()
            .filter_map(|(index, byte)| (*byte == b'z').then_some(index))
            .collect();
        assert_eq!(positions.len(), 2, "预期名字节恰两处: {positions:?}");
        for position in positions {
            bytes[position] = 0xFF;
        }
        let error = import_package(&ctx, &bytes).expect_err("non-UTF-8 must fail");
        assert!(error.contains("not valid UTF-8"), "error: {error}");
    }

    #[test]
    fn rejects_archive_over_50mb() {
        let ctx = test_ctx(&tempfile::tempdir().expect("t"));
        let bytes = vec![0u8; MAX_ARCHIVE_BYTES + 1];
        let error = import_package(&ctx, &bytes).expect_err("oversize must fail");
        assert!(error.contains("50MB"), "error: {error}");
    }

    #[test]
    fn rejects_unpacked_total_over_200mb() {
        let ctx = test_ctx(&tempfile::tempdir().expect("t"));
        // 声明尺寸撒谎：big.bin 实际 1 字节，中央目录声明 300MB。
        let mut bytes = zip_of(&[("big.bin", b"x"), (WORKFLOW_FILE, GOOD_YAML.as_bytes())]);
        let sig = b"PK\x01\x02";
        let needle = b"big.bin";
        let mut patched = false;
        for index in 0..bytes.len().saturating_sub(46 + needle.len()) {
            if bytes[index..index + 4] == sig[..]
                && &bytes[index + 46..index + 46 + needle.len()] == needle
            {
                bytes[index + 24..index + 28].copy_from_slice(&300_000_000u32.to_le_bytes());
                patched = true;
            }
        }
        assert!(patched, "未找到 big.bin 中央目录条目");
        let error = import_package(&ctx, &bytes).expect_err("zip bomb must fail");
        assert!(error.contains("200MB"), "error: {error}");
    }

    #[test]
    fn rejects_missing_workflow_yaml() {
        let ctx = test_ctx(&tempfile::tempdir().expect("t"));
        let bytes = zip_of(&[(WORKFLOW_README, b"# hi")]);
        let error = import_package(&ctx, &bytes).expect_err("missing yaml must fail");
        assert!(error.contains("missing workflow.yaml"), "error: {error}");
    }

    #[test]
    fn rejects_broken_yaml() {
        let ctx = test_ctx(&tempfile::tempdir().expect("t"));
        let bytes = zip_of(&[(WORKFLOW_FILE, b"not: [valid")]);
        let error = import_package(&ctx, &bytes).expect_err("broken yaml must fail");
        assert!(error.contains("Unable to parse workflow yaml"), "error: {error}");
    }

    #[test]
    fn rejects_bad_slug() {
        let ctx = test_ctx(&tempfile::tempdir().expect("t"));
        for slug in ["UPPER", "../escape", "a/b"] {
            let yaml = format!("name: 小包\nslug: {slug}\nversion: 0.1.0\ndescription: 负例\n");
            let bytes = zip_of(&[(WORKFLOW_FILE, yaml.as_bytes())]);
            let error = import_package(&ctx, &bytes).expect_err("bad slug must fail");
            assert!(error.contains("failed validation"), "slug '{slug}': {error}");
        }
        // 坏 slug 对应的目录不得落盘。
        assert!(!ctx.data_dir().join("escape").exists());
        assert!(!workflows_dir(&ctx).join("UPPER").exists());
    }

    #[test]
    fn rejects_invalid_source_json() {
        let ctx = test_ctx(&tempfile::tempdir().expect("t"));
        let bytes = zip_of(&[
            (WORKFLOW_FILE, GOOD_YAML.as_bytes()),
            ("source.json", b"{ not valid json"),
        ]);
        let error = import_package(&ctx, &bytes).expect_err("bad source.json must fail");
        assert!(error.contains("source.json"), "error: {error}");
        assert!(!workflows_dir(&ctx).join("mini-flow").exists(), "不落半成品");
    }

    #[test]
    fn imports_minimal_package_without_readme_or_source() {
        let ctx = test_ctx(&tempfile::tempdir().expect("t"));
        let bytes = zip_of(&[(WORKFLOW_FILE, GOOD_YAML.as_bytes())]);
        let result = import_package(&ctx, &bytes).expect("import minimal");
        assert_eq!(
            result,
            ImportResult {
                slug: "mini-flow".to_string(),
                had_source: false
            }
        );
        let dir = workflows_dir(&ctx).join("mini-flow");
        assert!(dir.join(WORKFLOW_FILE).is_file());
        assert!(!dir.join(WORKFLOW_README).exists());
        assert!(crate::workflow_update::read_source(&ctx, "mini-flow").is_none());
    }

    #[test]
    fn save_export_to_path_roundtrip() {
        let temp = tempfile::tempdir().expect("temp dir");
        let payload = b"fake-zip-bytes";
        let encoded = STANDARD.encode(payload);
        let target = temp.path().join("out").join("pkg.zip");
        // 父目录不存在 → 写失败（不擅自建目录，路径来自保存对话框）。
        assert!(save_export_to_path(&path_to_string(&target), &encoded).is_err());
        fs::create_dir_all(target.parent().expect("parent")).expect("mkdir");
        save_export_to_path(&path_to_string(&target), &encoded).expect("save");
        assert_eq!(fs::read(&target).expect("read back"), payload);

        let error = save_export_to_path(&path_to_string(&target), "!!!").expect_err("bad base64");
        assert!(error.contains("not valid base64"), "error: {error}");
    }
}
