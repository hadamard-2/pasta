"""Reads the test probe's report and checks it. Only ever run inside nested-shell.sh.

Usage:
  probe_check.py wait hidden <pid> <timeout>
  probe_check.py wait shown <pid> <timeout>
  probe_check.py wait nudged <pid> <timeout> "<counts printed before>"
  probe_check.py counts <pid>
  probe_check.py ever-listed <pid>
"""
import json
import sys
import time

import gi

gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib  # noqa: E402


def report():
    connection = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    reply = connection.call_sync(
        "com.pasta.TestProbe", "/com/pasta/TestProbe", "com.pasta.TestProbe1", "Report",
        None, GLib.VariantType("(s)"), Gio.DBusCallFlags.NONE, 5000, None)
    return json.loads(reply.unpack()[0])


def windows_of(data, pid):
    return [w for w in data["windows"] if w["pid"] == pid]


def hidden(data, pid):
    """Returns None when every window of `pid` is hidden everywhere, else the reason it is not."""
    windows = windows_of(data, pid)
    if not windows:
        return f"no windows for pid {pid}"
    for w in windows:
        if not (w["skip"] and w["isSkip"] and not w["overview"]):
            return f"window not hidden: {w}"
    if any(pid in app["pids"] for app in data["running"]):
        return f"pid {pid} still in running apps: {data['running']}"
    app_ids = {w["appId"] for w in windows if w["appId"]}
    if app_ids & set(data["dash"]):
        return f"app {app_ids & set(data['dash'])} still in the dash: {data['dash']}"
    return None


def shown(data, pid):
    """Returns None when every window of `pid` is listed everywhere, else the reason it is not."""
    windows = windows_of(data, pid)
    if not windows:
        return f"no windows for pid {pid}"
    for w in windows:
        if w["skip"] or w["isSkip"] or not w["overview"]:
            return f"window hidden: {w}"
    if not any(pid in app["pids"] for app in data["running"]):
        return f"pid {pid} missing from running apps: {data['running']}"
    app_ids = {w["appId"] for w in windows if w["appId"]}
    if not app_ids & set(data["dash"]):
        return f"none of {app_ids} in the dash: {data['dash']}"
    return None


def counts(data, pid):
    key = str(pid)
    return data["appStateChanges"].get(key, 0), data["skipNotifies"].get(key, 0)


def nudged(before):
    app_before, skip_before = (int(n) for n in before.split())

    def check(data, pid):
        app_now, skip_now = counts(data, pid)
        if app_now > app_before and skip_now > skip_before:
            return None
        return f"no nudge for pid {pid}: app-state-changed {app_before}->{app_now}, notify::skip-taskbar {skip_before}->{skip_now}"

    return check


def wait(check, pid, timeout):
    deadline = time.monotonic() + timeout
    reason = "never checked"
    while time.monotonic() < deadline:
        try:
            reason = check(report(), pid)
        except GLib.Error as error:
            reason = f"probe unavailable: {error.message}"
        if reason is None:
            return 0
        time.sleep(0.3)
    print(f"FAIL after {timeout}s: {reason}", flush=True)
    return 1


def main(argv):
    command, *args = argv
    if command == "wait":
        state, pid, timeout, *rest = args
        check = {"hidden": hidden, "shown": shown}.get(state) or nudged(rest[0])
        return wait(check, int(pid), float(timeout))
    if command == "counts":
        print(*counts(report(), int(args[0])), flush=True)
        return 0
    if command == "ever-listed":
        print("yes" if str(int(args[0])) in report()["everListed"] else "no", flush=True)
        return 0
    print(f"unknown command {command}", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
