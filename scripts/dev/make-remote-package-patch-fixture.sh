#!/usr/bin/env bash
set -euo pipefail

OUT_DIR="${1:-/tmp/fst-rpp-fixture}"
rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR/root/app/demo/bin" "$OUT_DIR/work"

printf 'old library\n' > "$OUT_DIR/root/app/demo/bin/libdemo.so"
(
  cd "$OUT_DIR/root"
  md5sum app/demo/bin/libdemo.so > md5
  tar -cf "$OUT_DIR/work/inner.tar" app md5
)
zstd -q -f "$OUT_DIR/work/inner.tar" -o "$OUT_DIR/work/demo.tar.zst"

mkdir -p "$OUT_DIR/middle/pkg/app"
cp "$OUT_DIR/work/demo.tar.zst" "$OUT_DIR/middle/pkg/app/demo.tar.zst"
(
  cd "$OUT_DIR/middle"
  md5sum pkg/app/demo.tar.zst > pkg/md5
  tar -cf "$OUT_DIR/work/middle.tar" pkg
)

mkdir -p "$OUT_DIR/outer/VMS"
cp "$OUT_DIR/work/middle.tar" "$OUT_DIR/outer/VMS/VMS.tar"
(
  cd "$OUT_DIR/outer"
  tar -czf "$OUT_DIR/VMS-fixture.tar.gz" VMS
)

printf 'Fixture written: %s\n' "$OUT_DIR/VMS-fixture.tar.gz"
printf 'Replacement target: app/demo/bin/libdemo.so in zst layer pkg/app/demo.tar.zst\n'
