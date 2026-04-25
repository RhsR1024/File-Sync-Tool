use crate::error_code::{ErrorCodeEntry, ErrorCodeStore, QueryResult, MAX_RANGE_SPAN, PAGE_SIZE};

impl ErrorCodeStore {
    pub fn ingest(&mut self, items: Vec<ErrorCodeEntry>) {
        self.entries.clear();
        for entry in items {
            self.entries.entry(entry.code).or_default().push(entry);
        }
    }

    pub fn row_count(&self) -> usize {
        self.entries.values().map(Vec::len).sum()
    }

    pub fn query_single(&self, code: u32, page: u32) -> QueryResult {
        let hits = self.entries.get(&code).cloned().unwrap_or_default();
        paginate(hits, page)
    }

    pub fn query_range(
        &self,
        start: u32,
        end: u32,
        page: u32,
    ) -> Result<QueryResult, &'static str> {
        if end < start {
            return Err("range_reversed");
        }
        if end - start > MAX_RANGE_SPAN {
            return Err("range_too_large");
        }

        let hits: Vec<ErrorCodeEntry> = self
            .entries
            .range(start..=end)
            .flat_map(|(_, entries)| entries.iter().cloned())
            .collect();
        Ok(paginate(hits, page))
    }

    pub fn query_keyword(&self, keyword: &str, page: u32) -> QueryResult {
        let needle = keyword.trim().to_lowercase();
        let hits: Vec<ErrorCodeEntry> = self
            .entries
            .values()
            .flatten()
            .filter(|entry| {
                if needle.is_empty() {
                    return true;
                }

                entry.message_cn.to_lowercase().contains(&needle)
                    || entry.message_en.to_lowercase().contains(&needle)
                    || entry.solution.to_lowercase().contains(&needle)
            })
            .cloned()
            .collect();
        paginate(hits, page)
    }
}

