"""Run with uv run scripts/check-tui.py [path/to/ibkit]. No account is used."""
from __future__ import annotations

import fcntl
import json
import os
import pty
import select
import signal
import struct
import sys
import tempfile
import termios
import time
import tomllib
from collections.abc import Callable
from pathlib import Path


def main() -> None:
    binary = Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/ibkit").resolve()
    with tempfile.TemporaryDirectory(prefix="ibkit-tui-check-") as temporary:
        root = Path(temporary)
        launcher = root / "launcher"
        launcher.write_text("#!/bin/sh\nexec sleep 60\n")
        launcher.chmod(0o700)
        config = root / "config.toml"
        state = root / "state"
        settings = root / "settings"
        config.write_text(
            f"executable = {json.dumps(str(launcher))}\n"
            f"agent = {json.dumps(str(launcher))}\n"
            f"state_dir = {json.dumps(str(state))}\n"
            f"settings_dir = {json.dumps(str(settings))}\n"
            "auto_login = false\nshutdown_timeout_secs = 1\n"
        )
        pid, terminal = pty.fork()
        if pid == 0:
            os.environ["TERM"] = "xterm-256color"
            os.execv(str(binary), [str(binary), "gateway", "--config", str(config), "tui"])
        fcntl.ioctl(terminal, termios.TIOCSWINSZ, struct.pack("HHHH", 34, 110, 0, 0))
        output = bytearray()
        complete = False
        reaped = False

        def drain() -> None:
            if select.select([terminal], [], [], 0.05)[0]:
                try:
                    chunk = os.read(terminal, 65536)
                except OSError:
                    return
                output.extend(chunk)
                # Terminal cursor query, if the platform emits one.
                if b"\x1b[6n" in chunk:
                    os.write(terminal, b"\x1b[1;1R")

        def until(condition: Callable[[], bool], seconds: float = 8) -> None:
            deadline = time.monotonic() + seconds
            while not condition():
                if time.monotonic() >= deadline:
                    raise AssertionError("The TUI did not reach the expected state within its limit")
                drain()

        def status_is(expected: str, previous_session: str | None = None) -> bool:
            try:
                status = json.loads((state / "status.json").read_text())
                return status["state"] == expected and (
                    previous_session is None or status["session"] != previous_session
                )
            except (FileNotFoundError, json.JSONDecodeError):
                return False

        try:
            until(lambda: b"ibkit" in output and b"Username" in output)
            until(lambda: not termios.tcgetattr(terminal)[3] & termios.ECHO)
            # Enter credentials with bracketed paste; password must never echo.
            os.write(terminal, b"\r\x1b[200~fixture-user\x1b[201~\r\x1b[B\r")
            os.write(terminal, b"\x1b[200~fixture-secret\x1b[201~\rs")  # save
            until(lambda: "api_port" in config.read_text())
            saved = tomllib.loads(config.read_text())
            assert "username_file" not in saved and "password_file" not in saved
            assert "fixture-secret" not in config.read_text()
            assert config.stat().st_mode & 0o777 == 0o600
            os.write(terminal, b"r")  # run
            until(lambda: status_is("starting"))
            controller = json.loads((state / "status.json").read_text())["controller_pid"]
            launch = json.loads((state / "launch.json").read_text())
            username_file = Path(launch["config"]["username_file"])
            password_file = Path(launch["config"]["password_file"])
            assert username_file.read_text() == "fixture-user"
            assert password_file.read_text() == "fixture-secret"
            original_session = launch["session"]
            os.write(terminal, b"c")  # cold restart
            until(lambda: status_is("starting", original_session))
            os.write(terminal, b"x")  # stop
            until(lambda: status_is("exited"))
            until(lambda: not username_file.exists() and not password_file.exists())
            os.write(terminal, b"q\r")
            deadline = time.monotonic() + 8
            exit_status: int | None = None
            while time.monotonic() < deadline:
                drain()
                child, value = os.waitpid(pid, os.WNOHANG)
                if child:
                    exit_status = value
                    reaped = True
                    break
            assert exit_status is not None and os.waitstatus_to_exitcode(exit_status) == 0
            assert b"fixture-secret" not in output and b"fixture-user" not in output
            assert b"\x1b[?1049l" in output, "The alternate terminal screen was not restored"
            assert b"\x1b[?2004l" in output, "Bracketed paste was not disabled"
            assert not username_file.exists() and not password_file.exists()
            try:
                os.kill(controller, 0)
            except ProcessLookupError:
                pass
            else:
                raise AssertionError("The TUI left its controller running")
            complete = True
            print("TUI credential masking, private save, owned Start/Stop, and terminal restoration passed.")
        finally:
            if not complete and not reaped:
                try:
                    os.kill(pid, signal.SIGTERM)
                    deadline = time.monotonic() + 4
                    while time.monotonic() < deadline:
                        drain()
                        child, _ = os.waitpid(pid, os.WNOHANG)
                        if child:
                            reaped = True
                            break
                finally:
                    if not reaped:
                        try:
                            os.kill(pid, signal.SIGKILL)
                        except ProcessLookupError:
                            pass
                        os.waitpid(pid, 0)
            os.close(terminal)


if __name__ == "__main__":
    main()
