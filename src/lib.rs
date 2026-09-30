//! Durable opt-in policy. Adapters provide observations, never persisted commands or prompts.
use anyhow::{Context, Result, bail, ensure};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Write},
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
    records: BTreeMap<String, Record>,
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
    canonical(&i.cwd)?;
    canonical(&i.session_file)?;
    let f = File::open(&i.session_file)?;
    let m = f.metadata()?;
    ensure!(
        m.is_file() && m.uid() == unsafe { libc::geteuid() },
        "invalid session file owner/type"
    );
    let mut lines = BufReader::new(f).lines();
    let h: Value = serde_json::from_str(&lines.next().context("missing session header")??)?;
    ensure!(
        h["type"] == "session"
            && h["version"] == 3
            && h["id"] == i.session_id
            && h["cwd"].as_str() == i.cwd.to_str(),
        "session header identity mismatch"
    );
    if let Some(leaf) = &i.leaf {
        let mut found = false;
        for line in lines {
            let v: Value = serde_json::from_str(&line?)?;
            if v["id"] == *leaf {
                found = true;
            }
        }
        ensure!(found, "branch leaf absent from session file");
    }
    Ok((m.dev(), m.ino()))
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
        self.execute_observed(request, &boot_id()?, now()?)
    }
    fn execute_observed(&self, request: Request, boot: &str, time: u64) -> Result<Value> {
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
                serde_json::from_reader::<_, Database>(f)
                    .context("corrupt/unknown state; refusing recovery")?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Database {
                schema: 1,
                records: BTreeMap::new(),
            },
            Err(error) => return Err(error.into()),
        };
        ensure!(db.schema == 1, "unknown state schema");
        for (k, r) in &db.records {
            ensure!(
                k == &key(&r.identity.session_file)
                    && identifier(&r.identity.session_id)
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
        let (result, changed) = apply(&mut db, request, boot, time)?;
        if changed {
            let tmp = self.dir.join(format!(".state-{}", Uuid::new_v4()));
            let write = (|| -> Result<()> {
                let mut f = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&tmp)?;
                serde_json::to_writer(&mut f, &db)?;
                f.write_all(b"\n")?;
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
fn apply(db: &mut Database, req: Request, boot: &str, time: u64) -> Result<(Value, bool)> {
    match req {
        Request::Status => Ok((
            json!({"records": db.records.values().collect::<Vec<_>>(), "boot":boot}),
            false,
        )),
        Request::Disable { session_file } => {
            canonical(&session_file)?;
            let r = db
                .records
                .get_mut(&key(&session_file))
                .context("unregistered session")?;
            r.enabled = false;
            r.activity = Activity::Stopped;
            Ok((json!({"record":r}), true))
        }
        Request::Recover {
            session_file,
            dry_run,
        } => {
            canonical(&session_file)?;
            let r = db
                .records
                .get_mut(&key(&session_file))
                .context("unregistered session")?;
            matches(r, &r.identity, validate(&r.identity)?, true)?;
            let reason = eligible(r, boot).err().map(|e| e.to_string());
            if dry_run {
                return Ok((
                    json!({"eligible":reason.is_none(), "blocked_reason":reason, "record":r}),
                    false,
                ));
            }
            if let Some(reason) = reason {
                bail!(reason);
            }
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
                            && a.expires >= time
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
            let r = owned(db, &identity, pid, boot)?;
            ensure!(
                r.activity != Activity::Busy && r.activity != Activity::Waiting,
                "enable only when idle/stopped"
            );
            r.enabled = true;
            r.activity = Activity::Idle;
            r.shutdown_ambiguous = false;
            r.attempt = None;
            r.identity.leaf = identity.leaf;
            Ok((json!({"record":r}), true))
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
            Ok((json!({"record":r}), true))
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
