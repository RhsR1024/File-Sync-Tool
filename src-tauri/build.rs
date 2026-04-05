use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    ensure_file_share_web_placeholder();
    tauri_build::build()
}

fn ensure_file_share_web_placeholder() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let web_dir = manifest_dir.join("../dist/file-share-web");
    let index_path = web_dir.join("index.html");

    println!("cargo:rerun-if-changed={}", web_dir.display());

    if index_path.exists() {
        return;
    }

    fs::create_dir_all(&web_dir).expect("create file share web asset dir");
    fs::write(&index_path, placeholder_index_html()).expect("write file share web placeholder");
}

fn placeholder_index_html() -> &'static str {
    r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>File Share Web Assets</title>
  </head>
  <body>
    <main style="font-family: sans-serif; padding: 24px; line-height: 1.5;">
      <h1>File Share Web Assets Placeholder</h1>
      <p>Run <code>pnpm build:file-share-web</code> to generate the embedded CHFS-style file manager.</p>
    </main>
  </body>
</html>
"#
}
