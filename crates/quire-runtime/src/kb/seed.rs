use super::*;

#[derive(Debug, Default, Deserialize)]
struct Frontmatter {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    system_prompt: Option<String>,
    #[serde(default, alias = "tools")]
    allowed_tools: Vec<String>,
    #[serde(default)]
    is_default: Option<bool>,
    #[serde(default)]
    enabled: Option<bool>,
}

impl KbService {
    pub async fn seed(&self, workspace: &Path, default_model: &str) -> RuntimeResult<SeedReport> {
        let branch = self.active_branch().await?.name;
        let mut report = SeedReport::default();
        let _ = report.documents;

        let agent_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*)::bigint FROM agent_definitions WHERE branch = $1")
                .bind(&branch)
                .fetch_one(self.store.pool())
                .await?;
        if agent_count == 0 {
            let agents_dir = workspace.join("agents");
            if agents_dir.is_dir() {
                for entry in std::fs::read_dir(&agents_dir)? {
                    let entry = entry?;
                    let path = entry.path();
                    if !is_markdown(&path) {
                        continue;
                    }
                    let raw = tokio::fs::read_to_string(&path).await?;
                    let (frontmatter, body) = parse_frontmatter(&raw)?;
                    let fallback = path
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .unwrap_or("agent")
                        .to_string();
                    let name = frontmatter
                        .as_ref()
                        .and_then(|value| value.name.clone())
                        .unwrap_or(fallback);
                    let description = frontmatter
                        .as_ref()
                        .and_then(|value| value.description.clone())
                        .unwrap_or_default();
                    let model = frontmatter
                        .as_ref()
                        .and_then(|value| value.model.clone())
                        .unwrap_or_else(|| default_model.to_string());
                    let system_prompt = frontmatter
                        .as_ref()
                        .and_then(|value| value.system_prompt.clone())
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or(body);
                    let allowed_tools = frontmatter
                        .as_ref()
                        .map(|value| value.allowed_tools.clone())
                        .unwrap_or_default();
                    let is_default = frontmatter
                        .as_ref()
                        .and_then(|value| value.is_default)
                        .unwrap_or(name == "main");
                    let enabled = frontmatter
                        .as_ref()
                        .and_then(|value| value.enabled)
                        .unwrap_or(true);
                    self.insert_agent_raw(
                        &branch,
                        UpsertKbAgentCommand {
                            name: &name,
                            description: &description,
                            model: &model,
                            system_prompt: &system_prompt,
                            allowed_tools: &allowed_tools,
                            is_default,
                            enabled,
                        },
                    )
                    .await?;
                    report.agents += 1;
                }
            }
        }

