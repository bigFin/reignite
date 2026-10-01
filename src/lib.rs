//! Recovery policy and standalone inspection. Never persist commands or prompts.
pub mod codex_probe;
pub mod discovery;
pub mod eligibility;
pub mod inspection;
pub mod pi_delivery;
mod pi_evidence;
pub mod pi_gate;
pub mod pi_handoff;
mod pi_rpc;
pub mod policy;
use anyhow::{Context, Result, bail, ensure};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

pub const CONTINUATION: &str = "The host restarted while this opted-in session was active. Inspect the current repository and session history and verify interrupted tool outcomes before continuing the authorized task. Unknown tool delivery or side effects may already have happened. Do not blindly replay deployment, API, payment, or other side-effecting commands. Preserve existing approval and authorization boundaries; ask the user when authorization or outcomes are uncertain. This is one recovery attempt, not a guarantee of exactly-once execution.";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub harness: String,
    pub session_id: String,
    pub session_file: PathBuf,
    pub cwd: PathBuf,
    pub leaf: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Owner {
    pub boot: String,
    pub pid: u32,
    pub start: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attempt {
    pub id: String,
    pub boot: String,
    pub expires: u64,
    pub status: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub identity: Identity,
    pub device: u64,
    pub inode: u64,
    pub owner: Owner,
    pub enabled: bool,
    pub activity: Activity,
    pub shutdown_ambiguous: bool,
    pub attempt: Option<Attempt>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    Idle,
    Busy,
    Waiting,
    Stopped,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Database {
    schema: u32,
    policy: policy::Policy,
    #[serde(deserialize_with = "unique_map")]
    records: BTreeMap<String, Record>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyDatabase {
    schema: u32,
    #[serde(deserialize_with = "unique_map")]
    records: BTreeMap<String, Record>,
}
// Duplicate map keys must not silently replace disables or uncertain attempts.
pub(crate) fn unique_map<'de, D, V>(
    deserializer: D,
) -> std::result::Result<BTreeMap<String, V>, D::Error>
where
    D: serde::Deserializer<'de>,
    V: Deserialize<'de>,
{
    struct Visitor<V>(std::marker::PhantomData<V>);
    impl<'de, V: Deserialize<'de>> serde::de::Visitor<'de> for Visitor<V> {
        type Value = BTreeMap<String, V>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a map with unique state keys")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut input: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut map = BTreeMap::new();
            while let Some((key, value)) = input.next_entry()? {
                if map.insert(key, value).is_some() {
                    return Err(serde::de::Error::custom("duplicate state key"));
                }
            }
            Ok(map)
        }
    }
    deserializer.deserialize_map(Visitor(std::marker::PhantomData))
}
#[derive(Deserialize)]
#[serde(untagged)]
enum StoredDatabase {
    Current(Database),
    Legacy(LegacyDatabase),
}
impl StoredDatabase {
    fn into_current(self) -> Result<Database> {
        match self {
            Self::Current(db) => {
                ensure!(db.schema == 2, "unknown state schema");
                Ok(db)
            }
            Self::Legacy(legacy) => {
                ensure!(legacy.schema == 1, "unknown state schema");
                let mut policy = policy::Policy::default();
                for record in legacy.records.values() {
                    if !record.enabled || record.activity == Activity::Stopped {
                        policy.disable(
                            &record.identity,
                            policy::DisableReason::LegacyDisabledOrStopped,
                        );
                    }
                }
                // Keep schema 1 until a successful mutation commits the migration.
                Ok(Database {
                    schema: 1,
                    policy,
                    records: legacy.records,
                })
            }
        }
    }
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Open {
        identity: Identity,
        pid: u32,
        ticket: Option<String>,
    },
    Enable {
        identity: Identity,
        pid: u32,
    },
    Observe {
        identity: Identity,
        pid: u32,
        activity: Activity,
    },
    Shutdown {
        identity: Identity,
        pid: u32,
    },
    Accepted {
        identity: Identity,
        pid: u32,
        attempt: String,
    },
    Disable {
        session_file: PathBuf,
    },
    Status,
    PolicyStatus {
        session_file: Option<PathBuf>,
    },
    DiscoverPi {
        #[serde(default)]
        sessions_roots: Vec<PathBuf>,
    },
    AssessPi {
        session_file: PathBuf,
        subagents_root: Option<PathBuf>,
        usage_log: Option<PathBuf>,
    },
    SetHostPolicy {
        enabled: bool,
    },
    ClearDisable {
        session_file: PathBuf,
    },
    Recover {
        session_file: PathBuf,
        dry_run: bool,
    },
}

