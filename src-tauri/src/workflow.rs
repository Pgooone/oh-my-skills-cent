use crate::context::AppContext;
use crate::fs_ops::{ensure_dir, path_to_string};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

pub const WORKFLOW_FILE: &str = "workflow.yaml";
pub const WORKFLOW_README: &str = "README.md";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Workflow {
    pub name: String,
    pub slug: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub groups: Vec<WorkflowGroup>,
    #[serde(default)]
    pub steps: Vec<WorkflowStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowGroup {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowStep {
    pub name: String,
    pub group: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub skills: Vec<StepSkill>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum StepSkill {
    Ref(SkillRef),
    Placeholder { placeholder: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillRef {
    pub source_type: String,
    pub source_url: String,
    pub slug: String,
    #[serde(default)]
    pub skill_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledWorkflow {
    pub slug: String,
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub icon: Option<String>,
    pub step_count: usize,
    pub has_placeholder: bool,
    #[serde(default)]
    pub error: Option<String>,
}

impl Workflow {
    pub fn from_yaml(text: &str) -> Result<Workflow, String> {
        serde_yml::from_str(text).map_err(|error| format!("Unable to parse workflow yaml: {error}"))
    }

    pub fn to_yaml(&self) -> Result<String, String> {
        serde_yml::to_string(self)
            .map_err(|error| format!("Unable to serialize workflow '{}': {error}", self.slug))
    }

    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if !is_valid_slug(&self.slug) {
            errors.push(format!(
                "Invalid workflow slug '{}': must be non-empty and match [a-z0-9-]+",
                self.slug
            ));
        }

        let group_ids: BTreeSet<&str> = self.groups.iter().map(|group| group.id.as_str()).collect();
        for step in &self.steps {
            if !group_ids.contains(step.group.as_str()) {
                errors.push(format!(
                    "Step '{}' references unknown group '{}'",
                    step.name, step.group
                ));
            }
            for skill in &step.skills {
                let StepSkill::Ref(reference) = skill else {
                    continue;
                };
                if reference.source_type != "github" {
                    errors.push(format!(
                        "Step '{}' skill '{}': unsupported sourceType '{}' (v1 supports 'github' only)",
                        step.name, reference.slug, reference.source_type
                    ));
                }
                if let Err(error) = crate::skill_ops::normalize_github_url(&reference.source_url) {
                    errors.push(format!(
                        "Step '{}' skill '{}': invalid sourceUrl '{}': {}",
                        step.name, reference.slug, reference.source_url, error
                    ));
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

pub fn workflows_dir(ctx: &AppContext) -> PathBuf {
    ctx.data_dir().join("workflows")
}

pub fn list_installed(ctx: &AppContext) -> Result<Vec<InstalledWorkflow>, String> {
    let root = workflows_dir(ctx);
    if !root.exists() {
        return Ok(Vec::new());
    }

    let entries = fs::read_dir(&root).map_err(|error| {
        format!(
            "Unable to list workflows at {}: {error}",
            path_to_string(&root)
        )
    })?;

    let mut installed = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let slug = entry.file_name().to_string_lossy().to_string();
        installed.push(match read_workflow_dir(&dir) {
            Ok(workflow) => installed_from_workflow(slug, workflow),
            Err(error) => InstalledWorkflow {
                name: slug.clone(),
                slug,
                version: String::new(),
                description: String::new(),
                author: None,
                tags: Vec::new(),
                icon: None,
                step_count: 0,
                has_placeholder: false,
                error: Some(error),
            },
        });
    }

    installed.sort_by(|left, right| left.slug.cmp(&right.slug));
    Ok(installed)
}

pub fn load(ctx: &AppContext, slug: &str) -> Result<Workflow, String> {
    let dir = workflow_dir(ctx, slug)?;
    if !dir.is_dir() {
        return Err(format!(
            "Workflow '{slug}' is not installed: {}",
            path_to_string(&dir)
        ));
    }
    read_workflow_dir(&dir)
}

pub fn save(ctx: &AppContext, workflow: &Workflow, readme: Option<&str>) -> Result<(), String> {
    if let Err(errors) = workflow.validate() {
        return Err(format!(
            "Workflow '{}' failed validation: {}",
            workflow.slug,
            errors.join("; ")
        ));
    }

    let dir = workflow_dir(ctx, &workflow.slug)?;
    ensure_dir(&dir)?;

    let text = workflow.to_yaml()?;
    let file = dir.join(WORKFLOW_FILE);
    fs::write(&file, text).map_err(|error| {
        format!(
            "Unable to write workflow at {}: {error}",
            path_to_string(&file)
        )
    })?;

    if let Some(readme) = readme {
        let readme_path = dir.join(WORKFLOW_README);
        fs::write(&readme_path, readme).map_err(|error| {
            format!(
                "Unable to write workflow README at {}: {error}",
                path_to_string(&readme_path)
            )
        })?;
    }

    Ok(())
}

pub fn delete(ctx: &AppContext, slug: &str) -> Result<(), String> {
    let dir = workflow_dir(ctx, slug)?;
    if !dir.is_dir() {
        return Err(format!(
            "Workflow '{slug}' is not installed: {}",
            path_to_string(&dir)
        ));
    }
    crate::fs_ops::remove_entry(&dir)
}

fn workflow_dir(ctx: &AppContext, slug: &str) -> Result<PathBuf, String> {
    if !is_valid_slug(slug) {
        return Err(format!(
            "Invalid workflow slug '{slug}': must be non-empty and match [a-z0-9-]+"
        ));
    }
    Ok(workflows_dir(ctx).join(slug))
}

fn read_workflow_dir(dir: &Path) -> Result<Workflow, String> {
    let file = dir.join(WORKFLOW_FILE);
    let text = fs::read_to_string(&file).map_err(|error| {
        format!(
            "Unable to read workflow at {}: {error}",
            path_to_string(&file)
        )
    })?;
    let workflow = Workflow::from_yaml(&text)
        .map_err(|error| format!("{}: {error}", path_to_string(&file)))?;
    if let Err(errors) = workflow.validate() {
        return Err(format!(
            "{}: {}",
            path_to_string(&file),
            errors.join("; ")
        ));
    }
    Ok(workflow)
}

fn installed_from_workflow(slug: String, workflow: Workflow) -> InstalledWorkflow {
    let has_placeholder = workflow
        .steps
        .iter()
        .flat_map(|step| step.skills.iter())
        .any(|skill| matches!(skill, StepSkill::Placeholder { .. }));
    InstalledWorkflow {
        slug,
        name: workflow.name,
        version: workflow.version,
        description: workflow.description,
        author: workflow.author,
        tags: workflow.tags,
        icon: workflow.icon,
        step_count: workflow.steps.len(),
        has_placeholder,
        error: None,
    }
}

fn is_valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOFTWARE_DEVELOPMENT_YAML: &str =
        include_str!("../tests/fixtures/workflows/software-development.yaml");
    const CODE_REVIEW_FLOW_YAML: &str =
        include_str!("../tests/fixtures/workflows/code-review-flow.yaml");

    fn test_ctx(temp: &tempfile::TempDir) -> AppContext {
        AppContext::new(temp.path().join("data"), temp.path().join("home"))
    }

    fn sample_workflow() -> Workflow {
        Workflow {
            name: "样例工作流".to_string(),
            slug: "sample-flow".to_string(),
            version: "0.1.0".to_string(),
            description: "样例".to_string(),
            author: Some("tester".to_string()),
            tags: vec!["sample".to_string()],
            icon: None,
            groups: vec![WorkflowGroup {
                id: "doing".to_string(),
                name: "执行".to_string(),
            }],
            steps: vec![WorkflowStep {
                name: "唯一步骤".to_string(),
                group: "doing".to_string(),
                description: String::new(),
                skills: vec![StepSkill::Ref(SkillRef {
                    source_type: "github".to_string(),
                    source_url: "https://github.com/mattpocock/skills.git".to_string(),
                    slug: "tdd".to_string(),
                    skill_path: Some("skills/engineering/tdd".to_string()),
                })],
            }],
        }
    }

    #[test]
    fn parses_real_registry_software_development_fixture() {
        let workflow = Workflow::from_yaml(SOFTWARE_DEVELOPMENT_YAML).expect("parse fixture");

        assert_eq!(workflow.slug, "software-development");
        assert_eq!(workflow.name, "软件开发工作流");
        assert_eq!(workflow.version, "0.1.0");
        assert_eq!(workflow.author.as_deref(), Some("Pgooone"));
        assert_eq!(
            workflow.tags,
            vec![
                "software-development".to_string(),
                "tdd".to_string(),
                "requirements".to_string()
            ]
        );
        assert_eq!(workflow.icon.as_deref(), Some("code"));
        assert_eq!(workflow.groups.len(), 3);
        assert_eq!(workflow.groups[0].id, "requirements");
        assert_eq!(workflow.steps.len(), 3);

        let step = &workflow.steps[0];
        assert_eq!(step.name, "需求澄清");
        assert_eq!(step.group, "requirements");
        assert_eq!(step.skills.len(), 1);
        let StepSkill::Ref(reference) = &step.skills[0] else {
            panic!("expected skill ref");
        };
        assert_eq!(reference.source_type, "github");
        assert_eq!(
            reference.source_url,
            "https://github.com/mattpocock/skills.git"
        );
        assert_eq!(reference.slug, "grill-me");
        assert_eq!(
            reference.skill_path.as_deref(),
            Some("skills/productivity/grill-me")
        );

        assert_eq!(workflow.validate(), Ok(()));
    }

    #[test]
    fn parses_real_registry_code_review_fixture_with_placeholder() {
        let workflow = Workflow::from_yaml(CODE_REVIEW_FLOW_YAML).expect("parse fixture");

        assert_eq!(workflow.slug, "code-review-flow");
        assert_eq!(workflow.groups.len(), 2);
        assert_eq!(workflow.steps.len(), 2);

        let StepSkill::Ref(reference) = &workflow.steps[0].skills[0] else {
            panic!("expected skill ref");
        };
        assert_eq!(reference.slug, "code-review");

        let StepSkill::Placeholder { placeholder } = &workflow.steps[1].skills[0] else {
            panic!("expected placeholder");
        };
        assert_eq!(placeholder, "待指定修复类 skill");

        assert_eq!(workflow.validate(), Ok(()));
    }

    #[test]
    fn untagged_boundary_empty_and_missing_skills_parse_to_empty_vec() {
        let with_empty = Workflow::from_yaml(
            "name: a\nslug: a\nversion: v1\ndescription: d\n\
             groups:\n  - id: g\n    name: g\n\
             steps:\n  - name: s\n    group: g\n    skills: []\n",
        )
        .expect("parse empty skills");
        assert!(with_empty.steps[0].skills.is_empty());

        let without_key = Workflow::from_yaml(
            "name: a\nslug: a\nversion: v1\ndescription: d\n\
             groups:\n  - id: g\n    name: g\n\
             steps:\n  - name: s\n    group: g\n",
        )
        .expect("parse missing skills key");
        assert!(without_key.steps[0].skills.is_empty());
    }

    #[test]
    fn untagged_boundary_single_placeholder() {
        let workflow = Workflow::from_yaml(
            "name: a\nslug: a\nversion: v1\ndescription: d\n\
             groups:\n  - id: g\n    name: g\n\
             steps:\n  - name: s\n    group: g\n    skills:\n      - placeholder: 待补充\n",
        )
        .expect("parse single placeholder");

        assert_eq!(workflow.steps[0].skills.len(), 1);
        let StepSkill::Placeholder { placeholder } = &workflow.steps[0].skills[0] else {
            panic!("expected placeholder");
        };
        assert_eq!(placeholder, "待补充");
    }

    #[test]
    fn untagged_boundary_single_ref_without_skill_path() {
        let workflow = Workflow::from_yaml(
            "name: a\nslug: a\nversion: v1\ndescription: d\n\
             groups:\n  - id: g\n    name: g\n\
             steps:\n  - name: s\n    group: g\n    skills:\n\
             \x20     - sourceType: github\n\
             \x20       sourceUrl: https://github.com/mattpocock/skills.git\n\
             \x20       slug: grill-me\n",
        )
        .expect("parse ref without skillPath");

        let StepSkill::Ref(reference) = &workflow.steps[0].skills[0] else {
            panic!("expected ref");
        };
        assert_eq!(reference.slug, "grill-me");
        assert_eq!(reference.skill_path, None);
    }

    #[test]
    fn validate_rejects_bad_slugs() {
        for slug in ["", "Software-Dev", "soft_ware", "../escape", "a/b"] {
            let mut workflow = sample_workflow();
            workflow.slug = slug.to_string();
            let Err(errors) = workflow.validate() else {
                panic!("slug '{slug}' must be rejected");
            };
            assert!(
                errors.iter().any(|error| error.contains("slug")),
                "slug '{slug}' errors: {errors:?}"
            );
        }
    }

    #[test]
    fn validate_rejects_step_referencing_unknown_group() {
        let mut workflow = sample_workflow();
        workflow.steps[0].group = "missing".to_string();

        let Err(errors) = workflow.validate() else {
            panic!("unknown group must be rejected");
        };
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("unknown group 'missing'"));
    }

    #[test]
    fn validate_rejects_non_github_source_type() {
        let mut workflow = sample_workflow();
        let StepSkill::Ref(reference) = &mut workflow.steps[0].skills[0] else {
            panic!("expected ref");
        };
        reference.source_type = "gitlab".to_string();

        let Err(errors) = workflow.validate() else {
            panic!("non-github sourceType must be rejected");
        };
        assert!(errors
            .iter()
            .any(|error| error.contains("unsupported sourceType 'gitlab'")));
    }

    #[test]
    fn validate_rejects_non_github_source_url() {
        let mut workflow = sample_workflow();
        let StepSkill::Ref(reference) = &mut workflow.steps[0].skills[0] else {
            panic!("expected ref");
        };
        reference.source_url = "https://gitlab.com/owner/repo.git".to_string();

        let Err(errors) = workflow.validate() else {
            panic!("non-github sourceUrl must be rejected");
        };
        assert!(errors
            .iter()
            .any(|error| error.contains("invalid sourceUrl")));
    }

    #[test]
    fn validate_accepts_github_url_spellings() {
        for url in [
            "https://github.com/mattpocock/skills.git",
            "https://github.com/mattpocock/skills/",
            "git@github.com:mattpocock/skills.git",
            "github.com/mattpocock/skills",
            "mattpocock/skills",
        ] {
            let mut workflow = sample_workflow();
            let StepSkill::Ref(reference) = &mut workflow.steps[0].skills[0] else {
                panic!("expected ref");
            };
            reference.source_url = url.to_string();
            assert_eq!(workflow.validate(), Ok(()), "url '{url}' must be accepted");
        }
    }

    #[test]
    fn validate_aggregates_multiple_errors() {
        let mut workflow = sample_workflow();
        workflow.slug = "Bad Slug".to_string();
        workflow.steps[0].group = "missing".to_string();
        let StepSkill::Ref(reference) = &mut workflow.steps[0].skills[0] else {
            panic!("expected ref");
        };
        reference.source_type = "ftp".to_string();
        reference.source_url = "https://example.com/x".to_string();

        let Err(errors) = workflow.validate() else {
            panic!("invalid workflow must be rejected");
        };
        assert_eq!(errors.len(), 4, "errors: {errors:?}");
    }

    #[test]
    fn save_load_delete_round_trip() {
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        let workflow = sample_workflow();

        save(&ctx, &workflow, Some("# 样例 README")).expect("save");
        let dir = workflows_dir(&ctx).join("sample-flow");
        assert!(dir.join(WORKFLOW_FILE).is_file());
        assert_eq!(
            fs::read_to_string(dir.join(WORKFLOW_README)).expect("readme"),
            "# 样例 README"
        );

        let loaded = load(&ctx, "sample-flow").expect("load");
        assert_eq!(loaded, workflow);

        delete(&ctx, "sample-flow").expect("delete");
        assert!(!dir.exists());
    }

    #[test]
    fn save_rejects_invalid_workflow_without_writing() {
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        let mut workflow = sample_workflow();
        workflow.steps[0].group = "missing".to_string();

        let error = save(&ctx, &workflow, None).expect_err("save must fail");
        assert!(error.contains("unknown group"));
        assert!(!workflows_dir(&ctx).join("sample-flow").exists());
    }

    #[test]
    fn list_installed_reports_steps_and_degrades_broken_files() {
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);

        save(&ctx, &sample_workflow(), None).expect("save sample");
        let placeholder_workflow =
            Workflow::from_yaml(CODE_REVIEW_FLOW_YAML).expect("parse fixture");
        save(&ctx, &placeholder_workflow, None).expect("save fixture");

        let broken = workflows_dir(&ctx).join("broken-flow");
        ensure_dir(&broken).expect("broken dir");
        fs::write(broken.join(WORKFLOW_FILE), "not: [valid").expect("write garbage");

        let stray_file = workflows_dir(&ctx).join("stray.txt");
        fs::write(&stray_file, "ignored").expect("write stray file");

        let installed = list_installed(&ctx).expect("list");
        assert_eq!(installed.len(), 3);
        assert_eq!(installed[0].slug, "broken-flow");
        assert!(installed[0].error.is_some());
        assert_eq!(installed[0].step_count, 0);

        assert_eq!(installed[1].slug, "code-review-flow");
        assert_eq!(installed[1].step_count, 2);
        assert!(installed[1].has_placeholder);
        assert_eq!(installed[1].error, None);

        assert_eq!(installed[2].slug, "sample-flow");
        assert_eq!(installed[2].step_count, 1);
        assert!(!installed[2].has_placeholder);
    }

    #[test]
    fn list_installed_flags_validation_failures_as_error_entries() {
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);

        let dir = workflows_dir(&ctx).join("invalid-group");
        ensure_dir(&dir).expect("dir");
        fs::write(
            dir.join(WORKFLOW_FILE),
            "name: a\nslug: invalid-group\nversion: v1\ndescription: d\n\
             groups:\n  - id: g\n    name: g\n\
             steps:\n  - name: s\n    group: nope\n",
        )
        .expect("write invalid workflow");

        let installed = list_installed(&ctx).expect("list");
        assert_eq!(installed.len(), 1);
        let error = installed[0].error.clone().expect("error entry");
        assert!(error.contains("unknown group 'nope'"), "error: {error}");
    }

    #[test]
    fn list_installed_returns_empty_when_dir_missing() {
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);
        assert_eq!(list_installed(&ctx).expect("list"), Vec::new());
    }

    #[test]
    fn load_and_delete_reject_traversal_and_missing_slugs() {
        let temp = tempfile::tempdir().expect("temp dir");
        let ctx = test_ctx(&temp);

        for slug in ["..", "../settings", "a/b", "UPPER", ""] {
            assert!(load(&ctx, slug).is_err(), "load '{slug}' must fail");
            assert!(delete(&ctx, slug).is_err(), "delete '{slug}' must fail");
            let mut workflow = sample_workflow();
            workflow.slug = slug.to_string();
            assert!(save(&ctx, &workflow, None).is_err(), "save '{slug}' must fail");
        }

        assert!(load(&ctx, "not-installed").is_err());
        assert!(delete(&ctx, "not-installed").is_err());
    }
}
