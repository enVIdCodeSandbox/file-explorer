#!/usr/bin/env bash
# Starts local FTP (2121), WebDAV (8081) and SFTP (2222) servers for the
# remote tests, runs them, and stops the servers again.
#   login: tester / secret
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
work=/tmp/fast-test-servers
mkdir -p "$work/ftproot" "$work/davroot" "$work/sftproot"
if [ ! -x "$work/venv/bin/python" ]; then
  python3 -m venv "$work/venv"
  "$work/venv/bin/pip" install -q pyftpdlib wsgidav cheroot asyncssh
fi
"$work/venv/bin/python" "$here/test-servers.py" > "$work/servers.log" 2>&1 &
pid=$!
trap 'kill $pid 2>/dev/null || true' EXIT
for port in 2121 8081 2222; do
  for _ in $(seq 50); do (echo > /dev/tcp/127.0.0.1/$port) 2>/dev/null && break; sleep 0.2; done
done
cd "$here/.." && FAST_TEST_SERVERS=1 cargo test -p fast-core --test remote_servers "$@"
