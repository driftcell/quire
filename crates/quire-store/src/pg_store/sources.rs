use super::*;

impl PgReportStore {
    pub async fn upsert_sources(
        &self,
        doc_id: &DocId,
        actor: &str,
        upserts: Vec<SourceUpsert>,
    ) -> Result<UpsertSourcesResult, StoreError> {
        let _guard = self.document_guard(doc_id.as_str()).await;
        let mut tx = self.pool.begin().await?;
        let current_rev = lock_current_revision(&mut tx, doc_id.as_str()).await?;
        let loaded = load_document_in_tx(&mut tx, doc_id.as_str()).await?;
        let before = loaded.document.clone();

        let existing_rows = sqlx::query!(
            r#"SELECT source_id, normalized_key, metadata
               FROM report_sources WHERE doc_id = $1
               ORDER BY source_id ASC
               FOR UPDATE"#,
            doc_id.as_str()
        )
        .fetch_all(&mut *tx)
        .await?;
        let mut key_to_id: BTreeMap<String, SourceId> = existing_rows
            .iter()
            .map(|row| {
                Ok((
                    row.normalized_key.clone(),
                    SourceId::parse(row.source_id.as_str())?,
                ))
            })
            .collect::<Result<BTreeMap<_, _>, StoreError>>()?;
        let mut sources: IndexMap<SourceId, SourceEntry> = before.sources.clone();
        let mut canonical_id_map = BTreeMap::new();
        let mut db_updates: BTreeMap<SourceId, (String, SourceEntry)> = BTreeMap::new();

        for upsert in upserts {
            let has_explicit_url = upsert
                .source
                .url
                .as_deref()
                .map(str::trim)
                .is_some_and(|url| !url.is_empty());
            let has_explicit_title = !upsert.source.title.trim().is_empty();
            if !has_explicit_title && !has_explicit_url && !sources.contains_key(&upsert.source_id)
            {
                return Err(StoreError::InvalidRequest(format!(
                    "source `{}` requires a non-empty `title` when registering a new source",
                    upsert.source_id
                )));
            }
            let normalized = if !has_explicit_title
                && !has_explicit_url
                && let Some(existing) = sources.get(&upsert.source_id)
            {
                normalized_source_key(existing)
            } else {
                normalized_source_key(&upsert.source)
            };
            let canonical_id = if !has_explicit_title
                && !has_explicit_url
                && sources.contains_key(&upsert.source_id)
            {
                upsert.source_id.clone()
            } else if let Some(existing_id) = key_to_id.get(&normalized) {
                existing_id.clone()
            } else if sources.contains_key(&upsert.source_id) {
                // The source ID exists under another normalized key; replace its metadata/key.
                for (key, id) in key_to_id.iter() {
                    if id == &upsert.source_id {
                        let stale_key = key.clone();
                        key_to_id.remove(&stale_key);
                        break;
                    }
                }
                upsert.source_id.clone()
            } else {
                upsert.source_id.clone()
            };
            canonical_id_map.insert(upsert.source_id.clone(), canonical_id.clone());
            if canonical_id != upsert.source_id {
                // Do not create an alias; callers should use the canonical ID.
                if let Some(existing) = sources.get(&canonical_id).cloned() {
                    let merged = merge_source(&existing, &upsert.source);
                    sources.insert(canonical_id.clone(), merged.clone());
                    db_updates.insert(canonical_id.clone(), (normalized.clone(), merged));
                }
                key_to_id.insert(normalized, canonical_id);
                continue;
            }

            if let Some(existing_source) = sources.get(&upsert.source_id) {
                let old_key = normalized_source_key(existing_source);
                key_to_id.remove(&old_key);
            }
            let merged = sources
                .get(&upsert.source_id)
                .map(|existing| merge_source(existing, &upsert.source))
                .unwrap_or_else(|| upsert.source.clone());
            let final_key = normalized_source_key(&merged);
            key_to_id.insert(final_key.clone(), canonical_id.clone());
            sources.insert(canonical_id.clone(), merged.clone());
            db_updates.insert(canonical_id, (final_key, merged));
        }

        let mut after = before.clone();
        after.sources = sources;
        after.revision = current_rev;
        if after.sources == before.sources {
            tx.commit().await?;
            let warnings = inline_warnings(&after);
            return Ok(UpsertSourcesResult {
                canonical_id_map,
                revision: current_rev,
                changed: false,
                sources: after.sources,
                inline_warnings: warnings,
            });
        }

        let new_revision = current_rev.next();
        after.revision = new_revision;
        let forward = vec![PatchOp::SetSources {
            sources: after.sources.clone(),
        }];
        let inverse = vec![PatchOp::SetSources {
            sources: before.sources.clone(),
        }];
        let forward_json = ops_to_json(&forward)?;
        let inverse_json = ops_to_json(&inverse)?;
        // Validate that normalized keys remain unique.
        validate_source_registry(&after.sources)?;
        for (source_id, (normalized, source)) in &db_updates {
            sqlx::query!(
                r#"INSERT INTO report_sources (doc_id, source_id, normalized_key, metadata, excerpt)
                   VALUES ($1, $2, $3, $4, $5)
                   ON CONFLICT (doc_id, source_id)
                   DO UPDATE SET normalized_key = EXCLUDED.normalized_key,
                                 metadata = EXCLUDED.metadata,
                                 excerpt = EXCLUDED.excerpt"#,
                doc_id.as_str(),
                source_id.as_str(),
                normalized,
                serde_json::to_value(source)?,
                source.excerpt.as_deref()
            )
            .execute(&mut *tx)
            .await?;
        }
        update_document_row(&mut tx, doc_id.as_str(), &after, new_revision).await?;
        sqlx::query!(
            r#"INSERT INTO report_revisions
               (doc_id, rev_num, actor, summary, forward_ops, inverse_ops, affected_block_ids)
               VALUES ($1, $2, $3, 'upsert_sources', $4, $5, '{}')"#,
            doc_id.as_str(),
            new_revision.get(),
            actor,
            forward_json,
            inverse_json
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        let event = RevisionEvent::new(
            doc_id.clone(),
            new_revision,
            actor,
            "upsert_sources",
            forward_json,
        );
        if let Err(error) = self.bus.publish_revision(&event).await {
            warn!(%error, doc_id = doc_id.as_str(), revision = %new_revision, "failed to publish source revision");
        }
        self.maybe_snapshot(doc_id.as_str(), new_revision, after.clone());

        let warnings = inline_warnings(&after);
        Ok(UpsertSourcesResult {
            canonical_id_map,
            revision: new_revision,
            changed: true,
            sources: after.sources,
            inline_warnings: warnings,
        })
    }

    /// Convenience for validating a doc before creating it.
    pub fn validate_document_for_create(document: &ReportDocument) -> Result<(), StoreError> {
        validate_doc_id(document.doc_id.as_str())?;

        validate_source_registry(&document.sources)
    }
}

pub(crate) fn merge_source_upserts(
    existing: &IndexMap<SourceId, SourceEntry>,
    upserts: &[SourceUpsert],
) -> Result<SourceMergeResult, StoreError> {
    let mut sources = existing.clone();
    let mut key_to_id: BTreeMap<String, SourceId> = sources
        .iter()
        .map(|(source_id, source)| (normalized_source_key(source), source_id.clone()))
        .collect();
    let mut canonical_map = BTreeMap::new();
    for upsert in upserts {
        let has_explicit_url = upsert
            .source
            .url
            .as_deref()
            .map(str::trim)
            .is_some_and(|url| !url.is_empty());
        let has_explicit_title = !upsert.source.title.trim().is_empty();
        if !has_explicit_title && !has_explicit_url && !sources.contains_key(&upsert.source_id) {
            return Err(StoreError::InvalidRequest(format!(
                "source `{}` requires a non-empty `title` when registering a new source",
                upsert.source_id
            )));
        }
        let normalized = if !has_explicit_title
            && !has_explicit_url
            && let Some(existing) = sources.get(&upsert.source_id)
        {
            normalized_source_key(existing)
        } else {
            normalized_source_key(&upsert.source)
        };
        let canonical_id = if !has_explicit_title
            && !has_explicit_url
            && sources.contains_key(&upsert.source_id)
        {
            upsert.source_id.clone()
        } else if let Some(existing_id) = key_to_id.get(&normalized) {
            existing_id.clone()
        } else {
            upsert.source_id.clone()
        };
        canonical_map.insert(upsert.source_id.clone(), canonical_id.clone());
        if canonical_id != upsert.source_id {
            if let Some(existing_source) = sources.get(&canonical_id).cloned() {
                let merged = merge_source(&existing_source, &upsert.source);
                key_to_id.insert(normalized_source_key(&merged), canonical_id.clone());
                sources.insert(canonical_id.clone(), merged);
            }
            continue;
        }
        if let Some(existing_source) = sources.get(&upsert.source_id) {
            let old_key = normalized_source_key(existing_source);
            key_to_id.remove(&old_key);
        }
        let merged = sources
            .get(&upsert.source_id)
            .map(|existing_source| merge_source(existing_source, &upsert.source))
            .unwrap_or_else(|| upsert.source.clone());
        key_to_id.insert(normalized_source_key(&merged), canonical_id.clone());
        sources.insert(canonical_id, merged);
    }
    validate_source_registry(&sources)?;
    Ok((sources, canonical_map))
}

pub fn normalize_title(title: &str) -> String {
    let mut output = String::new();
    let mut pending_space = false;
    for ch in title.chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() {
            if pending_space && !output.is_empty() {
                output.push(' ');
            }
            pending_space = false;
            output.push(ch);
        } else {
            pending_space = true;
        }
    }
    if output.is_empty() {
        "untitled".to_string()
    } else {
        output
    }
}

