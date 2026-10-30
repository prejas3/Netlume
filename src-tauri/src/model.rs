use serde::{Deserialize, Serialize};

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