pub(crate) fn paginate(items: Vec<ErrorCodeEntry>, page: u32) -> QueryResult {
    let total = items.len();
    let normalized_page = page.max(1);
    let start = (normalized_page - 1) as usize * PAGE_SIZE as usize;
    let end = (start + PAGE_SIZE as usize).min(total);
    let entries = if start >= total {
        Vec::new()
    } else {
        items[start..end].to_vec()
    };

    QueryResult {
        entries,
        total,
        page: normalized_page,
        page_size: PAGE_SIZE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(code: u32, cn: &str, en: &str, solution: &str, source: &str) -> ErrorCodeEntry {
        ErrorCodeEntry {
            code,
            message_cn: cn.to_string(),
            message_en: en.to_string(),
            solution: solution.to_string(),
            module: String::new(),
            remark: String::new(),
            source_file: source.to_string(),
        }
    }

    fn store_with(items: Vec<ErrorCodeEntry>) -> ErrorCodeStore {
        let mut store = ErrorCodeStore::default();
        store.ingest(items);
        store.loaded = true;
        store
    }

    #[test]
    fn ingest_clears_previous_entries() {
        let mut store = ErrorCodeStore::default();
        store.ingest(vec![entry(1, "a", "a", "", "10w.csv")]);
        store.ingest(vec![entry(2, "b", "b", "", "20w.csv")]);
        assert_eq!(store.entries.len(), 1);
        assert!(store.entries.contains_key(&2));
        assert!(!store.entries.contains_key(&1));
    }

    #[test]
    fn single_query_returns_matching_entry() {
        let store = store_with(vec![
            entry(0, "执行成功", "Success.", "", "10w.csv"),
            entry(1, "执行失败", "Error.", "", "10w.csv"),
        ]);
        let result = store.query_single(1, 1);
        assert_eq!(result.total, 1);
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].message_en, "Error.");
        assert_eq!(result.page, 1);
        assert_eq!(result.page_size, PAGE_SIZE);
    }

    #[test]
    fn single_query_returns_empty_when_missing() {
        let store = store_with(vec![entry(0, "执行成功", "Success.", "", "10w.csv")]);
        let result = store.query_single(999, 1);
        assert_eq!(result.total, 0);
        assert!(result.entries.is_empty());
    }

    #[test]
    fn single_query_returns_multiple_when_same_code_in_two_files() {
        let store = store_with(vec![
            entry(100, "异常 A", "Err A", "", "10w.csv"),
            entry(100, "异常 B", "Err B", "", "20w.csv"),
        ]);
        let result = store.query_single(100, 1);
        assert_eq!(result.total, 2);
        assert_eq!(result.entries.len(), 2);
    }

    #[test]
    fn pagination_returns_first_page_then_second() {
        let mut items: Vec<ErrorCodeEntry> = (0..120)
            .map(|i| entry(i, &format!("cn{i}"), "", "", "10w.csv"))
            .collect();
        items.sort_by_key(|item| item.code);
        let store = store_with(items);

        let result = paginate_for_test(&store, 1);
        assert_eq!(result.entries.len(), 50);
        assert_eq!(result.entries[0].code, 0);
        assert_eq!(result.total, 120);

        let result = paginate_for_test(&store, 2);
        assert_eq!(result.entries.len(), 50);
        assert_eq!(result.entries[0].code, 50);

        let result = paginate_for_test(&store, 3);
        assert_eq!(result.entries.len(), 20);
    }

    #[test]
    fn pagination_normalizes_page_zero_to_one_and_clamps_overshoot() {
        let store = store_with(vec![entry(1, "a", "", "", "10w.csv")]);
        let result = paginate_for_test(&store, 0);
        assert_eq!(result.page, 1);
        assert_eq!(result.entries.len(), 1);

        let result = paginate_for_test(&store, 999);
        assert!(result.entries.is_empty());
    }

    #[test]
    fn range_query_returns_entries_sorted_ascending() {
        let store = store_with(vec![
            entry(300_500, "C", "C", "", "30w.csv"),
            entry(300_100, "A", "A", "", "30w.csv"),
            entry(300_900, "D", "D", "", "30w.csv"),
            entry(300_300, "B", "B", "", "30w.csv"),
            entry(400_000, "Z", "Z", "", "40w.csv"),
        ]);
        let result = store.query_range(300_000, 301_000, 1).expect("ok");
        assert_eq!(result.total, 4);
        let codes: Vec<u32> = result.entries.iter().map(|entry| entry.code).collect();
        assert_eq!(codes, vec![300_100, 300_300, 300_500, 300_900]);
    }

    #[test]
    fn range_query_inclusive_endpoints() {
        let store = store_with(vec![
            entry(100, "L", "", "", "10w.csv"),
            entry(200, "R", "", "", "10w.csv"),
        ]);
        let result = store.query_range(100, 200, 1).expect("ok");
        assert_eq!(result.total, 2);
    }

    #[test]
    fn range_query_rejects_span_above_1000() {
        let store = ErrorCodeStore::default();
        let error = store.query_range(0, 1_001, 1).unwrap_err();
        assert_eq!(error, "range_too_large");
    }

    #[test]
    fn range_query_accepts_span_exactly_1000() {
        let store = ErrorCodeStore::default();
        assert!(store.query_range(300_000, 301_000, 1).is_ok());
    }

    #[test]
    fn range_query_rejects_reversed_endpoints() {
        let store = ErrorCodeStore::default();
        let error = store.query_range(500, 100, 1).unwrap_err();
        assert_eq!(error, "range_reversed");
    }

    #[test]
    fn keyword_query_matches_in_message_cn_en_and_solution() {
        let store = store_with(vec![
            entry(1, "执行失败", "Error.", "", "10w.csv"),
            entry(2, "成功", "Success.", "", "10w.csv"),
            entry(3, "其他", "Other.", "请重启服务", "10w.csv"),
        ]);
        let result = store.query_keyword("失败", 1);
        assert_eq!(result.total, 1);
        assert_eq!(result.entries[0].code, 1);

        let result = store.query_keyword("Success", 1);
        assert_eq!(result.total, 1);
        assert_eq!(result.entries[0].code, 2);

        let result = store.query_keyword("重启", 1);
        assert_eq!(result.total, 1);
        assert_eq!(result.entries[0].code, 3);
    }

    #[test]
    fn keyword_query_is_case_insensitive() {
        let store = store_with(vec![entry(1, "Foo", "BarBaz", "", "10w.csv")]);
        assert_eq!(store.query_keyword("BARBAZ", 1).total, 1);
        assert_eq!(store.query_keyword("foo", 1).total, 1);
    }

    #[test]
    fn keyword_query_empty_returns_all_sorted() {
        let store = store_with(vec![
            entry(2, "B", "", "", "10w.csv"),
            entry(1, "A", "", "", "10w.csv"),
            entry(3, "C", "", "", "10w.csv"),
        ]);
        let result = store.query_keyword("", 1);
        assert_eq!(result.total, 3);
        let codes: Vec<u32> = result.entries.iter().map(|entry| entry.code).collect();
        assert_eq!(codes, vec![1, 2, 3]);
    }

    #[test]
    fn keyword_query_no_match_returns_empty() {
        let store = store_with(vec![entry(1, "执行失败", "Error.", "", "10w.csv")]);
        let result = store.query_keyword("nonexistent", 1);
        assert_eq!(result.total, 0);
        assert!(result.entries.is_empty());
    }

    fn paginate_for_test(store: &ErrorCodeStore, page: u32) -> QueryResult {
        let all: Vec<ErrorCodeEntry> = store.entries.values().flatten().cloned().collect();
        super::paginate(all, page)
    }
}
