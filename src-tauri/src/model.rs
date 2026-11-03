use serde::{Deserialize, Serialize};

// CCS/1 IO: commands=set_limit,set_block,set_global,get_global,history,get_autostart,set_autostart(on)->bool,open_external(https url|"data"),get_language()->"pl"|"en",set_language("pl"|"en") persisted | events=stats,global
pub const STATS_EVENT: &str = "stats";
pub const GLOBAL_EVENT: &str = "global";

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Limit {
    pub down_bps: Option<u64>,
    pub up_bps: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AppStat {
    pub id: String,
    pub name: String,
    pub path: String,
    pub down_bps: u64,
    pub up_bps: u64,
    pub down_total: u64,
    pub up_total: u64,
    pub limit: Option<Limit>,
    pub blocked: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Global {
    pub limit: Limit,
    pub paused: bool,
}
