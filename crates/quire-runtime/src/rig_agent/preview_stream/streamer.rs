use super::*;

impl PreviewState {
    pub(crate) async fn append_arguments(&mut self, part: usize, fragment: &str) {
        let Some(stream) = self.streams.get_mut(&part) else {
            return;
        };
        stream.buffer.push_str(fragment);
        let Some(doc_id) = current_doc_id(&self.doc_id) else {
            return;
        };
        let arguments = Value::Object(parse_partial_arguments(&stream.buffer));
        let blocks = extract_preview_blocks(&arguments);
        for (index, candidate) in blocks.iter().enumerate() {
            if !candidate.anchor_resolved {
                continue;
            }
            if !stream.streamers.contains_key(&index) {
                let block_id = candidate
                    .block_id
                    .clone()
                    .unwrap_or_else(|| format!("preview{part}{index}"));
                let key = candidate
                    .block_id
                    .clone()
                    .unwrap_or_else(|| format!("auto-{index}"));
                let preview_id = format!(
                    "{}:{}:{}:{}:{}",
                    self.run_id, stream.tool_name, stream.invocation_id, part, key
                );
                stream.streamers.insert(
                    index,
                    ToolPreviewStreamer::new(
                        &self.base_url,
                        self.token.clone(),
                        &doc_id,
                        &preview_id,
                        &self.actor,
                        &block_id,
                        self.throttle_ms,
                    )
                    .with_anchor(candidate.anchor.clone())
                    .with_report_store(self.report_store.clone()),
                );
            }
            if let Some(streamer) = stream.streamers.get_mut(&index) {
                streamer.block_id = candidate
                    .block_id
                    .clone()
                    .unwrap_or_else(|| format!("preview{part}{index}"));
                if streamer.anchor != candidate.anchor {
                    streamer.set_anchor(candidate.anchor.clone());
                }
                let block_id = BlockId::parse(
                    candidate
                        .block_id
                        .clone()
                        .unwrap_or_else(|| format!("preview{part}{index}")),
                )
                .unwrap_or_else(|_| BlockId::pending());
                match streamer.push_block(candidate.partial(block_id)) {
                    Ok(_) => {
                        stream.preview_ids.insert(preview_id_for(streamer));
                    }
                    Err(error) => {
                        tracing::warn!(%error, tool = %stream.tool_name, "ephemeral preview publish failed");
                    }
                }
            }
        }
    }

    pub(crate) async fn finish_tool_call(
        &mut self,
        _turn: usize,
        call_id: &str,
        tool_name: &str,
        arguments: &Value,
    ) {
        let target_block_ids: BTreeSet<String> = extract_preview_blocks(arguments)
            .into_iter()
            .filter_map(|block| block.block_id)
            .collect();
        let matched_by_blocks = (!target_block_ids.is_empty())
            .then(|| {
                self.streams
                    .iter()
                    .filter(|(_, stream)| stream.tool_name == tool_name)
                    .filter_map(|(part, stream)| {
                        let overlap = stream
                            .streamers
                            .values()
                            .filter(|streamer| target_block_ids.contains(&streamer.block_id))
                            .count();
                        (overlap > 0).then_some((*part, overlap))
                    })
                    .max_by(|(left_part, left_overlap), (right_part, right_overlap)| {
                        left_overlap
                            .cmp(right_overlap)
                            .then_with(|| right_part.cmp(left_part))
                    })
                    .map(|(part, _)| part)
            })
            .flatten();
        let part = matched_by_blocks.or_else(|| {
            self.streams
                .iter()
                .filter(|(_, stream)| stream.tool_name == tool_name)
                .map(|(part, _)| *part)
                .min()
        });
        if let Some(part) = part {
            self.finalize_part(part, call_id, arguments).await;
        }
    }

