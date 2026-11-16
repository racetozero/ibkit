"""GatewayClient against a real ``ibkit gateway run`` with a stand-in launcher."""

from __future__ import annotations

import json
import os
import socket
import subprocess
import time
from collections.abc import Iterator
from pathlib import Path

import pytest
from ibkit import IbkitError
from ibkit.gateway import Control, ErrorCode, GatewayClient, GatewayError, State

BINARY = Path(__file__).resolve().parents[3] / 'target' / 'debug' / 'ibkit'
TOKEN = 'python-test-token-0123456789'


@pytest.fixture
def controller(
    tmp_path: Path,
) -> Iterator[tuple[GatewayClient, subprocess.Popen[bytes]]]:
    if not BINARY.is_file():
        pytest.skip('build the controller first: cargo build -p ibkit-cli')
    launcher = tmp_path / 'launcher'
    launcher.write_text('#!/bin/sh\nexec sleep 60\n')
    launcher.chmod(0o700)
    with socket.socket() as probe:
        probe.bind(('127.0.0.1', 0))
        port: int = probe.getsockname()[1]
    config = tmp_path / 'config.toml'
    config.write_text(
        f'executable = {json.dumps(str(launcher))}\n'
        f'agent = {json.dumps(str(launcher))}\n'
        f'state_dir = {json.dumps(str(tmp_path / "state"))}\n'
        f'settings_dir = {json.dumps(str(tmp_path / "settings"))}\n'
        'auto_login = false\nshutdown_timeout_secs = 1\n'
        f'control_listen = "127.0.0.1:{port}"\n'
    )
    process = subprocess.Popen(
        [str(BINARY), 'gateway', '--config', str(config), 'run'],
        env={**os.environ, 'IBKIT_CONTROL_TOKEN': TOKEN},
        stdout=subprocess.DEVNULL,
    )
    client = GatewayClient(f'http://127.0.0.1:{port}', TOKEN)
    deadline = time.monotonic() + 10
    while True:
        try:
            if client.health()['live']:
                break
        except (IbkitError, ConnectionError):
            pass
        if time.monotonic() > deadline:
            process.kill()
            pytest.fail('the controller did not start')
        time.sleep(0.05)
    yield client, process
    process.kill()
    process.wait()


def test_status_settings_events_and_stop(
    controller: tuple[GatewayClient, subprocess.Popen[bytes]],
) -> None:
    client, process = controller
    status = client.status()
    assert status['controller_pid'] == process.pid
    assert isinstance(status['state'], State)
    assert status['last_start_result']['error_code'] is ErrorCode.NONE
    assert client.ready()['ready'] is False
    assert 'ibkit_gateway_up 1' in client.metrics()
    assert '/v1/metrics' in client.openapi()['paths']

    with pytest.raises(GatewayError, match='bearer token') as refused:
        GatewayClient(client.url, 'wrong-token-0123456789').status()
    assert refused.value.status == 401
    with pytest.raises(IbkitError, match='base32'):
        client.setTotpSecret('123456')
    with pytest.raises(IbkitError, match='unknown setting'):
        client.updateConfig({'no_such_key': 1})
    with pytest.raises(ValueError):
        client.control('jump')

    events = client.events()
    assert client.updateConfig({'api_port': 4100})['api_port'] == 4100
    deadline = time.monotonic() + 10
    while True:
        event = events.recv(timeout=max(0.0, deadline - time.monotonic()))
        assert event is not None, 'no Started event after the reload'
        if event['event'] == 'state_changed':
            assert isinstance(event['state'], State)
        if event['event'] == 'started' and event['session'] != status['session']:
            break

    # DELETE /v1/config removes the saved setting; the default applies.
    assert client.clearConfig()['api_port'] == 4002
    while client.status()['state'] == State.EXITED.value:
        time.sleep(0.05)
    client.control(Control.STOP)
    assert process.wait(timeout=10) == 0


async def test_async_methods(
    controller: tuple[GatewayClient, subprocess.Popen[bytes]],
) -> None:
    client, process = controller
    assert (await client.statusAsync())['controller_pid'] == process.pid
    assert (await client.configAsync())['control_listen'] is not None
    assert 'ibkit_gateway_ready 0' in await client.metricsAsync()
