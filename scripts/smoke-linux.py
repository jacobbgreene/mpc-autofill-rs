"""Launch a packaged app on a private X display and verify its window renders."""
import os
from pathlib import Path
import signal
import struct
import subprocess
import sys
import tempfile
import time

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
        print("PASS: Linux package stayed running and rendered its main window.")
    except Exception:
        app_log.flush()
        print((root / "app.log").read_text(), file=sys.stderr)
        raise
    finally:
        if app is not None:
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
