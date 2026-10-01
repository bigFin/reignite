//! Host policy and native-session disable overrides. No lifecycle or execution claims.
use crate::{Identity, identifier};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub host_enabled: bool,
    #[serde(deserialize_with = "crate::unique_map")]
    pub disabled_sessions: BTreeMap<String, SessionDisable>,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            host_enabled: true,
            disabled_sessions: BTreeMap::new(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionDisable {
    pub harness: String,
    pub session_id: String,
    /// Provenance only: disable follows the native ID even if a file is moved.
    pub last_known_file: PathBuf,
    pub last_known_cwd: PathBuf,
    pub reason: DisableReason,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisableReason {
    Explicit,
    LegacyDisabledOrStopped,
}
fn key(identity: &Identity) -> String {
    format!("{}:{}", identity.harness, identity.session_id)
}
impl Policy {
    pub(crate) fn validate(&self) -> Result<()> {
        for (key, disabled) in &self.disabled_sessions {
            ensure!(
                disabled.harness == "pi"
                    && identifier(&disabled.session_id)
                    && disabled.last_known_file.is_absolute()
                    && disabled.last_known_cwd.is_absolute()
                    && *key == format!("{}:{}", disabled.harness, disabled.session_id),
                "corrupt session disable identity"
            );
        }
        Ok(())
    }
    pub(crate) fn disable(&mut self, identity: &Identity, reason: DisableReason) {
        self.disabled_sessions.insert(
            key(identity),
            SessionDisable {
                harness: identity.harness.clone(),
                session_id: identity.session_id.clone(),
                last_known_file: identity.session_file.clone(),
                last_known_cwd: identity.cwd.clone(),
                reason,
            },
        );
    }
    pub(crate) fn clear_disable(&mut self, identity: &Identity) -> bool {
        self.disabled_sessions.remove(&key(identity)).is_some()
    }
    pub(crate) fn disabled(&self, identity: &Identity) -> Option<&SessionDisable> {
        self.disabled_sessions.get(&key(identity))
    }
    pub(crate) fn blocked_reason(&self, identity: &Identity) -> Option<&'static str> {
        if !self.host_enabled {
            Some("host recovery disabled")
        } else if self.disabled(identity).is_some() {
            Some("native session recovery disabled")
        } else {
            None
        }
    }
}
