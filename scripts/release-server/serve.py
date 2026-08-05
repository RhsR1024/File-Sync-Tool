#!/usr/bin/env python3
"""Minimal release server for File Sync Tool."""

from __future__ import annotations

import argparse
import hashlib
import http.server
import json
import os
from pathlib import Path
import socketserver
import sys
import threading
from urllib.parse import urlsplit


MATERIAL_INDEX_PATH = '/virtual-device-assets/files.json'
MATERIAL_DIRECTORIES: dict[str, set[str]] = {}
MATERIAL_TREES = {
    'alarm-images': {'.jpg', '.jpeg', '.png'},
    'prepared-videos': {'.json', '.h264'},
}

ALARM_IMAGE_ROLES = {
    'person': {'scene', 'person'},
    'face': {'scene', 'face'},
    'car': {'scene', 'vehicle', 'plate'},
    'nonmotor': {'scene', 'nonmotor'},
}


def is_grouped_alarm_image(path: Path, material_root: Path) -> bool:
    parts = path.relative_to(material_root).parts
    if len(parts) != 4 or parts[0] != 'alarm-images':
        return False
    category, group, filename = parts[1:]
    suffix = group.removeprefix(f'{category}-')
    return (
        category in ALARM_IMAGE_ROLES
        and group.startswith(f'{category}-')
        and len(suffix) >= 3
        and suffix.isdigit()
        and Path(filename).stem in ALARM_IMAGE_ROLES[category]
    )


class ReleaseRequestHandler(http.server.SimpleHTTPRequestHandler):
    """Static release files plus an automatically generated loose-material list."""

    material_hash_cache: dict[str, tuple[int, int, str]] = {}
    material_hash_lock = threading.Lock()

    def do_GET(self) -> None:
        if urlsplit(self.path).path == MATERIAL_INDEX_PATH:
            self._serve_material_index(include_body=True)
            return
        super().do_GET()

    def do_HEAD(self) -> None:
        if urlsplit(self.path).path == MATERIAL_INDEX_PATH:
            self._serve_material_index(include_body=False)
            return
        super().do_HEAD()

    def _serve_material_index(self, *, include_body: bool) -> None:
        try:
            payload = json.dumps(
                {'files': self._material_files()},
                ensure_ascii=False,
                separators=(',', ':'),
            ).encode('utf-8')
        except OSError as error:
            self.send_error(500, f'failed to scan virtual device materials: {error}')
            return
        self.send_response(200)
        self.send_header('Content-Type', 'application/json; charset=utf-8')
        self.send_header('Content-Length', str(len(payload)))
        self.send_header('Cache-Control', 'no-store')
        self.end_headers()
        if include_body:
            self.wfile.write(payload)

    def _material_files(self) -> list[dict[str, object]]:
        release_root = Path.cwd().resolve()
        material_root = release_root / 'virtual-device-assets'
        files: list[dict[str, object]] = []
        for relative_directory, extensions in MATERIAL_DIRECTORIES.items():
            directory = material_root.joinpath(*relative_directory.split('/'))
            if not directory.is_dir():
                continue
            for path in directory.iterdir():
                if not path.is_file() or path.suffix.lower() not in extensions:
                    continue
                stat = path.stat()
                relative = path.relative_to(material_root).as_posix()
                files.append({
                    'path': relative,
                    'size': stat.st_size,
                    # This is a cache identity, not a signature or trust decision.
                    'content_id': self._content_id(path, stat.st_size, stat.st_mtime_ns),
                })
        for relative_directory, extensions in MATERIAL_TREES.items():
            directory = material_root.joinpath(*relative_directory.split('/'))
            if not directory.is_dir():
                continue
            for path in directory.rglob('*'):
                if not path.is_file() or path.suffix.lower() not in extensions:
                    continue
                if relative_directory == 'alarm-images' and not is_grouped_alarm_image(path, material_root):
                    continue
                tree_relative = path.relative_to(directory)
                if any(part.startswith('.') or part == 'staging' for part in tree_relative.parts):
                    continue
                stat = path.stat()
                relative = path.relative_to(material_root).as_posix()
                files.append({
                    'path': relative,
                    'size': stat.st_size,
                    'content_id': self._content_id(path, stat.st_size, stat.st_mtime_ns),
                })
        files.sort(key=lambda item: str(item['path']).casefold())
        return files

    @classmethod
    def _content_id(cls, path: Path, size: int, modified_ns: int) -> str:
        # Hashing a large prepared H.264 file is intentionally serialized: the first request
        # after a normal file change performs one streaming pass, while other
        # clients wait and then reuse the result instead of all hammering disk.
        with cls.material_hash_lock:
            key = str(path)
            cached = cls.material_hash_cache.get(key)
            if cached is not None and cached[:2] == (size, modified_ns):
                return cached[2]
            digest = hashlib.sha256()
            with path.open('rb') as source:
                for chunk in iter(lambda: source.read(1024 * 1024), b''):
                    digest.update(chunk)
            value = digest.hexdigest()
            cls.material_hash_cache[key] = (size, modified_ns, value)
            return value


def main() -> int:
    parser = argparse.ArgumentParser(description='Serve manifest.json and release .exe files.')
    parser.add_argument('port', nargs='?', type=int, default=8080)
    parser.add_argument('--port', dest='port_override', type=int)
    parser.add_argument('--bind', default='0.0.0.0')
    args = parser.parse_args()

    port = args.port_override if args.port_override is not None else args.port
    handler = ReleaseRequestHandler
    handler.extensions_map.setdefault('.json', 'application/json')

    with socketserver.ThreadingTCPServer((args.bind, port), handler) as httpd:
        httpd.daemon_threads = True
        cwd = os.getcwd()
        print(f'[file-sync-tool-release] serving {cwd} at http://{args.bind}:{port}')
        print('[file-sync-tool-release] Press Ctrl+C to stop.')
        try:
            httpd.serve_forever()
        except KeyboardInterrupt:
            print('\n[file-sync-tool-release] stopping.')
            return 0

    return 0


if __name__ == '__main__':
    sys.exit(main())
