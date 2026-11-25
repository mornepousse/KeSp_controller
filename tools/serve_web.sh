#!/bin/sh
# Serves the browser version on this computer. Open http://localhost:8000 in
# Chrome or Edge, then press Connect. Ctrl+C stops it.
cd "$(dirname "$0")/../web" && exec python3 -m http.server 8000 --bind 127.0.0.1