pub fn boot_id() -> Result<String> {
    let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id")?
        .trim()
        .to_owned();
    Uuid::parse_str(&boot).context("invalid Linux boot identity")?;
    Ok(boot)
}
fn now() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}
fn process_start(pid: u32) -> Result<String> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let fields: Vec<_> = stat
        .rsplit_once(')')
        .context("invalid process stat")?
        .1
        .split_whitespace()
        .collect();
    ensure!(fields.first() != Some(&"Z"), "owner is a zombie");
    Ok(fields
        .get(19)
        .context("missing process start time")?
        .to_string())
}
fn owner(pid: u32, boot: &str, cwd: &Path) -> Result<Owner> {
    ensure!(pid > 1, "invalid owner pid");
    ensure!(
        fs::metadata(format!("/proc/{pid}"))?.uid() == unsafe { libc::geteuid() },
        "foreign owner"
    );
    ensure!(
        fs::canonicalize(format!("/proc/{pid}/cwd"))? == cwd,
        "owner cwd mismatch"
    );
    Ok(Owner {
        boot: boot.to_owned(),
        pid,
        start: process_start(pid)?,
    })
}
fn live(o: &Owner, boot: &str) -> bool {
    o.boot == boot && process_start(o.pid).is_ok_and(|s| s == o.start)
}
fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
}
fn canonical(path: &Path) -> Result<PathBuf> {
    ensure!(path.is_absolute(), "path must be absolute");
    let p = fs::canonicalize(path)?;
    ensure!(
        p == path,
        "path must be canonical (no symlinks or traversal)"
    );
    Ok(p)
}
fn key(path: &Path) -> String {
    format!("{:x}", Sha256::digest(path.as_os_str().as_encoded_bytes()))
}
fn validate(i: &Identity) -> Result<(u64, u64)> {
    ensure!(i.harness == "pi", "unsupported harness");
    ensure!(
        identifier(&i.session_id) && i.leaf.as_ref().is_none_or(|l| identifier(l)),
        "invalid identifier"
    );
    let session = inspection::registered_session(i)?;
    ensure!(
        session.native_id == i.session_id && session.cwd == i.cwd,
        "session header identity mismatch"
    );
    Ok((session.device, session.inode))
}
fn matches(r: &Record, i: &Identity, inode: (u64, u64), branch: bool) -> Result<()> {
    ensure!(
        r.identity.harness == i.harness
            && r.identity.session_id == i.session_id
            && r.identity.session_file == i.session_file
            && r.identity.cwd == i.cwd
            && (r.device, r.inode) == inode,
        "registered identity mismatch"
    );
    ensure!(
        !branch || r.identity.leaf == i.leaf,
        "branch identity mismatch"
    );
    Ok(())
}
fn eligible(r: &Record, boot: &str) -> Result<()> {
    ensure!(r.enabled, "disabled");
    ensure!(r.activity == Activity::Busy, "not active: {:?}", r.activity);
    ensure!(
        !r.shutdown_ambiguous,
        "ambiguous quit/shutdown requires manual action"
    );
    ensure!(r.owner.boot != boot, "no host boot change");
    ensure!(!live(&r.owner, boot), "owner still live");
    ensure!(
        r.attempt.is_none(),
        "automatic attempt already authorized/claimed; no automatic retry"
    );
    Ok(())
}
fn private(path: &Path, dir: bool) -> Result<()> {
    let m = fs::symlink_metadata(path)?;
    ensure!(
        !m.file_type().is_symlink()
            && m.uid() == unsafe { libc::geteuid() }
            && m.mode() & 0o077 == 0
            && if dir { m.is_dir() } else { m.is_file() },
        "state path must be private and owned: {}",
        path.display()
    );
    Ok(())
}

