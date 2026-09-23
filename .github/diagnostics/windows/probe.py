"""Measures Windows costs that the Rust tests pay more than Linux does.

    probe.py system   drive, NTFS and service settings
    probe.py net      time to a refused loopback connection
    probe.py flush    FlushFileBuffers (os.fsync) latency
    probe.py fs       small-file, link, rename and delete costs
    probe.py spawn    process start costs
"""
import os
import shutil
import socket
import statistics
import subprocess
import sys
import tempfile
import threading
import time


def run(cmd):
    out = subprocess.run(cmd, capture_output=True, text=True, shell=isinstance(cmd, str))
    return (out.stdout + out.stderr).strip()


def ms(seconds):
    return f"{seconds * 1000:8.1f} ms"


def system():
    work = os.getcwd()[:2]
    temp = tempfile.gettempdir()[:2]
    print(f"workspace drive {work}, TEMP drive {temp}")
    for drive in sorted({work, temp}):
        print(f"8dot3 on {drive}: {run(f'fsutil 8dot3name query {drive}')}")
    print("last access:", run("fsutil behavior query DisableLastAccess"))
    print(run(["powershell", "-NoProfile", "-Command",
               "Get-PhysicalDisk | Format-Table FriendlyName,MediaType,BusType,Size -AutoSize | Out-String -Width 200;"
               "Get-Service WSearch,SysMain -ErrorAction SilentlyContinue | Format-Table Name,Status | Out-String"]))


def refused(host, port, family=socket.AF_INET):
    start = time.perf_counter()
    try:
        with socket.socket(family, socket.SOCK_STREAM) as sock:
            sock.settimeout(20)
            sock.connect((host, port))
        outcome = "connected"
    except OSError as err:
        outcome = type(err).__name__
    return time.perf_counter() - start, outcome


def closed_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def net():
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen()
    cases = [
        ("127.0.0.1:1", "127.0.0.1", 1, socket.AF_INET),
        ("127.0.0.1:9", "127.0.0.1", 9, socket.AF_INET),
        ("127.0.0.1:<just closed>", "127.0.0.1", closed_port(), socket.AF_INET),
        ("[::1]:1", "::1", 1, socket.AF_INET6),
        ("127.0.0.1:<listening>", "127.0.0.1", listener.getsockname()[1], socket.AF_INET),
    ]
    for label, host, port, family in cases:
        times = [refused(host, port, family) for _ in range(3)]
        print(f"{label:26} {times[0][1]:24} " + " ".join(ms(t) for t, _ in times))
    print("localhost resolves to:", [a[4][0] for a in socket.getaddrinfo("localhost", 1, type=socket.SOCK_STREAM)])
    listener.close()
    localhost_to_ipv4_only_server()


def localhost_to_ipv4_only_server():
    """The TS test registry listens on 127.0.0.1 but is addressed as localhost."""
    import http.server

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), http.server.SimpleHTTPRequestHandler)
    port = server.server_address[1]
    threading.Thread(target=server.serve_forever, daemon=True).start()
    for label in ("localhost", "127.0.0.1"):
        times = []
        for _ in range(3):
            start = time.perf_counter()
            socket.create_connection((label, port), timeout=20).close()
            times.append(time.perf_counter() - start)
        print(f"python connect {label}:<ipv4 server>   " + " ".join(ms(t) for t in times))
    script = (
        "const u=process.argv[1];(async()=>{for(let i=0;i<3;i++){const t=performance.now();"
        "await fetch(u,{method:'HEAD'}).catch(e=>console.log(e.cause?.code));"
        "console.log((performance.now()-t).toFixed(1)+' ms')}})()"
    )
    for label in ("localhost", "127.0.0.1"):
        # A fresh process per request, like each pnpm the tests spawn.
        times = [run(["node", "-e", script, f"http://{label}:{port}/"]).splitlines()[0] for _ in range(3)]
        print(f"node fetch {label}:<ipv4 server>, new process each: {', '.join(times)}")
    server.shutdown()


def flush():
    for directory in (tempfile.gettempdir(), os.getcwd()):
        for writers in (1, 8):
            latencies = []

            def writer(index):
                path = os.path.join(directory, f"flush-probe-{index}")
                fd = os.open(path, os.O_CREAT | os.O_WRONLY | os.O_BINARY)
                try:
                    for _ in range(100):
                        os.write(fd, b"x")
                        start = time.perf_counter()
                        os.fsync(fd)
                        latencies.append(time.perf_counter() - start)
                finally:
                    os.close(fd)
                    os.remove(path)

            threads = [threading.Thread(target=writer, args=(i,)) for i in range(writers)]
            for thread in threads:
                thread.start()
            for thread in threads:
                thread.join()
            print(f"fsync {directory[:2]} writers={writers}: median {ms(statistics.median(latencies))}")


def timed(label, count, action):
    start = time.perf_counter()
    done = 0
    try:
        for i in range(count):
            action(i)
            done += 1
    except OSError as err:
        print(f"{label:34} failed after {done}: {err}")
        return
    print(f"{label:34} {ms((time.perf_counter() - start) / count)} per op")


def fs():
    import _winapi

    for base in (tempfile.gettempdir(), os.getcwd()):
        root = tempfile.mkdtemp(dir=base, prefix="fs-probe-")
        print(f"-- {base[:2]}")
        files = os.path.join(root, "files")
        os.mkdir(files)
        timed("create 2000 small files", 2000, lambda i: open(os.path.join(files, f"f{i}.json"), "wb").write(b"{}"))
        timed("hard link", 300, lambda i: os.link(os.path.join(files, f"f{i}.json"), os.path.join(root, f"h{i}")))
        targets = os.path.join(root, "targets")
        os.mkdir(targets)
        for i in range(300):
            os.mkdir(os.path.join(targets, f"d{i}"))
        timed("junction", 300, lambda i: _winapi.CreateJunction(os.path.join(targets, f"d{i}"), os.path.join(root, f"j{i}")))
        timed("directory symlink", 300, lambda i: os.symlink(os.path.join(targets, f"d{i}"), os.path.join(root, f"s{i}"), target_is_directory=True))
        timed("rename over existing", 500, lambda i: (open(os.path.join(root, "tmp"), "wb").write(b"x"), os.replace(os.path.join(root, "tmp"), os.path.join(root, "dest"))))
        start = time.perf_counter()
        shutil.rmtree(root)
        print(f"{'remove tree (~3000 entries)':34} {ms(time.perf_counter() - start)}")


def spawn():
    pnpm = os.path.join("target", "debug", "pnpm.exe")
    commands = [("cmd /c exit 0", ["cmd", "/c", "exit", "0"]), ("git --version", ["git", "--version"]),
                ("node -e 0", ["node", "-e", "0"]), ("python -c pass", [sys.executable, "-c", "pass"])]
    if os.path.exists(pnpm):
        commands.append(("pnpm.exe --version (debug)", [pnpm, "--version"]))
    for label, cmd in commands:
        times = []
        for _ in range(20):
            start = time.perf_counter()
            subprocess.run(cmd, capture_output=True)
            times.append(time.perf_counter() - start)
        print(f"{label:30} median {ms(statistics.median(times))}  min {ms(min(times))}")


if __name__ == "__main__":
    {"system": system, "net": net, "flush": flush, "fs": fs, "spawn": spawn}[sys.argv[1]]()