    async fn finalize_part(&mut self, part: usize, call_id: &str, arguments: &Value) {
        let Some(mut stream) = self.streams.remove(&part) else {
            return;
        };
        let Some(doc_id) = current_doc_id(&self.doc_id) else {
            return;
        };
        // The model may emit `blocks` before `target_id`/`section_id`/`type`. Frames sent
        // while those fields are missing carry the provisional `AppendToDoc` anchor. Once
        // the complete arguments arrive, republish every block with its resolved anchor and
        // give the frontend one paint window before the commit clears the previews.
        let mut corrected = false;
        let final_blocks = extract_preview_blocks(arguments);
        for (index, candidate) in final_blocks.iter().enumerate() {
            if !candidate.anchor_resolved {
                continue;
            }
            if !stream.streamers.contains_key(&index) {
                let block_id = candidate
                    .block_id
                    .clone()
                    .unwrap_or_else(|| format!("preview{part}{index}"));
                let key = candidate
                    .block_id
                    .clone()
                    .unwrap_or_else(|| format!("auto-{index}"));
                let preview_id = format!(
                    "{}:{}:{}:{}:{}",
                    self.run_id, stream.tool_name, stream.invocation_id, part, key
                );
                stream.streamers.insert(
                    index,
                    ToolPreviewStreamer::new(
                        &self.base_url,
                        self.token.clone(),
                        &doc_id,
                        &preview_id,
                        &self.actor,
                        &block_id,
                        self.throttle_ms,
                    )
                    .with_anchor(candidate.anchor.clone())
                    .with_report_store(self.report_store.clone()),
                );
                corrected = true;
            }
            let Some(streamer) = stream.streamers.get_mut(&index) else {
                continue;
            };
            streamer.block_id = candidate
                .block_id
                .clone()
                .unwrap_or_else(|| format!("preview{part}{index}"));
            if streamer.anchor != candidate.anchor {
                streamer.set_anchor(candidate.anchor.clone());
                corrected = true;
            }
            let block_id = BlockId::parse(
                candidate
                    .block_id
                    .clone()
                    .unwrap_or_else(|| format!("preview{part}{index}")),
            )
            .unwrap_or_else(|_| BlockId::pending());
            match streamer.push_block_now(candidate.partial(block_id)).await {
                Ok(_) => {
                    stream.preview_ids.insert(preview_id_for(streamer));
                }
                Err(error) => {
                    tracing::warn!(%error, tool = %stream.tool_name, "ephemeral preview publish failed");
                }
            }
        }
        if corrected {
            tokio::time::sleep(PREVIEW_ANCHOR_SETTLE).await;
        }
        if !stream.preview_ids.is_empty() {
            self.previews_by_call.insert(
                call_id.to_string(),
                stream.preview_ids.into_iter().collect(),
            );
        }
    }

    pub(crate) fn take_preview_ids(&mut self, call_id: &str) -> Option<Vec<String>> {
        let ids = self.previews_by_call.remove(call_id)?;
        self.dispatched_previews
            .insert(call_id.to_string(), ids.clone());
        Some(ids)
    }

    #[cfg(test)]
    pub(crate) fn for_test(run_id: &str, doc_id: SharedDocId) -> Self {
        Self {
            run_id: run_id.to_string(),
            doc_id,
            report_store: None,
            base_url: String::new(),
            token: SecretString::from(String::new()),
            actor: "test".into(),
            throttle_ms: 0,
            streams: HashMap::new(),
            previews_by_call: HashMap::new(),
            dispatched_previews: HashMap::new(),
        }
    }
}

pub(crate) fn inject_preview_ids(arguments: &mut Value, preview_ids: &[String]) {
    if preview_ids.is_empty() {
        return;
    }
    let Some(object) = arguments.as_object_mut() else {
        return;
    };
    let mut merged: Vec<String> = object
        .get("preview_ids")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    for preview_id in preview_ids {
        if !merged.iter().any(|existing| existing == preview_id) {
            merged.push(preview_id.clone());
        }
    }
    let merged = serde_json::to_value(merged).unwrap_or_else(|error| {
        warn!(%error, "failed to serialize merged preview IDs");
        Value::Array(Vec::new())
    });
    object.insert("preview_ids".to_string(), merged);
}