pub struct Store {
    dir: PathBuf,
}
impl Store {
    pub fn new(dir: PathBuf) -> Result<Self> {
        ensure!(dir.is_absolute(), "state directory must be absolute");
        if !dir.exists() {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&dir)?;
        }
        private(&dir, true)?;
        canonical(&dir)?;
        Ok(Self { dir })
    }
    /// Execute against the kernel's actual boot observation. No CLI boot override exists.
    pub fn execute(&self, request: Request) -> Result<Value> {
        self.execute_observed(request, &boot_id()?)
    }
    // Transitional transport authority only. Native eligibility is not inferred
    // from this snapshot; Open revalidates policy/ticket atomically at claim time.
    pub(crate) fn pi_ticket_scope(&self, path: &Path, ticket: &str) -> Result<Record> {
        Uuid::parse_str(ticket).context("invalid restore ticket")?;
        let snapshot = self.execute(Request::Status)?;
        let policy: policy::Policy = serde_json::from_value(snapshot["policy"].clone())?;
        let record: Record = snapshot["records"].as_array().context("missing records")?
            .iter().filter_map(|record| serde_json::from_value::<Record>(record.clone()).ok())
            .find(|record| record.identity.session_file == path)
            .context("no registered legacy restore context; native automatic recovery is not implemented")?;
        ensure!(
            policy.blocked_reason(&record.identity).is_none(),
            "restore blocked by host/session policy"
        );
        let boot = boot_id()?;
        ensure!(
            record.enabled
                && record.activity == Activity::Busy
                && !record.shutdown_ambiguous
                && record.owner.boot != boot
                && !live(&record.owner, &boot),
            "legacy restore no longer eligible"
        );
        let attempt = record
            .attempt
            .as_ref()
            .context("missing restore authorization")?;
        ensure!(
            attempt.id == ticket
                && attempt.boot == boot
                && attempt.status == "authorized"
                && attempt.expires >= now()?,
            "stale/consumed/mismatched restore ticket"
        );
        Ok(record)
    }
    fn execute_observed(&self, request: Request, boot: &str) -> Result<Value> {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(self.dir.join("lock"))?;
        private(&self.dir.join("lock"), false)?;
        lock.lock_exclusive()?;
        let path = self.dir.join("state.json");
        let mut db = match fs::symlink_metadata(&path) {
            Ok(_) => {
                private(&path, false)?;
                let f = OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_NOFOLLOW)
                    .open(&path)?;
                ensure!(f.metadata()?.len() <= 16 * 1024 * 1024, "state too large");
                let mut bytes = Vec::new();
                f.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
                ensure!(
                    bytes.len() <= 16 * 1024 * 1024,
                    "state grew beyond byte limit"
                );
                serde_json::from_slice::<StoredDatabase>(&bytes)
                    .context("corrupt/unknown state; refusing recovery")?
                    .into_current()?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Database {
                schema: 2,
                policy: policy::Policy::default(),
                records: BTreeMap::new(),
            },
            Err(error) => return Err(error.into()),
        };
        db.policy.validate()?;
        for (k, r) in &db.records {
            ensure!(
                k == &key(&r.identity.session_file)
                    && r.identity.harness == "pi"
                    && r.identity.session_file.is_absolute()
                    && r.identity.cwd.is_absolute()
                    && identifier(&r.identity.session_id)
                    && r.identity.leaf.as_deref().is_none_or(identifier)
                    && Uuid::parse_str(&r.owner.boot).is_ok(),
                "corrupt record identity"
            );
            if let Some(a) = &r.attempt {
                ensure!(
                    Uuid::parse_str(&a.id).is_ok()
                        && Uuid::parse_str(&a.boot).is_ok()
                        && ["authorized", "claimed", "accepted"].contains(&a.status.as_str()),
                    "corrupt attempt"
                );
            }
        }
        let (result, changed) = apply(&mut db, request, boot)?;
        if changed {
            db.schema = 2;
            let mut bytes = serde_json::to_vec(&db)?;
            bytes.push(b'\n');
            ensure!(
                bytes.len() <= 16 * 1024 * 1024,
                "state/migration exceeds byte limit; original retained"
            );
            let tmp = self.dir.join(format!(".state-{}", Uuid::new_v4()));
            let write = (|| -> Result<()> {
                let mut f = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&tmp)?;
                f.write_all(&bytes)?;
                f.sync_all()?;
                fs::rename(&tmp, &path)?;
                File::open(&self.dir)?.sync_all()?;
                Ok(())
            })();
            if write.is_err() {
                let _ = fs::remove_file(&tmp);
            }
            write?;
        }
        Ok(result)
    }
}
fn apply(db: &mut Database, req: Request, boot: &str) -> Result<(Value, bool)> {
    match req {
        Request::Status => Ok((
            json!({"records": db.records.values().collect::<Vec<_>>(), "boot":boot,
                "policy":db.policy,"state_schema":db.schema,"migration_pending":db.schema == 1}),
            false,
        )),
        Request::PolicyStatus { session_file } => {
            let identity = session_file
                .as_deref()
                .map(inspection::native_identity)
                .transpose()?;
            let mut legacy_blockers = Vec::new();
            let mut legacy_attempts = Vec::new();
            if let Some(identity) = &identity {
                for record in db.records.values().filter(|record| {
                    record.identity.harness == identity.harness
                        && record.identity.session_id == identity.session_id
                }) {
                    if !record.enabled {
                        legacy_blockers.push("legacy_session_disabled");
                    }
                    if record.activity == Activity::Stopped {
                        legacy_blockers.push("legacy_session_stopped");
                    }
                    if record.shutdown_ambiguous {
                        legacy_blockers.push("legacy_shutdown_ambiguous");
                    }
                    if let Some(attempt) = &record.attempt {
                        legacy_blockers.push("legacy_attempt_retained");
                        legacy_attempts.push(attempt);
                    }
                }
            }
            legacy_blockers.sort_unstable();
            legacy_blockers.dedup();
            Ok((
                json!({"policy":db.policy,"session":identity,
                "policy_enabled_for_session":identity.as_ref().map(|identity| db.policy.blocked_reason(identity).is_none()),
                "legacy_blockers":legacy_blockers,"legacy_attempts":legacy_attempts,
                "eligible":false,"native_automatic_recovery_implemented":false,
                "state_schema":db.schema,"migration_pending":db.schema == 1}),
                false,
            ))
        }
        Request::DiscoverPi { sessions_roots } => {
            let roots = if sessions_roots.is_empty() {
                discovery::default_roots()?
            } else {
                sessions_roots
            };
            let report = discovery::discover(roots, &db.policy, &db.records, boot, db.schema)?;
            Ok((serde_json::to_value(report)?, false))
        }
        Request::AssessPi {
            session_file,
            subagents_root,
            usage_log,
        } => {
            let (inspection, lineage) =
                inspection::inspect_for_assessment(&session_file, subagents_root.as_deref())?;
            let usage = pi_gate::usage(usage_log.as_deref(), &inspection.session)?;
            let report = eligibility::assess(
                inspection,
                lineage,
                usage,
                &db.policy,
                db.records.values(),
                boot,
                db.schema,
            );
            Ok((serde_json::to_value(report)?, false))
        }
        Request::SetHostPolicy { enabled } => {
            db.policy.host_enabled = enabled;
            Ok((json!({"policy":db.policy,"attempts_reset":false}), true))
        }
        Request::ClearDisable { session_file } => {
            let identity = inspection::native_identity(&session_file)?;
            let cleared = db.policy.clear_disable(&identity);
            Ok((
                json!({"cleared":cleared,"session":identity,"attempts_reset":false,
                "legacy_records_changed":false}),
                true,
            ))
        }
        Request::Disable { session_file } => {
            let identity = inspection::native_identity(&session_file)?;
            db.policy
                .disable(&identity, policy::DisableReason::Explicit);
            // Block all retained aliases of the native session without erasing attempts.
            for record in db.records.values_mut().filter(|record| {
                record.identity.harness == identity.harness
                    && record.identity.session_id == identity.session_id
            }) {
                record.enabled = false;
                record.activity = Activity::Stopped;
            }
            Ok((
                json!({"disabled":db.policy.disabled(&identity),
                "record":db.records.get(&key(&session_file)),"attempts_reset":false}),
                true,
            ))
        }
        Request::Recover {
            session_file,
            dry_run,
        } => {
            let identity = inspection::native_identity(&session_file)?;
            let policy_reason = db.policy.blocked_reason(&identity).map(str::to_owned);
            let Some(r) = db.records.get_mut(&key(&session_file)) else {
                let reason = policy_reason.unwrap_or_else(|| "native automatic recovery is not implemented; lifecycle and ownership evidence missing".into());
                ensure!(dry_run, "{reason}");
                return Ok((
                    json!({"eligible":false,"blocked_reason":reason,"identity":identity,
                        "eligibility_scope":"native_unimplemented","native_eligibility_verified":false}),
                    false,
                ));
            };
            matches(r, &r.identity, validate(&r.identity)?, true)?;
            let reason = policy_reason.or_else(|| eligible(r, boot).err().map(|e| e.to_string()));
            if dry_run {
                return Ok((
                    json!({"eligible":reason.is_none(), "blocked_reason":reason, "record":r,
                        "eligibility_scope":"legacy_prototype","native_eligibility_verified":false}),
                    false,
                ));
            }
            if let Some(reason) = reason {
                bail!(reason);
            }
            let time = now()?; // After locking and validating the source.
            let a = Attempt {
                id: Uuid::new_v4().to_string(),
                boot: boot.to_owned(),
                expires: time + 120,
                status: "authorized".into(),
            };
            let ticket = a.id.clone();
            r.attempt = Some(a);
            Ok((json!({"ticket":ticket,"expires":time+120}), true))
        }
        Request::Open {
            identity,
            pid,
            ticket,
        } => {
            if ticket.is_some() {
                ensure!(
                    db.policy.blocked_reason(&identity).is_none(),
                    "restore blocked by host/session policy"
                );
            }
            let inode = validate(&identity)?;
            let o = owner(pid, boot, &identity.cwd)?;
            let k = key(&identity.session_file);
            let mut attempt = None;
            let mut claimed = None;
            if let Some(r) = db.records.get_mut(&k) {
                matches(r, &identity, inode, ticket.is_some())?;
                ensure!(
                    !live(&r.owner, boot) || r.owner == o,
                    "session owned by another live process"
                );
                if let Some(ticket) = &ticket {
                    ensure!(
                        r.enabled
                            && r.activity == Activity::Busy
                            && !r.shutdown_ambiguous
                            && r.owner.boot != boot,
                        "restore no longer eligible"
                    );
                    let a = r
                        .attempt
                        .as_mut()
                        .context("missing restore authorization")?;
                    ensure!(
                        a.id == *ticket
                            && a.boot == boot
                            && a.expires >= now()?
                            && a.status == "authorized",
                        "stale/consumed/mismatched ticket"
                    );
                    a.status = "claimed".into();
                    claimed = Some(a.id.clone());
                }
                attempt = r.attempt.clone();
            } else {
                ensure!(ticket.is_none(), "unregistered restore ticket");
            }
            let r = Record {
                identity,
                device: inode.0,
                inode: inode.1,
                owner: o,
                enabled: claimed.is_some(),
                activity: if claimed.is_some() {
                    Activity::Busy
                } else {
                    Activity::Idle
                },
                shutdown_ambiguous: false,
                attempt,
            };
            db.records.insert(k, r.clone());
            Ok((
                json!({"record":r,"attempt":claimed,"continuation":claimed.as_ref().map(|_| CONTINUATION)}),
                true,
            ))
        }
        Request::Enable { identity, pid } => {
            ensure!(db.policy.host_enabled, "host recovery disabled");
            let record = {
                let r = owned(db, &identity, pid, boot)?;
                ensure!(
                    r.activity != Activity::Busy && r.activity != Activity::Waiting,
                    "enable only when idle/stopped"
                );
                r.enabled = true;
                r.activity = Activity::Idle;
                r.shutdown_ambiguous = false;
                r.attempt = None;
                r.identity.leaf = identity.leaf.clone();
                r.clone()
            };
            // The legacy explicit enable command retains its documented manual reset.
            // New host-policy/clear-disable commands never perform this reset.
            db.policy.clear_disable(&identity);
            Ok((json!({"record":record}), true))
        }
        Request::Observe {
            identity,
            pid,
            activity,
        } => {
            let r = owned(db, &identity, pid, boot)?;
            r.identity.leaf = identity.leaf;
            if activity == Activity::Stopped {
                r.enabled = false;
            }
            // An external disable must not be overwritten by subsequent lifecycle observations.
            r.activity = if r.enabled {
                activity
            } else {
                Activity::Stopped
            };
            let record = r.clone();
            if activity == Activity::Stopped {
                db.policy.disable(
                    &record.identity,
                    policy::DisableReason::LegacyDisabledOrStopped,
                );
            }
            Ok((json!({"record":record}), true))
        }
        Request::Shutdown { identity, pid } => {
            let r = owned(db, &identity, pid, boot)?;
            r.shutdown_ambiguous = true;
            Ok((json!({"record":r}), true))
        }
        Request::Accepted {
            identity,
            pid,
            attempt,
        } => {
            let r = owned(db, &identity, pid, boot)?;
            let a = r.attempt.as_mut().context("no attempt")?;
            ensure!(a.id == attempt && a.status == "claimed", "attempt mismatch");
            a.status = "accepted".into();
            Ok((json!({"record":r}), true))
        }
    }
}
fn owned<'a>(db: &'a mut Database, i: &Identity, pid: u32, boot: &str) -> Result<&'a mut Record> {
    let inode = validate(i)?;
    let r = db
        .records
        .get_mut(&key(&i.session_file))
        .context("unregistered session")?;
    matches(r, i, inode, false)?;
    ensure!(
        r.owner == owner(pid, boot, &i.cwd)?,
        "owner identity mismatch"
    );
    Ok(r)
}
