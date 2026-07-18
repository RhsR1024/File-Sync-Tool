pub fn sh_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

use super::inventory::InternalLayer;

pub enum PatchScriptOutput<'a> {
    NewFile { output_path: &'a str },
    Overwrite,
}

pub struct PatchScriptArgs<'a> {
    pub package_path: &'a str,
    pub replacement_path: &'a str,
    pub target_internal_path: &'a str,
    pub target_layer: Option<&'a InternalLayer>,
    pub output: PatchScriptOutput<'a>,
    pub workdir: &'a str,
}

pub fn bash_stdin_command(script: &str) -> String {
    format!("bash -s <<'__FST_REMOTE_PACKAGE_PATCH__'\n{script}\n__FST_REMOTE_PACKAGE_PATCH__")
}

pub fn build_scan_script(package_path: &str) -> String {
    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail
export LC_ALL=C

PKG={package_path}

stage() {{ printf '##STAGE:%s\n' "$1"; }}
log() {{ printf '##LOG:%s:%s\n' "$1" "$2"; }}
result() {{ printf '##RESULT:%s=%s\n' "$1" "$2"; }}
fail() {{
  printf '##ERROR:%s\n' "$1"
  exit 1
}}
raw() {{
  local layer="$1"
  while IFS= read -r line; do
    printf '##RAW:%s\t%s\n' "$layer" "$line"
  done
}}

stage scan_preflight
[ -f "$PKG" ] || fail "Package not found: $PKG"
command -v zstd >/dev/null 2>&1 || fail "zstd command is required"
command -v gzip >/dev/null 2>&1 || fail "gzip command is required"
command -v tar >/dev/null 2>&1 || fail "tar command is required"

PKG_DIR=$(cd "$(dirname -- "$PKG")" && pwd -P)
WORK="$PKG_DIR/.fst-scan-$(date +%s)-$$"
mkdir -p "$WORK"
cleanup() {{ rm -rf "$WORK"; }}
trap cleanup EXIT
result workdir "$WORK"

pkg_kb=$(du -k "$PKG" | awk '{{print $1; exit}}')
# gzip -l ISIZE wraps at 4GB, so keep the compressed-size heuristic as a floor.
unpacked_kb=$(gzip -l "$PKG" 2>/dev/null | awk 'NR==2 {{print int($2 / 1024) + 1}}')
[ -n "${{unpacked_kb:-}}" ] || unpacked_kb=0
avail_kb=$(df -Pk "$PKG_DIR" | awk 'NR==2 {{print $4}}')
need_kb=$((pkg_kb * 3))
est_kb=$((unpacked_kb * 2 + pkg_kb))
if [ "$est_kb" -gt "$need_kb" ]; then need_kb=$est_kb; fi
log info "Free space check: need $need_kb KB, available ${{avail_kb:-0}} KB"
if [ "${{avail_kb:-0}}" -lt "$need_kb" ]; then
  fail "Insufficient free space for scan: need $need_kb KB, available $avail_kb KB"
fi

stage scan_outer
gzip -dc "$PKG" > "$WORK/outer.tar"
tar -tvf "$WORK/outer.tar" | raw outer
mapfile -t middle_members < <(tar -tf "$WORK/outer.tar" | awk '/\.tar$/ {{print}}')
if [ "${{#middle_members[@]}}" -gt 1 ]; then
  printf '##LOG:error:Expected at most one middle .tar, found %s\n' "${{#middle_members[@]}}"
  for member in ${{middle_members[@]+"${{middle_members[@]}}"}}; do printf '##LOG:error:middle candidate: %s\n' "$member"; done
  fail "Package structure is not supported"
fi
if [ "${{#middle_members[@]}}" -eq 1 ]; then
  MIDDLE_TAR_PATH="${{middle_members[0]}}"
  tar -xOf "$WORK/outer.tar" "$MIDDLE_TAR_PATH" > "$WORK/middle.tar"
else
  # Some product packages are conventional tar.gz archives whose decompressed
  # tar directly contains the product files and optional *.tar.zst members.
  MIDDLE_TAR_PATH=""
  cp "$WORK/outer.tar" "$WORK/middle.tar"
  log info "No nested middle .tar found; scanning the outer tar directly"
fi
result middle_tar_path "$MIDDLE_TAR_PATH"

stage scan_middle
tar -tvf "$WORK/middle.tar" | raw middle

stage scan_inner
mapfile -t zst_members < <(tar -tf "$WORK/middle.tar" | awk '/\.tar\.zst$/ {{print}}')
for zst_path in ${{zst_members[@]+"${{zst_members[@]}}"}}; do
  log info "Scanning $zst_path"
  tar -xOf "$WORK/middle.tar" "$zst_path" | zstd --long=31 -dc | tar -tvf - | raw "zst:$zst_path"
done

stage scan_done
log info "Package scan complete"
"#,
        package_path = sh_quote(package_path),
    )
}

