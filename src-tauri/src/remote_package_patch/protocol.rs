#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptLine {
    Stage(String),
    Log { level: String, message: String },
    Result { key: String, value: String },
    Error(String),
    Raw { layer_tag: String, line: String },
    Plain(String),
}

pub fn parse_script_line(line: &str) -> ScriptLine {
    if let Some(rest) = line.strip_prefix("##STAGE:") {
        return ScriptLine::Stage(rest.to_string());
    }
    if let Some(rest) = line.strip_prefix("##LOG:") {
        if let Some((level, message)) = rest.split_once(':') {
            return ScriptLine::Log {
                level: level.to_string(),
                message: message.to_string(),
            };
        }
    }
    if let Some(rest) = line.strip_prefix("##RESULT:") {
        if let Some((key, value)) = rest.split_once('=') {
            return ScriptLine::Result {
                key: key.to_string(),
                value: value.to_string(),
            };
        }
    }
    if let Some(rest) = line.strip_prefix("##ERROR:") {
        return ScriptLine::Error(rest.to_string());
    }
    if let Some(rest) = line.strip_prefix("##RAW:") {
        if let Some((layer_tag, raw)) = rest.split_once('\t') {
            return ScriptLine::Raw {
                layer_tag: layer_tag.to_string(),
                line: raw.to_string(),
            };
        }
    }
    ScriptLine::Plain(line.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_structured_script_lines() {
        assert_eq!(
            parse_script_line("##STAGE:verify"),
            ScriptLine::Stage("verify".into())
        );
        assert_eq!(
            parse_script_line("##LOG:warn:temp kept"),
            ScriptLine::Log {
                level: "warn".into(),
                message: "temp kept".into()
            },
        );
        assert_eq!(
            parse_script_line("##RESULT:output_path=/tmp/a=b.tar.gz"),
            ScriptLine::Result {
                key: "output_path".into(),
                value: "/tmp/a=b.tar.gz".into()
            },
        );
        assert_eq!(
            parse_script_line("##ERROR:failed"),
            ScriptLine::Error("failed".into()),
        );
    }

    #[test]
    fn parses_raw_inventory_lines() {
        assert_eq!(
            parse_script_line(
                "##RAW:zst:comp/a.tar.zst\t-rw-r--r-- root/root 7 2026-01-02 03:04 a"
            ),
            ScriptLine::Raw {
                layer_tag: "zst:comp/a.tar.zst".into(),
                line: "-rw-r--r-- root/root 7 2026-01-02 03:04 a".into(),
            },
        );
    }

    #[test]
    fn malformed_lines_are_plain() {
        assert_eq!(
            parse_script_line("##LOG:bad"),
            ScriptLine::Plain("##LOG:bad".into())
        );
        assert_eq!(
            parse_script_line("plain"),
            ScriptLine::Plain("plain".into())
        );
    }
}
