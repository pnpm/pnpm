# Runs a command as the foreground job of a pseudo-terminal, presses Ctrl+C
# once the command prints "started", and echoes everything the command wrote.
import os
import pty
import select
import sys
import time

pid, fd = pty.fork()
if pid == 0:
    os.execvp(sys.argv[1], sys.argv[1:])

output = b''
deadline = time.time() + 30
interrupted = False
while time.time() < deadline:
    ready, _, _ = select.select([fd], [], [], 0.1)
    if ready:
        try:
            data = os.read(fd, 4096)
        except OSError:
            break
        if not data:
            break
        output += data
    if not interrupted and b'started' in output:
        os.write(fd, b'\x03')
        interrupted = True
_, status = os.waitpid(pid, 0)
sys.stdout.write(output.decode(errors='replace'))
sys.exit(os.waitstatus_to_exitcode(status) if os.WIFEXITED(status) else 128 + os.WTERMSIG(status))
