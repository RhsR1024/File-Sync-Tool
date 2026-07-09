use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum InternalLayer {
    Middle,
    Zst {
        #[serde(rename = "zstPath")]
        zst_path: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageEntry {
    pub layer: InternalLayer,
    pub path: String,
    pub kind: EntryKind,
    pub size: u64,
    pub perms_text: String,
    pub owner_text: String,
    pub mtime_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageInventory {
    pub package_path: String,
    pub middle_tar_path: String,
    pub entries: Vec<PackageEntry>,
}

pub fn parse_raw_layer(tag: &str) -> Option<InternalLayer> {
    if tag == "middle" {
        return Some(InternalLayer::Middle);
    }

    tag.strip_prefix("zst:").map(|zst_path| InternalLayer::Zst {
        zst_path: zst_path.to_string(),
    })
}

fn split_token(value: &str) -> Option<(&str, &str)> {
    let value = value.trim_start();
    if value.is_empty() {
        return None;
    }

    match value.find(char::is_whitespace) {
        Some(index) => Some((&value[..index], &value[index..])),
        None => Some((value, "")),
    }
}

pub fn parse_tar_verbose_line(layer: InternalLayer, line: &str) -> Option<PackageEntry> {
    let (perms, rest) = split_token(line)?;
    if perms.len() != 10 {
        return None;
    }

    let kind = match perms.as_bytes().first()? {
        b'-' => EntryKind::File,
        b'd' => EntryKind::Dir,
        b'l' => EntryKind::Symlink,
        _ => EntryKind::Other,
    };

    let (owner, rest) = split_token(rest)?;
    if !owner.contains('/') {
        return None;
    }
    let (size, rest) = split_token(rest)?;
    let size = size.parse::<u64>().ok()?;
    let (date, rest) = split_token(rest)?;
    let (time, rest) = split_token(rest)?;
    let mut path = rest.strip_prefix(' ').unwrap_or(rest).to_string();
    if path.is_empty() {
        return None;
    }
    if kind == EntryKind::Symlink {
        if let Some((left, _)) = path.split_once(" -> ") {
            path = left.to_string();
        }
    }

    Some(PackageEntry {
        layer,
        path,
        kind,
        size,
        perms_text: perms.to_string(),
        owner_text: owner.to_string(),
        mtime_text: format!("{date} {time}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_regular_file_line() {
        let entry = parse_tar_verbose_line(
            InternalLayer::Middle,
            "-rw-r--r-- root/root 123 2026-01-02 03:04 comp/bin/libdemo.so",
        )
        .unwrap();
        assert_eq!(entry.kind, EntryKind::File);
        assert_eq!(entry.path, "comp/bin/libdemo.so");
        assert_eq!(entry.size, 123);
        assert_eq!(entry.perms_text, "-rw-r--r--");
        assert_eq!(entry.owner_text, "root/root");
        assert_eq!(entry.mtime_text, "2026-01-02 03:04");
    }

    #[test]
    fn preserves_dot_prefix_and_spaces() {
        let entry = parse_tar_verbose_line(
            InternalLayer::Middle,
            "-rw-r--r-- 1000/1000 7 2026-01-02 03:04 ./dir with spaces/file.so",
        )
        .unwrap();
        assert_eq!(entry.path, "./dir with spaces/file.so");
    }

    #[test]
    fn strips_symlink_arrow_target() {
        let entry = parse_tar_verbose_line(
            InternalLayer::Middle,
            "lrwxrwxrwx root/root 0 2026-01-02 03:04 comp/lib.so -> lib.so.1",
        )
        .unwrap();
        assert_eq!(entry.kind, EntryKind::Symlink);
        assert_eq!(entry.path, "comp/lib.so");
    }

    #[test]
    fn parses_layer_tags() {
        assert_eq!(parse_raw_layer("middle"), Some(InternalLayer::Middle));
        assert_eq!(
            parse_raw_layer("zst:comp/a.tar.zst"),
            Some(InternalLayer::Zst {
                zst_path: "comp/a.tar.zst".into()
            }),
        );
        assert_eq!(parse_raw_layer("outer"), None);
    }
}
