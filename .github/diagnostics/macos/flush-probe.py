"""Time fcntl(F_FULLFSYNC) against fsync with 1 and 12 concurrent writers."""
import fcntl
import os
import statistics
import tempfile
import threading
import time

CALLS_PER_WRITER = 100


def writer(directory, index, full, latencies):
    path = os.path.join(directory, f"writer-{index}")
    fd = os.open(path, os.O_CREAT | os.O_WRONLY, 0o644)
    try:
        for _ in range(CALLS_PER_WRITER):
            os.write(fd, b"x")
            start = time.perf_counter()
            if full:
                fcntl.fcntl(fd, fcntl.F_FULLFSYNC)
            else:
                os.fsync(fd)
            latencies.append((time.perf_counter() - start) * 1000)
    finally:
        os.close(fd)


def run(writers, full):
    latencies = []
    with tempfile.TemporaryDirectory() as directory:
        threads = [
            threading.Thread(target=writer, args=(directory, i, full, latencies))
            for i in range(writers)
        ]
        start = time.perf_counter()
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join()
        wall = time.perf_counter() - start
    latencies.sort()
    p90 = latencies[int(len(latencies) * 0.9)]
    name = "F_FULLFSYNC" if full else "fsync"
    print(
        f"{name:<11} writers={writers:<2} calls={len(latencies):<5} "
        f"median={statistics.median(latencies):7.2f} ms  p90={p90:7.2f} ms  wall={wall:6.2f} s"
    )


for writers in (1, 12):
    for full in (False, True):
        run(writers, full)
