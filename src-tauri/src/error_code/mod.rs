//! Error code lookup feature.
//!
//! See `docs/superpowers/specs/2026-04-25-error-code-lookup-design.md`.

pub mod cache;
pub mod commands;
pub mod gitlab;
pub mod parser;
pub mod store;
pub mod sync;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorCodeEntry {
    pub code: u32,
    pub message_cn: String,
    pub message_en: String,
    pub solution: String,
    pub module: String,
    pub remark: String,
    pub source_file: String,
}

#[derive(Debug, Deserialize)]
pub struct QueryRequest {
    pub mode: String,
    pub value: String,
    pub page: u32,
}

#[derive(Debug, Serialize)]
pub struct QueryResult {
    pub entries: Vec<ErrorCodeEntry>,
    pub total: usize,
    pub page: u32,
    pub page_size: u32,
}

#[derive(Debug, Serialize)]
pub struct SyncReport {
    pub file_count: usize,
    pub row_count: usize,
    pub last_synced_at: String,
}

#[derive(Debug, Serialize)]
pub struct MetaInfo {
    pub has_cache: bool,
    pub last_synced_at: Option<String>,
    pub file_count: usize,
    pub row_count: usize,
}

pub const PAGE_SIZE: u32 = 50;
pub const MAX_RANGE_SPAN: u32 = 1000;

#[derive(Default)]
pub struct ErrorCodeStore {
    pub entries: BTreeMap<u32, Vec<ErrorCodeEntry>>,
    pub last_synced_at: Option<String>,
    pub loaded: bool,
}

pub type ErrorCodeState = Mutex<ErrorCodeStore>;
