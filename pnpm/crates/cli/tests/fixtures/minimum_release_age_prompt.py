import errno
import os
import pty
import select
import subprocess
import sys
import time


master, slave = pty.openpty()
process = subprocess.Popen(sys.argv[1:], stdin=slave, stdout=slave, stderr=slave)
os.close(slave)
output = bytearray()
approved = False
deadline = time.monotonic() + 60

try:
    while time.monotonic() < deadline:
        if not select.select([master], [], [], 0.1)[0]:
            if process.poll() is not None:
                break
            continue
        try:
            chunk = os.read(master, 65536)
        except OSError as error:
            if error.errno == errno.EIO:
                break
            raise
        if not chunk:
            break
        output.extend(chunk)
        if not approved and b"proceed with the install?" in output:
            os.write(master, b"y")
            approved = True
    else:
        raise TimeoutError("interactive install did not finish within 60 seconds")
    process.wait(timeout=5)
finally:
    if process.poll() is None:
        process.kill()
    process.wait()
    os.close(master)
    sys.stdout.buffer.write(output)

sys.exit(process.returncode)