        let skill_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*)::bigint FROM skill_definitions WHERE branch = $1")
                .bind(&branch)
                .fetch_one(self.store.pool())
                .await?;
        if skill_count == 0 {
            let skills_dir = workspace.join("skills");
            if skills_dir.is_dir() {
                for entry in std::fs::read_dir(&skills_dir)? {
                    let entry = entry?;
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }
                    let manifest = path.join("SKILL.md");
                    if !manifest.is_file() {
                        continue;
                    }
                    let raw = tokio::fs::read_to_string(&manifest).await?;
                    let (frontmatter, body) = parse_frontmatter(&raw)?;
                    let fallback = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("skill")
                        .to_string();
                    let name = frontmatter
                        .as_ref()
                        .and_then(|value| value.name.clone())
                        .unwrap_or(fallback);
                    let description = frontmatter
                        .as_ref()
                        .and_then(|value| value.description.clone())
                        .unwrap_or_default();
                    let resources = read_skill_resources(&path)?;
                    self.insert_skill_raw(&branch, &name, &description, &body, true, &resources)
                        .await?;
                    report.skills += 1;
                }
            }
        }

        if report.documents + report.agents + report.skills > 0 {
            report.kb_version = self
                .after_kb_mutation_on(&branch, KbBranchStatus::Active)
                .await?;
        } else {
            report.kb_version = self.store.kb_version().await?;
        }
        Ok(report)
    }

    // ---------------------------------------------------------------------
    // Documents
    // ---------------------------------------------------------------------

    async fn insert_agent_raw(
        &self,
        branch: &str,
        command: UpsertKbAgentCommand<'_>,
    ) -> RuntimeResult<()> {
        let UpsertKbAgentCommand {
            name,
            description,
            model,
            system_prompt,
            allowed_tools,
            is_default,
            enabled,
        } = command;
        sqlx::query(
            r#"INSERT INTO agent_definitions
               (branch, name, description, model, system_prompt, allowed_tools, is_default, version, enabled)
               VALUES ($1, $2, $3, $4, $5, $6, $7, 1, $8)
               ON CONFLICT (branch, name) DO NOTHING"#,
        )
        .bind(branch)
        .bind(name)
        .bind(description)
        .bind(model)
        .bind(system_prompt)
        .bind(serde_json::to_value(allowed_tools)?)
        .bind(is_default)
        .bind(enabled)
        .execute(self.store.pool())
        .await?;
        Ok(())
    }

    async fn insert_skill_raw(
        &self,
        branch: &str,
        name: &str,
        description: &str,
        body: &str,
        enabled: bool,
        resources: &[(String, String)],
    ) -> RuntimeResult<()> {
        let mut tx = self.store.pool().begin().await?;
        sqlx::query(
            r#"INSERT INTO skill_definitions (branch, name, description, body, version, enabled)
               VALUES ($1, $2, $3, $4, 1, $5)
               ON CONFLICT (branch, name) DO NOTHING"#,
        )
        .bind(branch)
        .bind(name)
        .bind(description)
        .bind(body)
        .bind(enabled)
        .execute(&mut *tx)
        .await?;
        for (path, content) in resources {
            sqlx::query(
                r#"INSERT INTO skill_resources (branch, skill_name, path, content, version)
                   VALUES ($1, $2, $3, $4, 1)
                   ON CONFLICT (branch, skill_name, path) DO NOTHING"#,
            )
            .bind(branch)
            .bind(name)
            .bind(path)
            .bind(content)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

fn parse_frontmatter(raw: &str) -> RuntimeResult<(Option<Frontmatter>, String)> {
    let trimmed = raw.trim_start_matches('\u{feff}').trim_start();
    let Some(rest) = trimmed.strip_prefix("---") else {
        return Ok((None, raw.trim().to_string()));
    };
    let Some((yaml, body)) = rest.split_once("\n---") else {
        return Ok((None, raw.trim().to_string()));
    };
    let frontmatter: Frontmatter = serde_yaml_ng::from_str(yaml.trim()).map_err(|error| {
        RuntimeError::InvalidRequest(format!("invalid YAML frontmatter: {error}"))
    })?;
    Ok((Some(frontmatter), body.trim().to_string()))
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

fn read_skill_resources(directory: &Path) -> RuntimeResult<Vec<(String, String)>> {
    let mut resources = BTreeMap::new();
    collect_skill_files(directory, directory, &mut resources, 0)?;
    Ok(resources.into_iter().collect())
}

fn collect_skill_files(
    root: &Path,
    current: &Path,
    resources: &mut BTreeMap<String, String>,
    depth: usize,
) -> RuntimeResult<()> {
    if depth > 8 {
        return Ok(());
    }
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        if relative == "SKILL.md" {
            continue;
        }
        if path.is_dir() {
            collect_skill_files(root, &path, resources, depth + 1)?;
        } else if let Ok(bytes) = std::fs::read(&path)
            && let Ok(text) = String::from_utf8(bytes)
        {
            resources.insert(relative, text);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(doc_id: &str, content: &str) -> KbDocumentRecord {
        KbDocumentRecord {
            doc_id: doc_id.to_string(),
            name: doc_id.to_string(),
            content: content.to_string(),
            version: 1,
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn frontmatter_and_body_are_split() {
        let raw = "---\nname: reviewer\nmodel: gpt-test\nallowed_tools: [lint_document, read_blocks]\n---\n\nReview carefully.\n";
        let (frontmatter, body) = parse_frontmatter(raw).unwrap();
        let frontmatter = frontmatter.expect("frontmatter");
        assert_eq!(frontmatter.name.as_deref(), Some("reviewer"));
        assert_eq!(
            frontmatter.allowed_tools,
            vec!["lint_document", "read_blocks"]
        );
        assert_eq!(body, "Review carefully.");
    }

    #[test]
    fn missing_frontmatter_keeps_the_whole_document_as_body() {
        let (frontmatter, body) = parse_frontmatter("Plain markdown").unwrap();
        assert!(frontmatter.is_none());
        assert_eq!(body, "Plain markdown");
    }

    #[test]
    fn disjoint_item_changes_auto_merge() {
        let base = KbEntries {
            documents: vec![document("shared", "base")],
            ..KbEntries::default()
        };
        let active = KbEntries {
            documents: vec![document("shared", "base"), document("active-only", "A")],
            ..KbEntries::default()
        };
        let branch = KbEntries {
            documents: vec![document("shared", "base"), document("branch-only", "B")],
            ..KbEntries::default()
        };
        let (merged, conflicts) = merge_entries(&base, &active, &branch);
        assert!(conflicts.is_empty());
        let keys: Vec<&str> = merged
            .documents
            .iter()
            .map(|document| document.doc_id.as_str())
            .collect();
        assert_eq!(keys, vec!["active-only", "branch-only", "shared"]);
    }

    #[test]
    fn overlapping_item_changes_report_three_way_content() {
        let base = KbEntries {
            documents: vec![document("shared", "base")],
            agents: Vec::new(),
            skills: Vec::new(),
        };
        let active = KbEntries {
            documents: vec![document("shared", "active")],
            agents: Vec::new(),
            skills: Vec::new(),
        };
        let branch = KbEntries {
            documents: vec![document("shared", "branch")],
            agents: Vec::new(),
            skills: Vec::new(),
        };
        let (_merged, conflicts) = merge_entries(&base, &active, &branch);
        assert_eq!(conflicts.len(), 1);
        let conflict = conflicts.first().expect("conflict");
        assert_eq!(conflict.item_kind, KbItemKind::Doc);
        assert_eq!(conflict.item_key, "shared");
        assert_eq!(
            conflict
                .base_content
                .as_ref()
                .and_then(|value| value.get("content"))
                .and_then(Value::as_str),
            Some("base")
        );
        assert_eq!(
            conflict
                .active_content
                .as_ref()
                .and_then(|value| value.get("content"))
                .and_then(Value::as_str),
            Some("active")
        );
        assert_eq!(
            conflict
                .branch_content
                .as_ref()
                .and_then(|value| value.get("content"))
                .and_then(Value::as_str),
            Some("branch")
        );
    }

    #[test]
    fn branch_names_are_restricted_to_safe_tokens() {
        assert!(validate_branch_name("draft-2026.11_a").is_ok());
        assert!(validate_branch_name("").is_err());
        assert!(validate_branch_name("-leading").is_err());
        assert!(validate_branch_name("has space").is_err());
        assert!(validate_branch_name("slash/name").is_err());
    }
}
