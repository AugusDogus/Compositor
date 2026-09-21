#!/usr/bin/env python3
"""Verify wl_data_device on isolated, headless Mutter, without data-control or X11.

Requires Mutter, python-gobject, and dbus-run-session. Never touches the active
session's clipboard. Build the probe with:
  cargo build --locked --example wayland_clipboard_check
"""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


def stop(process):
    if process is not None and process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def check(runtime):
    from gi.repository import Gio, GLib

    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    probe = None
    with open(runtime / "mutter.log", "w+") as log:
        server = subprocess.Popen(
            ["mutter", "--wayland", "--headless", "--no-x11",
             "--virtual-monitor", "1400x900", "--wayland-display", "clipboard-test"],
            stdout=log, stderr=log,
        )
        try:
            for _ in range(100):
                ready = bus.call_sync(
                    "org.freedesktop.DBus", "/org/freedesktop/DBus",
                    "org.freedesktop.DBus", "NameHasOwner",
                    GLib.Variant("(s)", ("org.gnome.Mutter.RemoteDesktop",)),
                    None, Gio.DBusCallFlags.NONE, 1000, None,
                ).unpack()[0]
                if ready and (runtime / "clipboard-test").exists():
                    break
                if server.poll() is not None:
                    raise RuntimeError("Mutter exited before creating the test display")
                time.sleep(.1)
            else:
                raise RuntimeError("Mutter did not create its test display within ten seconds")
            session = bus.call_sync(
                "org.gnome.Mutter.RemoteDesktop", "/org/gnome/Mutter/RemoteDesktop",
                "org.gnome.Mutter.RemoteDesktop", "CreateSession", None, None,
                Gio.DBusCallFlags.NONE, 5000, None,
            ).unpack()[0]

            def call(method, args=None):
                return bus.call_sync(
                    "org.gnome.Mutter.RemoteDesktop", session,
                    "org.gnome.Mutter.RemoteDesktop.Session", method, args, None,
                    Gio.DBusCallFlags.NONE, 5000, None,
                )

            def input_event():
                # Real copy commands have fresh input serials. Mutter rejects
                # repeated synthetic set_selection calls with the same serial.
                for pressed in [True, False]:
                    call("NotifyKeyboardKeycode", GLib.Variant("(ub)", (42, pressed)))

            call("Start")
            input_event()
            env = dict(os.environ, WAYLAND_DISPLAY="clipboard-test",
                       COMPOSITOR_TEST_CLIPBOARD="wayland",
                       DBUS_SESSION_BUS_ADDRESS=f"unix:path={runtime}/no-portals")
            probe = subprocess.Popen(
                ["target/debug/examples/wayland_clipboard_check"], env=env,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            )
            deadline = time.monotonic() + 35
            while probe.poll() is None and time.monotonic() < deadline:
                input_event()
                time.sleep(.02)
            stdout, stderr = probe.communicate(timeout=5)
            print(stdout, end="")
            if probe.returncode:
                raise RuntimeError(stderr or f"Clipboard probe exited {probe.returncode}")
            call("Stop")
        except Exception:
            log.flush()
            log.seek(0)
            print(log.read(), file=sys.stderr)
            raise
        finally:
            stop(probe)
            stop(server)


if __name__ == "__main__":
    os.chdir(Path(__file__).resolve().parent.parent)
    if len(sys.argv) == 2 and sys.argv[1] == "--isolated":
        check(Path(os.environ["XDG_RUNTIME_DIR"]))
    elif len(sys.argv) == 1:
        with tempfile.TemporaryDirectory(prefix="compositor-clipboard-") as directory:
            env = dict(os.environ, XDG_RUNTIME_DIR=directory, XDG_CONFIG_HOME=directory,
                       GIO_USE_VFS="local")
            for name in ["DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS"]:
                env.pop(name, None)
            result = subprocess.run(
                ["dbus-run-session", "--", sys.executable, __file__, "--isolated"], env=env,
            )
            sys.exit(result.returncode)
    else:
        sys.exit("Run without arguments to create an isolated Wayland session")
