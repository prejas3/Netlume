use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};

use crate::model::{AppStat, Global, Limit};

#[derive(Clone, Default, Deserialize, Serialize)]
pub struct AppRule {
    pub limit: Limit,
    pub blocked: bool,
}

#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Rules {
    pub apps: BTreeMap<String, AppRule>,
    pub global: Global,
}

#[derive(Default)]
struct Today {
    day: String,
    apps: BTreeMap<String, AppStat>,
    previous: BTreeMap<String, [u64; 2]>,
}

pub struct Storage {
    root: PathBuf,
    today: Mutex<Today>,
}

impl Storage {
    pub fn new(root: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(root.join("history"))?;
        Ok(Self {
            root,
            today: Mutex::new(Today::default()),
        })
    }

    pub fn load_rules(&self) -> io::Result<Rules> {
        read_json(&self.root.join("rules.json"))
    }

    pub fn save_rules(&self, rules: &Rules) -> io::Result<()> {
        write_json(&self.root.join("rules.json"), rules)
    }

    pub fn record(&self, day: &str, live: &[AppStat]) -> io::Result<()> {
        let mut today = self.today.lock().map_err(lock_error)?;
        if today.day != day {
            if !today.day.is_empty() {
                write_json(&self.history_path(&today.day), &today.apps)?;
            }
            today.day = day.to_string();
            today.apps = read_json(&self.history_path(day)).unwrap_or_default();
        }
        for app in live {
            let previous = today
                .previous
                .insert(app.path.clone(), [app.down_total, app.up_total])
                .unwrap_or_default();
            let entry = today
                .apps
                .entry(app.path.clone())
                .or_insert_with(|| history_stat(app));
            entry.name.clone_from(&app.name);
            entry.down_total += app.down_total.saturating_sub(previous[0]);
            entry.up_total += app.up_total.saturating_sub(previous[1]);
        }
        Ok(())
    }

    pub fn flush(&self) -> io::Result<()> {
        let today = self.today.lock().map_err(lock_error)?;
        if today.day.is_empty() {
            return Ok(());
        }
        write_json(&self.history_path(&today.day), &today.apps)
    }

    pub fn history(&self, day: &str) -> io::Result<Vec<AppStat>> {
        let today = self.today.lock().map_err(lock_error)?;
        let apps: BTreeMap<String, AppStat> = if today.day == day {
            today.apps.clone()
        } else {
            read_json(&self.history_path(day)).unwrap_or_default()
        };
        Ok(apps.into_values().collect())
    }

    fn history_path(&self, day: &str) -> PathBuf {
        self.root.join("history").join(format!("{day}.json"))
    }
}

fn history_stat(app: &AppStat) -> AppStat {
    AppStat {
        id: app.id.clone(),
        name: app.name.clone(),
        path: app.path.clone(),
        down_bps: 0,
        up_bps: 0,
        down_total: 0,
        up_total: 0,
        limit: None,
        blocked: false,
    }
}

fn read_json<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> io::Result<T> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(error),
    }
}

fn write_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    fs::write(path, bytes)
}

fn lock_error<T>(error: std::sync::PoisonError<T>) -> io::Error {
    io::Error::other(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "netlume-storage-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn stat(total: u64) -> AppStat {
        AppStat {
            id: "a".into(),
            name: "App".into(),
            path: "a.exe".into(),
            down_bps: 1,
            up_bps: 2,
            down_total: total,
            up_total: total / 2,
            limit: None,
            blocked: false,
        }
    }

    #[test]
    fn rules_round_trip() {
        let root = temp_root();
        let storage = Storage::new(root.clone()).unwrap();
        let mut rules = Rules::default();
        rules.apps.insert(
            "a.exe".into(),
            AppRule {
                limit: Limit {
                    down_bps: Some(12),
                    up_bps: None,
                },
                blocked: true,
            },
        );
        storage.save_rules(&rules).unwrap();
        assert_eq!(
            storage.load_rules().unwrap().apps["a.exe"].limit.down_bps,
            Some(12)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn history_adds_only_tick_deltas_and_survives_reload() {
        let root = temp_root();
        let storage = Storage::new(root.clone()).unwrap();
        storage.record("2026-10-30", &[stat(100)]).unwrap();
        storage.record("2026-10-30", &[stat(140)]).unwrap();
        storage.flush().unwrap();
        let history = Storage::new(root.clone())
            .unwrap()
            .history("2026-10-30")
            .unwrap();
        assert_eq!((history[0].down_total, history[0].up_total), (140, 70));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn day_change_does_not_recount_the_session() {
        let root = temp_root();
        let storage = Storage::new(root.clone()).unwrap();
        storage.record("2026-10-30", &[stat(100)]).unwrap();
        storage.record("2026-10-31", &[stat(140)]).unwrap();
        assert_eq!(storage.history("2026-10-31").unwrap()[0].down_total, 40);
        fs::remove_dir_all(root).unwrap();
    }
}
