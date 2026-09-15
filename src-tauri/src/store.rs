use crate::engine::{Repo, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize)]
pub struct Project {
    pub root: String,
    pub name: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub id: i64,
    pub root: String,
    pub name: String,
    pub kind: String,
    pub paths: Vec<String>,
    pub color: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub default_state: String,
    pub groups: std::collections::BTreeMap<String, bool>,
    pub resources: std::collections::BTreeMap<String, bool>,
    pub clients: Vec<String>,
}
impl Preset {
    fn validate(&self) -> Result<()> {
        if self.id.is_empty()
            || self.id.len() > 120
            || self.name.trim().is_empty()
            || self.name.len() > 240
            || !["keep", "on", "off"].contains(&self.default_state.as_str())
            || self.groups.len() > 10000
            || self.resources.len() > 10000
            || self
                .clients
                .iter()
                .any(|c| !crate::global::CLIENTS.iter().any(|(name, _)| *name == c))
        {
            return Err("方案配置不正确".into());
        }
        Ok(())
    }
}
pub struct Store(Connection);
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS projects(root TEXT PRIMARY KEY, name TEXT NOT NULL); CREATE TABLE IF NOT EXISTS collections(id INTEGER PRIMARY KEY,root TEXT NOT NULL,name TEXT NOT NULL,kind TEXT NOT NULL,paths TEXT NOT NULL, UNIQUE(root,name,kind));").map_err(|e|e.to_string())?;
        // Add presentation metadata without changing existing membership or profiles.
        let columns: Vec<String> = conn
            .prepare("PRAGMA table_info(collections)")
            .map_err(|e| e.to_string())?
            .query_map([], |r| r.get(1))
            .map_err(|e| e.to_string())?
            .collect::<std::result::Result<_, _>>()
            .map_err(|e| e.to_string())?;
        if !columns.iter().any(|c| c == "color") {
            conn.execute_batch(
                "ALTER TABLE collections ADD COLUMN color TEXT NOT NULL DEFAULT 'slate';",
            )
            .map_err(|e| e.to_string())?;
        }
        if !columns.iter().any(|c| c == "position") {
            conn.execute_batch(
                "ALTER TABLE collections ADD COLUMN position INTEGER NOT NULL DEFAULT 0;",
            )
            .map_err(|e| e.to_string())?;
        }
        conn.execute_batch("CREATE TABLE IF NOT EXISTS presets(scope TEXT NOT NULL,id TEXT NOT NULL,name TEXT NOT NULL,body TEXT NOT NULL,PRIMARY KEY(scope,id),UNIQUE(scope,name));")
            .map_err(|e| e.to_string())?;
        // One-time migration of old disabled-resource snapshots into explicit presets.
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        {
            let mut stmt = tx
                .prepare("SELECT id,root,name,paths FROM collections WHERE kind='profile'")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                })
                .map_err(|e| e.to_string())?;
            for row in rows {
                let (id, scope, name, paths) = row.map_err(|e| e.to_string())?;
                let paths: Vec<String> = serde_json::from_str(&paths).map_err(|e| e.to_string())?;
                let preset = Preset {
                    id: format!("legacy-{id}"),
                    name: name.clone(),
                    default_state: "on".into(),
                    groups: Default::default(),
                    resources: paths.into_iter().map(|p| (p, false)).collect(),
                    clients: vec![],
                };
                tx.execute(
                    "INSERT INTO presets(scope,id,name,body) VALUES(?1,?2,?3,?4)",
                    params![
                        scope,
                        preset.id,
                        name,
                        serde_json::to_string(&preset).map_err(|e| e.to_string())?
                    ],
                )
                .map_err(|e| e.to_string())?;
            }
        }
        tx.execute("DELETE FROM collections WHERE kind='profile'", [])
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(Self(conn))
    }
    pub fn projects(&self) -> Result<Vec<Project>> {
        let mut stmt = self
            .0
            .prepare("SELECT root,name FROM projects ORDER BY name")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Project {
                    root: r.get(0)?,
                    name: r.get(1)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }
    pub fn add(&self, path: &str) -> Result<String> {
        let repo = Repo::open(Path::new(path))?;
        let root = repo.root.to_string_lossy().to_string();
        self.0
            .execute(
                "INSERT OR IGNORE INTO projects(root,name) VALUES(?1,?2)",
                params![
                    root,
                    repo.root.file_name().unwrap_or_default().to_string_lossy()
                ],
            )
            .map_err(|e| e.to_string())?;
        Ok(root)
    }
    pub fn ensure_project(&self, root: &str) -> Result<Repo> {
        if !self.projects()?.iter().any(|p| p.root == root) {
            return Err("请先添加项目".into());
        }
        Repo::open(Path::new(root))
    }
    pub fn collections(&self, root: &str) -> Result<Vec<Collection>> {
        let mut stmt = self
            .0
            .prepare(
                "SELECT id,root,name,kind,paths,color FROM collections WHERE root=?1 ORDER BY kind,position,name",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([root], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            let (id, root, name, kind, p, color) = r.map_err(|e| e.to_string())?;
            out.push(Collection {
                id,
                root,
                name,
                kind,
                color,
                paths: serde_json::from_str(&p).map_err(|e| e.to_string())?,
            });
        }
        Ok(out)
    }
    pub fn save(&self, root: &str, name: &str, kind: &str, paths: Vec<String>) -> Result<()> {
        self.ensure_project(root)?;
        if name.trim().is_empty() || name.len() > 120 || !["group", "profile"].contains(&kind) {
            return Err("名称或类型不正确".into());
        }
        let known = self.ensure_project(root)?.scan()?.resources;
        for p in &paths {
            if !known.iter().any(|r| &r.path == p) {
                return Err(format!("资源已不存在：{p}"));
            }
        }
        self.0.execute("INSERT INTO collections(root,name,kind,paths) VALUES(?1,?2,?3,?4) ON CONFLICT(root,name,kind) DO UPDATE SET paths=excluded.paths",params![root,name.trim(),kind,serde_json::to_string(&paths).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
        Ok(())
    }
    pub fn update_group(&self, root: &str, id: i64, name: &str, color: &str) -> Result<()> {
        self.ensure_project(root)?;
        if name.trim().is_empty()
            || name.len() > 120
            || ![
                "blue", "violet", "emerald", "amber", "rose", "cyan", "slate",
            ]
            .contains(&color)
        {
            return Err("名称或颜色不正确".into());
        }
        let changed = self
            .0
            .execute(
                "UPDATE collections SET name=?1,color=?2 WHERE root=?3 AND id=?4 AND kind='group'",
                params![name.trim(), color, root, id],
            )
            .map_err(|e| e.to_string())?;
        if changed != 1 {
            return Err("分组已不存在".into());
        }
        Ok(())
    }
    pub fn move_group(
        &self,
        root: &str,
        paths: Vec<String>,
        destination: Option<i64>,
    ) -> Result<()> {
        let known = self.ensure_project(root)?.scan()?.resources;
        if paths.is_empty() || paths.iter().any(|p| !known.iter().any(|r| &r.path == p)) {
            return Err("请选择存在的资源".into());
        }
        let groups: Vec<_> = self
            .collections(root)?
            .into_iter()
            .filter(|c| c.kind == "group")
            .collect();
        if destination.is_some_and(|id| !groups.iter().any(|c| c.id == id)) {
            return Err("目标分组已不存在".into());
        }
        let tx = self.0.unchecked_transaction().map_err(|e| e.to_string())?;
        for mut c in groups {
            c.paths.retain(|p| !paths.contains(p));
            if destination == Some(c.id) {
                for p in &paths {
                    if !c.paths.contains(p) {
                        c.paths.push(p.clone());
                    }
                }
            }
            tx.execute(
                "UPDATE collections SET paths=?1 WHERE id=?2",
                params![
                    serde_json::to_string(&c.paths).map_err(|e| e.to_string())?,
                    c.id
                ],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }
    pub fn reorder_groups(&self, root: &str, ids: Vec<i64>) -> Result<()> {
        self.ensure_project(root)?;
        let groups: Vec<_> = self
            .collections(root)?
            .into_iter()
            .filter(|c| c.kind == "group")
            .collect();
        let unique: std::collections::BTreeSet<_> = ids.iter().copied().collect();
        if ids.len() != groups.len()
            || unique.len() != ids.len()
            || groups.iter().any(|g| !unique.contains(&g.id))
        {
            return Err("分组列表已变化，请刷新后重试".into());
        }
        let tx = self.0.unchecked_transaction().map_err(|e| e.to_string())?;
        for (position, id) in ids.iter().enumerate() {
            tx.execute(
                "UPDATE collections SET position=?1 WHERE root=?2 AND id=?3",
                params![position as i64, root, id],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }
    pub fn presets(&self, scope: &str) -> Result<Vec<Preset>> {
        let mut stmt = self
            .0
            .prepare("SELECT body FROM presets WHERE scope=?1 ORDER BY name")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([scope], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        rows.map(|row| {
            let mut preset: Preset = serde_json::from_str(&row.map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            preset.clients.retain(|client| {
                crate::global::CLIENTS
                    .iter()
                    .any(|(name, _)| *name == client)
            });
            Ok(preset)
        })
        .collect()
    }
    pub fn save_preset(&self, scope: &str, mut preset: Preset) -> Result<()> {
        if scope != "global" {
            self.ensure_project(scope)?;
        }
        preset.name = preset.name.trim().into();
        preset.validate()?;
        if scope == "global" && preset.clients.is_empty() {
            return Err("请选择目标客户端".into());
        }
        self.0.execute("INSERT INTO presets(scope,id,name,body) VALUES(?1,?2,?3,?4) ON CONFLICT(scope,id) DO UPDATE SET name=excluded.name,body=excluded.body",
            params![scope,preset.id,preset.name,serde_json::to_string(&preset).map_err(|e| e.to_string())?]).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn delete_preset(&self, scope: &str, id: &str) -> Result<()> {
        self.0
            .execute(
                "DELETE FROM presets WHERE scope=?1 AND id=?2",
                params![scope, id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn delete_collection(&self, id: i64) -> Result<()> {
        self.0
            .execute("DELETE FROM collections WHERE id=?1", [id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
