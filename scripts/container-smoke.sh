#!/bin/sh
set -eu
image=${1:-ibkit-rs:reliability}
suffix="$$"
network="ibkit-smoke-$suffix"
gateway="ibkit-smoke-$suffix"
server="ibkit-api-fixture-$suffix"
cleanup() {
    docker rm -f "$server" "$gateway" >/dev/null 2>&1 || true
    docker network rm "$network" >/dev/null 2>&1 || true
}
trap cleanup EXIT
docker network create "$network" >/dev/null
docker run -d --name "$gateway" --network "$network" --entrypoint /bin/sh "$image" -c '
sed -e "s/auto_login = true/auto_login = false/" -e "s/initialization_timeout_secs = 900/initialization_timeout_secs = 120/" -e "s/shutdown_timeout_secs = 30/shutdown_timeout_secs = 5/" /etc/ibkit/config.toml > /tmp/observe.toml
export IBKIT_CONFIG=/tmp/observe.toml
exec /usr/bin/tini -g -- ibkit gateway run' >/dev/null
ready=false
for _ in $(seq 1 60); do
    if docker logs "$gateway" 2>&1 | grep -q '"kind":"login"'; then ready=true; break; fi
    sleep 1
done
if [ "$ready" != true ]; then docker logs "$gateway"; echo 'Gateway did not display its login screen within 60 seconds.' >&2; exit 1; fi
docker exec "$gateway" ibkit gateway --config /tmp/observe.toml health --kind live
if docker exec "$gateway" ibkit gateway --config /tmp/observe.toml health --kind ready 2>/dev/null; then
    echo 'An unauthenticated Gateway must fail its ready probe.' >&2; exit 1
fi
# A test server shares Gateway's network namespace and binds only loopback.
# A separate client must reach it through the production Rust forwarder.
docker run -d --name "$server" --network "container:$gateway" python:3.13-slim python -u -c '
import socket
with socket.socket() as server:
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server.bind(("127.0.0.1", 4002)); server.listen()
    with server.accept()[0] as client:
        data = client.recv(1024)
        assert data == b"forward-test\x00"
        client.sendall(b"forward-reply\x00")
' >/dev/null
docker run --rm --network "$network" python:3.13-slim python -c '
import socket, sys, time
for attempt in range(20):
    try:
        with socket.create_connection((sys.argv[1], 4004), timeout=3) as client:
            client.sendall(b"forward-test\x00")
            assert client.recv(1024) == b"forward-reply\x00"
        break
    except (ConnectionError, TimeoutError):
        if attempt == 19: raise
        time.sleep(1)
' "$gateway"
docker stop --time 15 "$gateway" >/dev/null
code=$(docker inspect --format '{{.State.ExitCode}}' "$gateway")
test "$code" = 0
echo 'Real Gateway startup, probes, external forwarding, and SIGTERM checks passed.'
