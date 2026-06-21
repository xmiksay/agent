//! Per-task throwaway PostgreSQL provisioning (issue #26).
//!
//! When `PROJECT_DB_ADMIN_URL` is set, the runner creates a dedicated
//! `LOGIN`-only role + a database it owns at session start, injects the DSN into
//! the agent's env (`DATABASE_URL` + the `PG*` vars) and initial prompt, and
//! guarantees teardown at session end — via a `Drop` guard on every exit path
//! and a startup sweep that mops up orphans a hard SIGKILL skipped.
//!
//! The provisioned DB is a separate throwaway for the *managed* project; it is
//! unrelated to the agent's own control-plane `DATABASE_URL`.

use anyhow::{Context, Result};
use sea_orm::{ConnectionTrait, Database, Statement};
use tracing::{info, warn};
use uuid::Uuid;

/// Deterministic, identifier-safe, ≤63-char name for a task's DB + role. A
/// `Uuid::simple()` is 32 hex chars, so `agent_task_<…>` is 43 chars.
fn task_object_name(task_id: Uuid) -> String {
    format!("agent_task_{}", task_id.simple())
}

/// Prefix all per-task DB/role names share — the unambiguous orphan marker the
/// startup sweep keys on.
const OBJECT_PREFIX: &str = "agent_task_";

/// A provisioned per-task database + role. Drop the matching `ProjectDbGuard` (or
/// call [`ProjectDb::teardown`]) to remove both; teardown is idempotent.
pub struct ProjectDb {
    admin_url: String,
    /// DB + role share this name (`agent_task_<id>`); the role owns the DB.
    name: String,
    host: String,
    port: Option<String>,
    password: String,
    /// DSN the agent connects with. Carries the password, so it stays in the
    /// child env, never logged.
    pub agent_url: String,
}

impl ProjectDb {
    /// Create the throwaway role (`LOGIN` only — no `CREATEDB`/`SUPERUSER`) and a
    /// database it owns. Identifiers are our deterministic names (not user
    /// input); the password is generated hex, so neither needs escaping.
    pub async fn provision(admin_url: &str, agent_host: &str, task_id: Uuid) -> Result<Self> {
        let name = task_object_name(task_id);
        // 64 hex chars of entropy from two v4 UUIDs — avoids a `rand` dependency
        // and contains no characters that need SQL-string escaping.
        let password = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());

        let admin = Database::connect(admin_url)
            .await
            .context("connecting to PROJECT_DB_ADMIN_URL")?;
        let backend = admin.get_database_backend();

        // Drop any stale leftovers from a previous task that reused this id (can't
        // happen with v4 ids, but keeps provision idempotent and self-healing).
        drop_objects(&admin, &name).await;

        admin
            .execute(Statement::from_string(
                backend,
                format!("CREATE ROLE \"{name}\" LOGIN PASSWORD '{password}'"),
            ))
            .await
            .context("creating throwaway role")?;
        admin
            .execute(Statement::from_string(
                backend,
                format!("CREATE DATABASE \"{name}\" OWNER \"{name}\""),
            ))
            .await
            .context("creating throwaway database")?;

        let (host, port) = split_host_port(agent_host);
        let agent_url = match &port {
            Some(p) => format!("postgres://{name}:{password}@{host}:{p}/{name}"),
            None => format!("postgres://{name}:{password}@{host}/{name}"),
        };

