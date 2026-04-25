use chardetng::EncodingDetector;
use encoding_rs::Encoding;

use crate::error_code::ErrorCodeEntry;

pub fn detect_encoding(bytes: &[u8]) -> &'static Encoding {
    let mut detector = EncodingDetector::new();
    detector.feed(bytes, true);
    detector.guess(None, true)
}

pub fn decode_bytes(bytes: &[u8]) -> String {
    let encoding = detect_encoding(bytes);
    let (cow, _, _) = encoding.decode(bytes);
    let text = cow.into_owned();
    text.strip_prefix('\u{FEFF}')
        .map(str::to_string)
        .unwrap_or(text)
}

pub fn parse_csv_text(text: &str, source_file: &str) -> Vec<ErrorCodeEntry> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(text.as_bytes());

    let mut entries = Vec::new();
    for (index, result) in reader.records().enumerate() {
        let line_num = index + 2;
        match result {
            Ok(record) => {
                if let Some(entry) = parse_row(&record, source_file) {
                    entries.push(entry);
                } else {
                    log::warn!(
                        "[error_code] skip invalid row {}:{} -> {:?}",
                        source_file,
                        line_num,
                        record
                    );
                }
            }
            Err(error) => {
                log::warn!(
                    "[error_code] CSV parse error {}:{}: {}",
                    source_file,
                    line_num,
                    error
                );
            }
        }
    }

    entries
}

pub fn parse_csv_bytes(bytes: &[u8], source_file: &str) -> Vec<ErrorCodeEntry> {
    let text = decode_bytes(bytes);
    parse_csv_text(&text, source_file)
}

fn parse_row(record: &csv::StringRecord, source_file: &str) -> Option<ErrorCodeEntry> {
    let code_cell = record.get(0)?.trim();
    let code: u32 = code_cell.parse().ok()?;

    Some(ErrorCodeEntry {
        code,
        message_cn: cell(record, 1),
        message_en: cell(record, 2),
        solution: cell(record, 3),
        module: cell(record, 4),
        remark: remark_cell(record),
        source_file: source_file.to_string(),
    })
}

fn cell(record: &csv::StringRecord, idx: usize) -> String {
    record.get(idx).unwrap_or("").trim().to_string()
}

fn remark_cell(record: &csv::StringRecord) -> String {
    if record.len() <= 6 {
        return cell(record, 5);
    }

    record
        .iter()
        .skip(5)
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;
    use encoding_rs::GBK;

    #[test]
    fn decode_ascii_is_identity() {
        let s = decode_bytes(b"hello,world");
        assert_eq!(s, "hello,world");
    }

    #[test]
    fn decode_utf8_chinese_round_trip() {
        let s = decode_bytes("执行成功".as_bytes());
        assert_eq!(s, "执行成功");
    }

    #[test]
    fn decode_gbk_chinese_round_trip() {
        let (encoded, _, had_errors) = GBK.encode("执行成功");
        assert!(!had_errors);
        let decoded = decode_bytes(&encoded);
        assert_eq!(decoded, "执行成功");
    }

    #[test]
    fn decode_strips_utf8_bom() {
        let mut bytes: Vec<u8> = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("0,执行成功".as_bytes());
        let decoded = decode_bytes(&bytes);
        assert_eq!(decoded, "0,执行成功");
    }

    #[test]
    fn parse_simple_row_with_header() {
        let text = "code,cn,en,solution,module,remark\n0,执行成功,Success.,,,";
        let entries = parse_csv_text(text, "10w.csv");
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.code, 0);
        assert_eq!(entry.message_cn, "执行成功");
        assert_eq!(entry.message_en, "Success.");
        assert_eq!(entry.solution, "");
        assert_eq!(entry.module, "");
        assert_eq!(entry.remark, "");
        assert_eq!(entry.source_file, "10w.csv");
    }

    #[test]
    fn parse_skips_non_numeric_first_cell() {
        let text = "code,cn,en,solution,module,remark\nABC,无效,Invalid,,,";
        let entries = parse_csv_text(text, "10w.csv");
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_handles_quoted_comma_in_solution() {
        let text = "code,cn,en,solution,module,remark\n100,异常,Error,\"重启服务,然后重试\",CORE,";
        let entries = parse_csv_text(text, "10w.csv");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].solution, "重启服务,然后重试");
    }

    #[test]
    fn parse_pads_short_rows_with_empty_strings() {
        let text = "code,cn,en,solution,module,remark\n5,短行";
        let entries = parse_csv_text(text, "10w.csv");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message_cn, "短行");
        assert_eq!(entries[0].solution, "");
        assert_eq!(entries[0].module, "");
    }

    #[test]
    fn parse_extra_columns_append_into_remark() {
        let text = "code,cn,en,solution,module,remark\n6,多列,Extra,,,foo,bar";
        let entries = parse_csv_text(text, "10w.csv");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].remark, "foo,bar");
    }

    #[test]
    fn parse_csv_bytes_handles_gbk() {
        let text = "code,cn,en,solution,module,remark\n0,执行成功,Success.,,,";
        let (encoded, _, had_errors) = GBK.encode(text);
        assert!(!had_errors);
        let entries = parse_csv_bytes(&encoded, "20w.csv");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message_cn, "执行成功");
    }
}
