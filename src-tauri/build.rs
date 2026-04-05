use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));

    configure_file_share_web_rebuilds(&manifest_dir);
    ensure_file_share_web_assets(&manifest_dir);
    tauri_build::build()
}

fn configure_file_share_web_rebuilds(manifest_dir: &Path) {
    for path in [
        manifest_dir.join("../src/share-web"),
        manifest_dir.join("../vite.file-share-web.config.ts"),
        manifest_dir.join("../package.json"),
        manifest_dir.join("../pnpm-lock.yaml"),
    ] {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

fn ensure_file_share_web_assets(manifest_dir: &Path) {
    let workspace_dir = manifest_dir.join("..");
    let web_source_dir = workspace_dir.join("src/share-web");
    let web_dir = workspace_dir.join("dist/file-share-web");
    let index_path = web_dir.join("index.html");

    if !web_source_dir.exists() {
        ensure_file_share_web_placeholder(&web_dir, &index_path);
        return;
    }

    let status = Command::new(pnpm_command())
        .current_dir(&workspace_dir)
        .arg("build:file-share-web")
        .status()
        .unwrap_or_else(|error| {
            panic!(
                "failed to run `{}` for file share web assets: {}",
                pnpm_command(),
                error
            )
        });

    if !status.success() {
        panic!(
            "`{} build:file-share-web` failed with status {}",
            pnpm_command(),
            status
        );
    }

    if !index_html_looks_built(&index_path) {
        ensure_file_share_web_placeholder(&web_dir, &index_path);
        panic!(
            "file share web build completed but dist/file-share-web/index.html is not a built asset"
        );
    }
}

fn ensure_file_share_web_placeholder(web_dir: &Path, index_path: &Path) {
    if index_path.exists() {
        return;
    }

    fs::create_dir_all(web_dir).expect("create file share web asset dir");
    fs::write(index_path, placeholder_index_html()).expect("write file share web placeholder");
}

fn index_html_looks_built(index_path: &Path) -> bool {
    let Ok(html) = fs::read_to_string(index_path) else {
        return false;
    };

    html.contains("<div id=\"app\"></div>")
        && html.contains("/assets/")
        && !html.contains("/main.ts")
}

fn pnpm_command() -> &'static str {
    if cfg!(windows) {
        "pnpm.cmd"
    } else {
        "pnpm"
    }
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