pub fn normalized_source_key(source: &SourceEntry) -> String {
    if let Some(url) = source
        .url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
    {
        let normalized = url.trim_end_matches('/').to_ascii_lowercase();
        return format!("url:{normalized}");
    }
    format!("title:{}", normalize_title(&source.title))
}

/// Merge two source entries, preferring non-empty fields from the incoming entry.
pub fn merge_source_entries(existing: &SourceEntry, incoming: &SourceEntry) -> SourceEntry {
    merge_source(existing, incoming)
}

pub(crate) fn merge_source(existing: &SourceEntry, incoming: &SourceEntry) -> SourceEntry {
    SourceEntry {
        title: if incoming.title.trim().is_empty() {
            existing.title.clone()
        } else {
            incoming.title.clone()
        },
        url: incoming
            .url
            .clone()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| existing.url.clone()),
        authors: if incoming.authors.is_empty() {
            existing.authors.clone()
        } else {
            incoming.authors.clone()
        },
        publisher: incoming
            .publisher
            .clone()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| existing.publisher.clone()),
        date: incoming
            .date
            .clone()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| existing.date.clone()),
        accessed: incoming
            .accessed
            .clone()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| existing.accessed.clone()),
        note: incoming
            .note
            .clone()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| existing.note.clone()),
        excerpt: incoming
            .excerpt
            .clone()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| existing.excerpt.clone()),
    }
}

pub(crate) fn validate_source_registry(
    sources: &IndexMap<SourceId, SourceEntry>,
) -> Result<(), StoreError> {
    let mut seen = BTreeMap::new();
    for (source_id, source) in sources {
        let key = normalized_source_key(source);
        if let Some(existing) = seen.insert(key.clone(), source_id.clone()) {
            return Err(StoreError::SourceConflict {
                source_id: source_id.clone(),
                normalized_key: key,
                canonical_id: existing,
            });
        }
    }
    Ok(())
}
