#!/usr/bin/env python3
"""Minimal release server for File Sync Tool."""

from __future__ import annotations

import argparse
import http.server
import os
import socketserver
import sys


def main() -> int:
    parser = argparse.ArgumentParser(description='Serve manifest.json and release .exe files.')
    parser.add_argument('port', nargs='?', type=int, default=8080)
    parser.add_argument('--port', dest='port_override', type=int)
    parser.add_argument('--bind', default='0.0.0.0')
    args = parser.parse_args()

    port = args.port_override if args.port_override is not None else args.port
    handler = http.server.SimpleHTTPRequestHandler
    handler.extensions_map.setdefault('.json', 'application/json')

    with socketserver.TCPServer((args.bind, port), handler) as httpd:
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