        info!(db = %name, host = %host, "provisioned per-task PostgreSQL database");
        Ok(Self {
            admin_url: admin_url.to_string(),
            name,
            host,
            port,
            password,
            agent_url,
        })
    }

    /// DB + role name (`agent_task_<id>`) — also the `PGDATABASE`/`PGUSER` value.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> Option<&str> {
        self.port.as_deref()
    }

    pub fn password(&self) -> &str {
        &self.password
    }

    /// Drop the database (terminating lingering connections via `WITH (FORCE)`)
    /// and the role. Idempotent — both statements are `IF EXISTS` and a failure
    /// is logged, not propagated, since teardown runs on best-effort exit paths.
    pub async fn teardown(&self) {
        match Database::connect(&self.admin_url).await {
            Ok(admin) => drop_objects(&admin, &self.name).await,
            Err(e) => {
                warn!(db = %self.name, error = %e, "teardown: cannot reach admin DB to drop throwaway objects")
            }
        }
    }

    /// Drop every `agent_task_%` database + role left behind by a previous
    /// process that died before its `Drop` guard ran (a hard SIGKILL). Safe to
    /// run at startup: no in-flight task survives a restart (`recover_orphans`
    /// already fails them), so every match is genuinely orphaned.
    pub async fn sweep_orphans(admin_url: &str) -> Result<u64> {
        let admin = Database::connect(admin_url)
            .await
            .context("connecting to PROJECT_DB_ADMIN_URL for sweep")?;
        let backend = admin.get_database_backend();

        let mut names = Vec::new();
        // Databases first, then roles — a role can't be dropped while it owns a DB.
        for row in admin
            .query_all(Statement::from_string(
                backend,
                format!("SELECT datname FROM pg_database WHERE datname LIKE '{OBJECT_PREFIX}%'"),
            ))
            .await
            .context("listing orphan databases")?
        {
            if let Ok(name) = row.try_get::<String>("", "datname") {
                names.push(name);
            }
        }
        for row in admin
            .query_all(Statement::from_string(
                backend,
                format!("SELECT rolname FROM pg_roles WHERE rolname LIKE '{OBJECT_PREFIX}%'"),
            ))
            .await
            .context("listing orphan roles")?
        {
            if let Ok(name) = row.try_get::<String>("", "rolname") {
                names.push(name);
            }
        }

        names.sort();
        names.dedup();
        let count = names.len() as u64;
        for name in names {
            drop_objects(&admin, &name).await;
        }
        if count > 0 {
            info!(count, "swept orphaned per-task PostgreSQL objects");
        }
        Ok(count)
    }
}

/// A scope guard that tears the database down when the runner returns — on `?`,
/// a graceful end, or an abort. `Drop` can't be async, so teardown is spawned;
/// it's idempotent and the startup sweep is the backstop if the process dies
/// before the spawned task runs.
pub struct ProjectDbGuard(pub Option<ProjectDb>);

impl Drop for ProjectDbGuard {
    fn drop(&mut self) {
        if let Some(pdb) = self.0.take() {
            tokio::spawn(async move { pdb.teardown().await });
        }
    }
}

/// Drop a database (with `FORCE` to terminate lingering sessions) and the
/// same-named role. Both `IF EXISTS`, best-effort.
async fn drop_objects(admin: &impl ConnectionTrait, name: &str) {
    let backend = admin.get_database_backend();
    let _ = admin
        .execute(Statement::from_string(
            backend,
            format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)"),
        ))
        .await;
    let _ = admin
        .execute(Statement::from_string(
            backend,
            format!("DROP ROLE IF EXISTS \"{name}\""),
        ))
        .await;
}

/// Split a `host[:port]` into its parts. IPv6 literals aren't supported (the
/// agent connects to a plain host or `host:port`), matching the DSN we build.
fn split_host_port(host: &str) -> (String, Option<String>) {
    match host.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() => {
            (h.to_string(), Some(p.to_string()))
        }
        _ => (host.to_string(), None),
    }
}

