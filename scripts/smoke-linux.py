"""Verify a packaged app renders and exits within five seconds of native close."""
import os
from pathlib import Path
import signal
import re
import struct
import subprocess
import sys
import tempfile
import time
from Xlib import X, display as xdisplay, protocol

binary = Path(sys.argv[1]).resolve()
root = Path(tempfile.mkdtemp(prefix="mpc-package-smoke-"))
framebuffer = root / "framebuffer"
framebuffer.mkdir()
read_fd, write_fd = os.pipe()
app = None
with (root / "xvfb.log").open("w") as xlog, (root / "app.log").open("w") as app_log:
    xserver = subprocess.Popen(
        ["Xvfb", "-displayfd", str(write_fd), "-screen", "0", "1100x820x24",
         "-nolisten", "tcp", "-ac", "-fbdir", str(framebuffer)],
        pass_fds=(write_fd,), stdout=subprocess.DEVNULL, stderr=xlog,
    )
    os.close(write_fd)
    try:
        with os.fdopen(read_fd) as stream:
            display = ":" + stream.readline().strip()
        assert display != ":", "Xvfb did not start"
        env = os.environ.copy()
        env.pop("WAYLAND_DISPLAY", None)
        env.update(DISPLAY=display, GDK_BACKEND="x11")
        for variable in ["XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_CONFIG_HOME", "XDG_RUNTIME_DIR"]:
            path = root / variable.lower()
            path.mkdir(mode=0o700)
            env[variable] = str(path)
        app = subprocess.Popen(
            ["dbus-run-session", "--", str(binary)], env=env,
            stdout=app_log, stderr=subprocess.STDOUT, start_new_session=True,
        )
        for _ in range(30):
            time.sleep(1)
            assert app.poll() is None, "App exited during startup"
            windows = subprocess.check_output(["xwininfo", "-root", "-tree"], env=env, text=True)
            if '"MPC Autofill"' in windows:
                window = re.search(r'(0x[0-9a-f]+) "MPC Autofill"', windows).group(1)
                break
        else:
            raise AssertionError("App did not create its main window")
        time.sleep(5)
        assert app.poll() is None, "App exited after opening its window"
        data = (framebuffer / "Xvfb_screen0").read_bytes()
        fields = struct.unpack(">25I", data[:100])
        header, version = fields[:2]
        width, height, bits, stride, colors = fields[4], fields[5], fields[11], fields[12], fields[19]
        assert version == 7 and bits == 32, "Unexpected framebuffer format"
        offset = header + colors * 12
        pixels = set()
        for row in range(height):
            start = offset + row * stride
            pixels.update(data[index:index + 4] for index in range(start, start + width * 4, 4))
        assert len(pixels) > 100, "App window did not render content"
        # WM_DELETE_WINDOW is the native close request; do not terminate the
        # process to satisfy the smoke test. Xvfb needs no window manager.
        connection = xdisplay.Display(display)
        target = connection.create_resource_object("window", int(window, 16))
        target.send_event(protocol.event.ClientMessage(
            window=target,
            client_type=connection.intern_atom("WM_PROTOCOLS"),
            data=(32, [connection.intern_atom("WM_DELETE_WINDOW"), X.CurrentTime, 0, 0, 0]),
        ), event_mask=0)
        connection.flush()
        connection.close()
        assert app.wait(timeout=5) == 0, "App failed during native close"
        print("PASS: Linux package rendered and closed cleanly within five seconds.")
    except Exception:
        app_log.flush()
        print((root / "app.log").read_text(), file=sys.stderr)
        raise
    finally:
        if app is not None and app.poll() is None:
            try:
                os.killpg(app.pid, signal.SIGTERM)
                app.wait(timeout=5)
            except (ProcessLookupError, subprocess.TimeoutExpired):
                if app.poll() is None:
                    os.killpg(app.pid, signal.SIGKILL)
                    app.wait()
        xserver.terminate()
        xserver.wait(timeout=5)
        print("Smoke logs:", root)
