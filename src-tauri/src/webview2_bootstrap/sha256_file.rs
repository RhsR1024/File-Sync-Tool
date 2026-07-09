//! Parse `.sha256` sidecar files: bare `<64hex>` or `<64hex>  <filename>`.

pub fn parse_sha256_file(content: &str) -> Result<String, String> {
    let token = content
        .trim_start_matches('\u{feff}')
        .split_whitespace()
        .next()
        .ok_or_else(|| "empty .sha256 file".to_string())?;
    if token.len() != 64 || !token.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("invalid sha256 content: {token:.80}"));
    }
    Ok(token.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";

    #[test]
    fn parses_bare_hash_with_whitespace() {
        assert_eq!(parse_sha256_file(&format!("{HASH}\r\n")).unwrap(), HASH);
    }

    #[test]
    fn parses_hash_with_filename() {
        let content = format!("{HASH}  MicrosoftEdgeWebView2RuntimeInstallerX64.exe\n");
        assert_eq!(parse_sha256_file(&content).unwrap(), HASH);
    }

    #[test]
    fn lowercases_uppercase_hash() {
        assert_eq!(parse_sha256_file(&HASH.to_ascii_uppercase()).unwrap(), HASH);
    }

    #[test]
    fn rejects_wrong_length_and_non_hex() {
        assert!(parse_sha256_file(&HASH[..63]).is_err());
        assert!(parse_sha256_file(&format!("g{}", &HASH[..63])).is_err());
        assert!(parse_sha256_file("").is_err());
        assert!(parse_sha256_file("   \n").is_err());
    }
}