pub fn build_patch_script(args: PatchScriptArgs<'_>) -> String {
    let (target_layer_kind, target_zst_path) = match args.target_layer {
        Some(InternalLayer::Middle) => ("middle", ""),
        Some(InternalLayer::Zst { zst_path }) => ("zst", zst_path.as_str()),
        None => ("auto", ""),
    };
    let (output_mode, output_path) = match args.output {
        PatchScriptOutput::NewFile { output_path } => ("newFile", output_path),
        PatchScriptOutput::Overwrite => ("overwrite", ""),
    };

    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail
export LC_ALL=C

PKG={package_path}
REPLACEMENT={replacement_path}
TARGET_INTERNAL_PATH={target_internal_path}
TARGET_LAYER_KIND={target_layer_kind}
TARGET_ZST_PATH={target_zst_path}
OUTPUT_MODE={output_mode}
OUTPUT_PATH={output_path}
WORK={workdir}

SUCCESS=0
stage() {{ printf '##STAGE:%s\n' "$1"; }}
log() {{ printf '##LOG:%s:%s\n' "$1" "$2"; }}
result() {{ printf '##RESULT:%s=%s\n' "$1" "$2"; }}
fail() {{
  printf '##ERROR:%s\n' "$1"
  exit 1
}}
cleanup() {{
  if [ "$SUCCESS" -eq 1 ]; then
    rm -rf "$WORK"
  else
    log warn "Remote workdir kept for troubleshooting: $WORK"
  fi
}}
trap cleanup EXIT

normalize_member() {{
  local value="$1"
  while [[ "$value" == ./* ]]; do value="${{value#./}}"; done
  printf '%s' "$value"
}}

validate_member_path() {{
  local value="$1"
  [[ -n "$value" ]] || fail "Internal target path is empty"
  [[ "$value" != /* ]] || fail "Internal target path must be relative"
  case "$value" in
    *"/../"*|"../"*|*".."|".") fail "Internal target path must not contain .." ;;
  esac
}}

find_member_matches() {{
  local tarfile="$1"
  local wanted_norm
  wanted_norm=$(normalize_member "$2")
  while IFS= read -r member; do
    local member_norm
    member_norm=$(normalize_member "$member")
    if [[ "$member_norm" == "$wanted_norm" ]]; then
      printf '%s\n' "$member"
    fi
  done < <(tar -tf "$tarfile")
}}

resolve_one_member() {{
  local tarfile="$1"
  local wanted="$2"
  mapfile -t matches < <(find_member_matches "$tarfile" "$wanted")
  if [ "${{#matches[@]}}" -eq 0 ]; then
    return 1
  fi
  if [ "${{#matches[@]}}" -gt 1 ]; then
    # Callers capture stdout; report details on stderr so they reach the log stream.
    printf '##LOG:error:Multiple matches for %s\n' "$wanted" >&2
    for match in "${{matches[@]}}"; do printf '##LOG:error:%s\n' "$match" >&2; done
    return 2
  fi
  printf '%s' "${{matches[0]}}"
}}

perm_digit() {{
  local triple="$1"
  local value=0
  [[ "${{triple:0:1}}" == "r" ]] && value=$((value + 4))
  [[ "${{triple:1:1}}" == "w" ]] && value=$((value + 2))
  [[ "${{triple:2:1}}" == "x" || "${{triple:2:1}}" == "s" || "${{triple:2:1}}" == "t" ]] && value=$((value + 1))
  printf '%s' "$value"
}}

mode_from_perms() {{
  local perms="$1"
  printf '%s%s%s' "$(perm_digit "${{perms:1:3}}")" "$(perm_digit "${{perms:4:3}}")" "$(perm_digit "${{perms:7:3}}")"
}}

replace_tar_member() {{
  local tarfile="$1"
  local member="$2"
  local source_file="$3"
  local listing
  listing=$(tar -tvf "$tarfile" "$member" | head -n 1)
  [[ "$listing" == -* ]] || fail "Target member is not a regular file: $member"
  local perms owner_group owner group mode
  perms=$(awk '{{print $1}}' <<< "$listing")
  owner_group=$(awk '{{print $2}}' <<< "$listing")
  owner="${{owner_group%%/*}}"
  group="${{owner_group#*/}}"
  mode=$(mode_from_perms "$perms")

  local stage_dir="$WORK/stage-$RANDOM-$RANDOM"
  mkdir -p "$stage_dir/$(dirname -- "$member")"
  ln "$source_file" "$stage_dir/$member" 2>/dev/null || cp "$source_file" "$stage_dir/$member"
  tar --delete -f "$tarfile" "$member"
  tar --append -f "$tarfile" -C "$stage_dir" --owner="$owner" --group="$group" --mode="$mode" "$member"
  rm -rf "$stage_dir"
}}

is_manifest_member() {{
  local base lower
  base=$(basename -- "$1")
  lower=$(printf '%s' "$base" | tr '[:upper:]' '[:lower:]')
  [[ "$lower" =~ ^(.+\.)?md5(sum)?(\.txt)?$ ]]
}}

path_matches_manifest_row() {{
  local manifest="$1"
  local row_path="$2"
  local target="$3"
  local row_norm target_norm manifest_norm manifest_dir rel
  row_norm=$(normalize_member "$row_path")
  target_norm=$(normalize_member "$target")
  [[ "$row_norm" == "$target_norm" ]] && return 0
  manifest_norm=$(normalize_member "$manifest")
  manifest_dir=$(dirname -- "$manifest_norm")
  if [[ "$manifest_dir" != "." && "$target_norm" == "$manifest_dir/"* ]]; then
    rel="${{target_norm#"$manifest_dir/"}}"
    [[ "$row_norm" == "$rel" ]] && return 0
  fi
  return 1
}}

update_md5_manifests_in_tar() {{
  local tarfile="$1"
  local first_path="$2"
  local first_md5="$3"
  local pending_paths=("$first_path")
  local pending_md5s=("$first_md5")
  local manifests=()
  while IFS= read -r member; do
    if is_manifest_member "$member"; then
      manifests+=("$member")
    fi
  done < <(tar -tf "$tarfile")
  if [ "${{#manifests[@]}}" -eq 0 ]; then
    log warn "No md5 manifest found in archive layer"
    return 0
  fi
  mapfile -t manifests < <(printf '%s\n' "${{manifests[@]}}" | awk '{{print length($0) "\t" $0}}' | sort -rn | cut -f2-)

  local manifest
  for manifest in "${{manifests[@]}}"; do
    local source_file="$WORK/manifest-$RANDOM-$RANDOM"
    local next_file="$source_file.new"
    tar -xOf "$tarfile" "$manifest" > "$source_file" || continue
    local changed=0
    : > "$next_file"
    while IFS= read -r line || [ -n "$line" ]; do
      local out_line="$line"
      if [[ "$line" =~ ^([0-9A-Fa-f]{{32}})([[:space:]*]+)(.*)$ ]]; then
        local sep="${{BASH_REMATCH[2]}}"
        local row_path="${{BASH_REMATCH[3]}}"
        local index
        for index in "${{!pending_paths[@]}}"; do
          if path_matches_manifest_row "$manifest" "$row_path" "${{pending_paths[$index]}}"; then
            out_line="${{pending_md5s[$index]}}${{sep}}${{row_path}}"
            changed=1
            break
          fi
        done
      fi
      printf '%s\n' "$out_line" >> "$next_file"
    done < "$source_file"

    if [ "$changed" -eq 1 ]; then
      mv "$next_file" "$source_file"
      replace_tar_member "$tarfile" "$manifest" "$source_file"
      local manifest_md5
      manifest_md5=$(md5sum "$source_file" | awk '{{print $1}}')
      pending_paths+=("$manifest")
      pending_md5s+=("$manifest_md5")
      result updated_manifest "$manifest"
      log info "Updated md5 manifest $manifest"
    else
      rm -f "$next_file"
    fi
    rm -f "$source_file"
  done
}}

resolve_auto_target() {{
  mapfile -t middle_matches < <(find_member_matches "$WORK/middle.tar" "$TARGET_INTERNAL_PATH")
  mapfile -t zst_members < <(tar -tf "$WORK/middle.tar" | awk '/\.tar\.zst$/ {{print}}')
  local zst_matches=()
  local zst_path
  for zst_path in ${{zst_members[@]+"${{zst_members[@]}}"}}; do
    if tar -xOf "$WORK/middle.tar" "$zst_path" | zstd --long=31 -dc | tar -tf - | while IFS= read -r member; do
      if [[ "$(normalize_member "$member")" == "$(normalize_member "$TARGET_INTERNAL_PATH")" ]]; then
        printf '%s\t%s\n' "$zst_path" "$member"
      fi
    done > "$WORK/zst-match.tmp"; then
      while IFS= read -r match; do zst_matches+=("$match"); done < "$WORK/zst-match.tmp"
    fi
  done
  local total=$(( ${{#middle_matches[@]}} + ${{#zst_matches[@]}} ))
  if [ "$total" -eq 0 ]; then
    fail "Target path not found in package: $TARGET_INTERNAL_PATH"
  fi
  if [ "$total" -gt 1 ]; then
    log error "Target path is ambiguous; select an explicit candidate"
    for match in ${{middle_matches[@]+"${{middle_matches[@]}}"}}; do log error "middle:$match"; done
    for match in ${{zst_matches[@]+"${{zst_matches[@]}}"}}; do log error "zst:$match"; done
    fail "Ambiguous target path"
  fi
  if [ "${{#middle_matches[@]}}" -eq 1 ]; then
    RESOLVED_LAYER_KIND=middle
    RESOLVED_TARGET="${{middle_matches[0]}}"
    RESOLVED_ZST=""
  else
    RESOLVED_LAYER_KIND=zst
    RESOLVED_ZST="${{zst_matches[0]%%$'\t'*}}"
    RESOLVED_TARGET="${{zst_matches[0]#*$'\t'}}"
  fi
}}

verify_final_target() {{
  stage verify
  gzip -dc "$WORK/output.tar.gz" > "$WORK/verify_outer.tar"
  if [ "$MIDDLE_IS_OUTER" -eq 1 ]; then
    cp "$WORK/verify_outer.tar" "$WORK/verify_middle.tar"
  else
    tar -xOf "$WORK/verify_outer.tar" "$MIDDLE_TAR_PATH" > "$WORK/verify_middle.tar"
  fi
  local actual_md5
  if [ "$RESOLVED_LAYER_KIND" = "zst" ]; then
    tar -xOf "$WORK/verify_middle.tar" "$RESOLVED_ZST" | zstd --long=31 -dc > "$WORK/verify_inner.tar"
    actual_md5=$(tar -xOf "$WORK/verify_inner.tar" "$RESOLVED_TARGET" | md5sum | awk '{{print $1}}')
  else
    actual_md5=$(tar -xOf "$WORK/verify_middle.tar" "$RESOLVED_TARGET" | md5sum | awk '{{print $1}}')
  fi
  if [ "$actual_md5" != "$REPLACEMENT_MD5" ]; then
    fail "Final package target md5 mismatch: expected $REPLACEMENT_MD5 got $actual_md5"
  fi
  result target_md5 "$actual_md5"
}}

stage preflight
validate_member_path "$TARGET_INTERNAL_PATH"
[ -f "$PKG" ] || fail "Package not found: $PKG"
[ -f "$REPLACEMENT" ] || fail "Replacement file not found after upload: $REPLACEMENT"
command -v zstd >/dev/null 2>&1 || fail "zstd command is required"
command -v gzip >/dev/null 2>&1 || fail "gzip command is required"
command -v tar >/dev/null 2>&1 || fail "tar command is required"
command -v md5sum >/dev/null 2>&1 || fail "md5sum command is required"
mkdir -p "$WORK"
PKG_DIR=$(cd "$(dirname -- "$PKG")" && pwd -P)
if [ "$OUTPUT_MODE" = "newFile" ]; then
  [ -n "$OUTPUT_PATH" ] || fail "Output path is required"
  [ ! -e "$OUTPUT_PATH" ] || fail "Output path already exists: $OUTPUT_PATH"
fi
pkg_kb=$(du -k "$PKG" | awk '{{print $1; exit}}')
# gzip -l ISIZE wraps at 4GB, so keep the compressed-size heuristic as a floor.
unpacked_kb=$(gzip -l "$PKG" 2>/dev/null | awk 'NR==2 {{print int($2 / 1024) + 1}}')
[ -n "${{unpacked_kb:-}}" ] || unpacked_kb=0
avail_kb=$(df -Pk "$PKG_DIR" | awk 'NR==2 {{print $4}}')
need_kb=$((pkg_kb * 4))
est_kb=$((unpacked_kb * 3 + pkg_kb))
if [ "$est_kb" -gt "$need_kb" ]; then need_kb=$est_kb; fi
log info "Free space check: need $need_kb KB, available ${{avail_kb:-0}} KB"
if [ "${{avail_kb:-0}}" -lt "$need_kb" ]; then
  fail "Insufficient free space for patch: need $need_kb KB, available $avail_kb KB"
fi
REPLACEMENT_MD5=$(md5sum "$REPLACEMENT" | awk '{{print $1}}')
result replacement_md5 "$REPLACEMENT_MD5"
result workdir "$WORK"

stage unpack_outer
gzip -dc "$PKG" > "$WORK/outer.tar"
mapfile -t middle_members < <(tar -tf "$WORK/outer.tar" | awk '/\.tar$/ {{print}}')
if [ "${{#middle_members[@]}}" -gt 1 ]; then
  fail "Expected at most one middle .tar, found ${{#middle_members[@]}}"
fi
if [ "${{#middle_members[@]}}" -eq 1 ]; then
  MIDDLE_IS_OUTER=0
  MIDDLE_TAR_PATH="${{middle_members[0]}}"
  stage extract_middle
  tar -xOf "$WORK/outer.tar" "$MIDDLE_TAR_PATH" > "$WORK/middle.tar"
else
  MIDDLE_IS_OUTER=1
  MIDDLE_TAR_PATH=""
  cp "$WORK/outer.tar" "$WORK/middle.tar"
  log info "No nested middle .tar found; patching the outer tar directly"
fi
result middle_tar_path "$MIDDLE_TAR_PATH"

stage resolve_target
RESOLVED_LAYER_KIND="$TARGET_LAYER_KIND"
RESOLVED_TARGET="$TARGET_INTERNAL_PATH"
RESOLVED_ZST="$TARGET_ZST_PATH"
if [ "$TARGET_LAYER_KIND" = "auto" ]; then
  resolve_auto_target
elif [ "$TARGET_LAYER_KIND" = "middle" ]; then
  rc=0
  RESOLVED_TARGET=$(resolve_one_member "$WORK/middle.tar" "$TARGET_INTERNAL_PATH") || rc=$?
  [ "$rc" -ne 2 ] || fail "Target path is ambiguous in middle tar: $TARGET_INTERNAL_PATH"
  [ "$rc" -eq 0 ] || fail "Target path not found in middle tar: $TARGET_INTERNAL_PATH"
elif [ "$TARGET_LAYER_KIND" = "zst" ]; then
  [ -n "$TARGET_ZST_PATH" ] || fail "zst layer path is required"
  rc=0
  RESOLVED_ZST=$(resolve_one_member "$WORK/middle.tar" "$TARGET_ZST_PATH") || rc=$?
  [ "$rc" -ne 2 ] || fail "zst member is ambiguous in middle tar: $TARGET_ZST_PATH"
  [ "$rc" -eq 0 ] || fail "zst member not found: $TARGET_ZST_PATH"
else
  fail "Unknown target layer kind: $TARGET_LAYER_KIND"
fi
result resolved_layer "$RESOLVED_LAYER_KIND"
result resolved_target "$RESOLVED_TARGET"
[ -z "$RESOLVED_ZST" ] || result resolved_zst "$RESOLVED_ZST"

if [ "$RESOLVED_LAYER_KIND" = "zst" ]; then
  stage extract_inner
  tar -xOf "$WORK/middle.tar" "$RESOLVED_ZST" > "$WORK/inner.tar.zst"
  zstd --long=31 -d -q -f "$WORK/inner.tar.zst" -o "$WORK/inner.tar"
  rc=0
  RESOLVED_TARGET=$(resolve_one_member "$WORK/inner.tar" "$RESOLVED_TARGET") || rc=$?
  [ "$rc" -ne 2 ] || fail "Target path is ambiguous in inner tar: $TARGET_INTERNAL_PATH"
  [ "$rc" -eq 0 ] || fail "Target path not found in inner tar: $TARGET_INTERNAL_PATH"
  result resolved_target "$RESOLVED_TARGET"

  stage replace_member
  replace_tar_member "$WORK/inner.tar" "$RESOLVED_TARGET" "$REPLACEMENT"

  stage update_md5
  update_md5_manifests_in_tar "$WORK/inner.tar" "$RESOLVED_TARGET" "$REPLACEMENT_MD5"

  stage repack_inner
  zstd -19 -T0 --long=31 -q -f "$WORK/inner.tar" -o "$WORK/new-inner.tar.zst"
  ZST_MD5=$(md5sum "$WORK/new-inner.tar.zst" | awk '{{print $1}}')
  replace_tar_member "$WORK/middle.tar" "$RESOLVED_ZST" "$WORK/new-inner.tar.zst"
  update_md5_manifests_in_tar "$WORK/middle.tar" "$RESOLVED_ZST" "$ZST_MD5"
else
  stage replace_member
  replace_tar_member "$WORK/middle.tar" "$RESOLVED_TARGET" "$REPLACEMENT"

  stage update_md5
  update_md5_manifests_in_tar "$WORK/middle.tar" "$RESOLVED_TARGET" "$REPLACEMENT_MD5"
fi

stage repack_middle
if [ "$MIDDLE_IS_OUTER" -eq 1 ]; then
  cp "$WORK/middle.tar" "$WORK/outer.tar"
else
  MIDDLE_MD5=$(md5sum "$WORK/middle.tar" | awk '{{print $1}}')
  replace_tar_member "$WORK/outer.tar" "$MIDDLE_TAR_PATH" "$WORK/middle.tar"
  update_md5_manifests_in_tar "$WORK/outer.tar" "$MIDDLE_TAR_PATH" "$MIDDLE_MD5"
fi

stage compress_outer
gzip -c "$WORK/outer.tar" > "$WORK/output.tar.gz"
verify_final_target

if [ "$OUTPUT_MODE" = "overwrite" ]; then
  stage backup_overwrite
  BACKUP_PATH="$PKG.bak-$(date +%Y%m%d%H%M%S)"
  cp -p "$PKG" "$BACKUP_PATH"
  mv -f "$WORK/output.tar.gz" "$PKG"
  result backup_path "$BACKUP_PATH"
  result output_path "$PKG"
else
  stage finalize
  mv "$WORK/output.tar.gz" "$OUTPUT_PATH"
  result output_path "$OUTPUT_PATH"
fi

stage cleanup
SUCCESS=1
log info "Remote package patch complete"
"#,
        package_path = sh_quote(args.package_path),
        replacement_path = sh_quote(args.replacement_path),
        target_internal_path = sh_quote(args.target_internal_path),
        target_layer_kind = sh_quote(target_layer_kind),
        target_zst_path = sh_quote(target_zst_path),
        output_mode = sh_quote(output_mode),
        output_path = sh_quote(output_path),
        workdir = sh_quote(args.workdir),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sh_quote_wraps_plain_paths() {
        assert_eq!(sh_quote("/opt/pkg"), "'/opt/pkg'");
        assert_eq!(sh_quote("/opt/my pkg/a.tar.gz"), "'/opt/my pkg/a.tar.gz'");
    }

    #[test]
    fn sh_quote_escapes_single_quotes() {
        assert_eq!(sh_quote("a'b"), r"'a'\''b'");
    }

    #[test]
    fn sh_quote_handles_empty_string() {
        assert_eq!(sh_quote(""), "''");
    }

    #[test]
    fn bash_stdin_command_wraps_script_in_literal_heredoc() {
        let command = bash_stdin_command("echo ok");
        assert!(command.contains("bash -s <<'__FST_REMOTE_PACKAGE_PATCH__'"));
        assert!(command.contains("echo ok"));
    }

    #[test]
    fn scan_script_contains_expected_pipeline_markers() {
        let script = build_scan_script("/tmp/pkg.tar.gz");
        assert!(script.contains("##RAW:%s\\t%s"));
        assert!(script.contains("gzip -dc"));
        assert!(script.contains("zstd --long=31 -dc"));
        assert_eq!(script.matches("zstd --long=31 -dc").count(), 1);
        assert!(script.contains("scan_done"));
        assert!(!script.contains("@PACKAGE_PATH@"));
        // Uncompressed-size based space estimate and bash<4.4 empty-array guard.
        assert!(script.contains("gzip -l"));
        assert!(script.contains("${zst_members[@]+"));
        assert!(script.contains("scanning the outer tar directly"));
        assert!(script.contains("Expected at most one middle .tar"));
    }

    #[test]
    fn patch_script_contains_safety_and_md5_steps() {
        let script = build_patch_script(PatchScriptArgs {
            package_path: "/tmp/pkg.tar.gz",
            replacement_path: "/tmp/work/libdemo.so",
            target_internal_path: "app/demo/bin/libdemo.so",
            target_layer: Some(&InternalLayer::Zst {
                zst_path: "pkg/app/demo.tar.zst".into(),
            }),
            output: PatchScriptOutput::NewFile {
                output_path: "/tmp/pkg.patched.tar.gz",
            },
            workdir: "/tmp/work",
        });
        assert!(script.contains("set -euo pipefail"));
        assert!(script.contains("update_md5_manifests_in_tar"));
        assert!(script.contains("backup_overwrite"));
        assert!(script.contains("Final package target md5 mismatch"));
        assert!(script.contains("'pkg/app/demo.tar.zst'"));
        assert!(script.contains("gzip -l"));
        assert!(script.contains("${zst_members[@]+"));
        assert!(script.contains("is ambiguous"));
        assert!(script.contains("MIDDLE_IS_OUTER=1"));
        assert!(script.contains("patching the outer tar directly"));
        assert_eq!(script.matches("zstd --long=31 -dc").count(), 2);
        assert!(script.contains("zstd --long=31 -d -q -f"));
        assert!(script.contains("zstd -19 -T0 --long=31 -q -f"));
    }
}
