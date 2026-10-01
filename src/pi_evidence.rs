//! Bounded metadata projection of RAW persisted ancestry, not Pi's runtime context.
//! Never interpret prompts, summaries, custom state or tool arguments as authority.
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const MAX_ENTRIES: usize = 65_536;
const MAX_TOOL_REFERENCES: usize = 65_536;

#[derive(Debug, Default, Serialize)]
pub struct PersistedLineage {
    pub selection: &'static str,
    pub leaf: Option<String>,
    pub entries: usize,
    pub last_message_entry: Option<String>,
    pub last_message_role: Option<&'static str>,
    pub last_assistant_stop_reason: Option<&'static str>,
    pub unmatched_tool_calls: usize,
    pub unmatched_tool_results: usize,
    pub ambiguous_tool_calls: usize,
    pub failed_or_unknown_tool_results: usize,
    pub opaque_entries: usize,
    pub system_loadout_reported: bool,
    /// A user-message segment on the reported ancestry, NOT an authorized recovery episode.
    pub reported_work: ReportedWork,
}
#[derive(Debug, Default, Serialize)]
pub struct ReportedWork {
    pub user_entry: Option<String>,
    pub last_assistant_entry: Option<String>,
    pub last_assistant_stop_reason: Option<&'static str>,
    pub aborted_messages: usize,
    pub error_messages: usize,
}
#[derive(Default)]
pub(crate) struct Index {
    entries: Vec<Entry>,
    positions: BTreeMap<String, usize>,
    tool_references: usize,
}
struct Entry {
    id: String,
    parent: Option<usize>,
    role: Option<&'static str>,
    stop: Option<&'static str>,
    calls: Vec<String>,
    result: Option<String>,
    result_ok: bool,
    opaque: bool,
    loadout: bool,
}
fn tool_id(value: &Value) -> Result<String> {
    let id = value
        .as_str()
        .context("unsupported native tool reference")?;
    ensure!(
        !id.is_empty() && id.len() <= 256,
        "native tool reference limit exceeded"
    );
    Ok(id.to_owned())
}
impl Index {
    pub fn observe(&mut self, value: &Value) -> Result<()> {
        ensure!(
            self.entries.len() < MAX_ENTRIES,
            "native assessment entry limit exceeded"
        );
        let id = value["id"].as_str().context("entry has no ID")?;
        ensure!(
            crate::identifier(id) && !self.positions.contains_key(id),
            "invalid/duplicate native entry ID"
        );
        let parent = match value.get("parentId") {
            Some(Value::Null) => None,
            Some(Value::String(parent)) => Some(
                *self
                    .positions
                    .get(parent)
                    .context("native ancestry missing, forward or cyclic")?,
            ),
            _ => anyhow::bail!("native ancestry missing or unsupported"),
        };
        let mut entry = Entry {
            id: id.to_owned(),
            parent,
            role: None,
            stop: None,
            calls: Vec::new(),
            result: None,
            result_ok: false,
            opaque: false,
            loadout: false,
        };
        match value["type"].as_str() {
            Some("message") => {
                let message = &value["message"];
                match message["role"].as_str() {
                    Some("system") => {
                        entry.loadout = message["sections"].is_object()
                            || message["toolsAdded"].is_array()
                            || message["toolsRemoved"].is_array();
                    }
                    Some("user") => entry.role = Some("user"),
                    Some("assistant") => {
                        entry.role = Some("assistant");
                        entry.stop = match message["stopReason"].as_str() {
                            Some("stop") => Some("stop"),
                            Some("length") => Some("length"),
                            Some("toolUse") => Some("toolUse"),
                            Some("error") => Some("error"),
                            Some("aborted") => Some("aborted"),
                            _ => {
                                entry.opaque = true;
                                None
                            }
                        };
                        if let Some(content) = message["content"].as_array() {
                            for block in content {
                                match block["type"].as_str() {
                                    Some("toolCall") => {
                                        self.tool_references += 1;
                                        ensure!(
                                            self.tool_references <= MAX_TOOL_REFERENCES,
                                            "native assessment tool reference limit exceeded"
                                        );
                                        entry.calls.push(tool_id(&block["id"])?);
                                    }
                                    Some("text" | "thinking" | "image") => {}
                                    _ => entry.opaque = true,
                                }
                            }
                        } else {
                            entry.opaque = true;
                        }
                    }
                    Some("toolResult") => {
                        entry.role = Some("toolResult");
                        self.tool_references += 1;
                        ensure!(
                            self.tool_references <= MAX_TOOL_REFERENCES,
                            "native assessment tool reference limit exceeded"
                        );
                        entry.result = Some(tool_id(&message["toolCallId"])?);
                        entry.result_ok = message["isError"].as_bool() == Some(false);
                    }
                    // Custom/bash/summary messages have harness-specific execution semantics.
                    _ => entry.opaque = true,
                }
            }
            Some("model_change" | "thinking_level_change" | "usage" | "label" | "session_info") => {
            }
            // Compaction, context edits and branch summaries change model context.
            // Do not imitate their projection or read arbitrary summaries as intent.
            _ => entry.opaque = true,
        }
        self.positions.insert(id.to_owned(), self.entries.len());
        self.entries.push(entry);
        Ok(())
    }
    pub fn finish(self) -> PersistedLineage {
        let mut report = PersistedLineage {
            selection: "last_persisted_entry_not_authoritative_active_branch",
            ..Default::default()
        };
        let Some(last) = self.entries.last() else {
            return report;
        };
        report.leaf = Some(last.id.clone());
        let mut path = Vec::new();
        let mut current = Some(self.entries.len() - 1);
        while let Some(position) = current {
            path.push(position);
            current = self.entries[position].parent;
        }
        let mut calls = BTreeSet::new();
        for position in path.into_iter().rev() {
            let entry = &self.entries[position];
            report.entries += 1;
            report.opaque_entries += usize::from(entry.opaque);
            report.system_loadout_reported |= entry.loadout;
            if let Some(role) = entry.role {
                report.last_message_entry = Some(entry.id.clone());
                report.last_message_role = Some(role);
            }
            if entry.role == Some("user") {
                report.reported_work = ReportedWork {
                    user_entry: Some(entry.id.clone()),
                    ..Default::default()
                };
            }
            if entry.role == Some("assistant") {
                report.last_assistant_stop_reason = entry.stop;
                report.reported_work.last_assistant_entry = Some(entry.id.clone());
                report.reported_work.last_assistant_stop_reason = entry.stop;
                report.reported_work.aborted_messages += usize::from(entry.stop == Some("aborted"));
                report.reported_work.error_messages += usize::from(entry.stop == Some("error"));
            }
            for call in &entry.calls {
                if !calls.insert(call) {
                    report.ambiguous_tool_calls += 1;
                }
            }
            if let Some(result) = &entry.result {
                if !calls.remove(result) {
                    report.unmatched_tool_results += 1;
                }
                if !entry.result_ok {
                    report.failed_or_unknown_tool_results += 1;
                }
            }
        }
        report.unmatched_tool_calls = calls.len();
        report
    }
}
