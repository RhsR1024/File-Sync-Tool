
#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn archive_url_url_encodes_project_path_and_branch() {
        let url = build_archive_url();
        assert!(url.starts_with("http://igcode.uniview.com/api/v4/projects/"));
        assert!(url.contains("RD-UNIVIEW%2Fpublic%2FpubResList%2Ferrorcode"));
        assert!(url.ends_with("/repository/archive.zip?sha=main"));
    }

    #[test]
    fn basic_auth_header_round_trips_to_credentials() {
        let header = build_basic_auth_header();
        let b64 = header
            .strip_prefix("Basic ")
            .expect("header must begin with Basic ");
        let decoded =
            base64::engine::general_purpose::STANDARD.decode(b64).expect("valid base64");
        assert_eq!(String::from_utf8(decoded).unwrap(), "cmo_ipc:*Ab64799254");
    }
}