/// The `host:port` the agent should connect to: an explicit override, else the
/// host parsed out of the admin DSN.
pub fn agent_host_from_admin(admin_url: &str, override_host: Option<&str>) -> String {
    if let Some(h) = override_host {
        return h.to_string();
    }
    // Parse `scheme://[user[:pass]@]host[:port]/db` → `host[:port]`.
    let after_scheme = admin_url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(admin_url);
    let authority = after_scheme.split('/').next().unwrap_or(after_scheme);
    let host_port = authority
        .rsplit_once('@')
        .map(|(_, rest)| rest)
        .unwrap_or(authority);
    if host_port.is_empty() {
        "localhost".to_string()
    } else {
        host_port.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_name_is_identifier_safe_and_short() {
        let name = task_object_name(Uuid::new_v4());
        assert!(name.starts_with("agent_task_"));
        assert!(name.len() <= 63);
        assert!(name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
    }

    #[test]
    fn agent_host_prefers_override() {
        assert_eq!(
            agent_host_from_admin("postgres://a:b@db:5432/postgres", Some("agent-db:5433")),
            "agent-db:5433"
        );
    }

    #[test]
    fn agent_host_parses_from_admin_url() {
        assert_eq!(
            agent_host_from_admin("postgres://admin:pw@localhost:5432/postgres", None),
            "localhost:5432"
        );
        assert_eq!(
            agent_host_from_admin("postgres://admin@dbhost/postgres", None),
            "dbhost"
        );
    }

    #[test]
    fn split_host_port_splits_only_numeric_port() {
        assert_eq!(
            split_host_port("localhost:5432"),
            ("localhost".to_string(), Some("5432".to_string()))
        );
        assert_eq!(split_host_port("dbhost"), ("dbhost".to_string(), None));
    }

    // ---- DB-backed provisioning (gated on PROJECT_DB_ADMIN_URL) ----
    //
    // Mirrors the `fresh_db` pattern in lifecycle.rs: needs a reachable admin DSN
    // with CREATE ROLE/DATABASE privileges. Skipped when the var is unset.

    fn admin_url() -> Option<String> {
        std::env::var("PROJECT_DB_ADMIN_URL")
            .ok()
            .filter(|v| !v.trim().is_empty())
    }

    #[tokio::test]
    async fn provision_creates_connectable_db_then_teardown_drops_it() {
        let Some(admin) = admin_url() else {
            eprintln!("PROJECT_DB_ADMIN_URL not set; skipping project_db provision test");
            return;
        };
        let host = agent_host_from_admin(&admin, None);
        let task_id = Uuid::new_v4();

        let pdb = ProjectDb::provision(&admin, &host, task_id).await.unwrap();

        // The throwaway role can log in and reach its own database.
        let conn = Database::connect(&pdb.agent_url)
            .await
            .expect("connect as throwaway role");
        conn.execute(Statement::from_string(
            conn.get_database_backend(),
            "CREATE TABLE fixture (id int)".to_string(),
        ))
        .await
        .expect("throwaway role can DDL in its own DB");
        drop(conn);

        pdb.teardown().await;

        // Database is gone → connecting as the (also-dropped) role fails.
        assert!(
            Database::connect(&pdb.agent_url).await.is_err(),
            "database should be dropped after teardown"
        );

        // Teardown is idempotent.
        pdb.teardown().await;
    }

    #[tokio::test]
    async fn sweep_removes_orphans() {
        let Some(admin) = admin_url() else {
            return;
        };
        let host = agent_host_from_admin(&admin, None);
        let pdb = ProjectDb::provision(&admin, &host, Uuid::new_v4())
            .await
            .unwrap();
        let leaked = pdb.name().to_string();
        // Simulate a crash: forget the guard so the objects survive.
        std::mem::forget(ProjectDbGuard(Some(pdb)));

        let swept = ProjectDb::sweep_orphans(&admin).await.unwrap();
        assert!(
            swept >= 1,
            "sweep should remove at least the leaked objects"
        );

        let conn = Database::connect(&admin).await.unwrap();
        let rows = conn
            .query_all(Statement::from_string(
                conn.get_database_backend(),
                format!("SELECT datname FROM pg_database WHERE datname = '{leaked}'"),
            ))
            .await
            .unwrap();
        assert!(rows.is_empty(), "leaked database should be swept");
    }
}
