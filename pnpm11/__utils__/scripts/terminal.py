# Runs a command as the foreground job of a pseudo-terminal, presses Ctrl+C
# once the command prints "started", and echoes everything the command wrote.
#
# A leading --deadline=SECONDS bounds the wait for "started" (30 by default),
# and the command then has 30 seconds to exit after the Ctrl+C. When either
# passes, the command and everything it started are killed and the exit
# status is 124, so a test fails instead of hanging on a command that never
# became ready or never shut down.
import os
import pty
import select
import signal
import sys
import time

SHUTDOWN_SECONDS = 30

argv = sys.argv[1:]
deadline_seconds = 30
if argv and argv[0].startswith('--deadline='):
    deadline_seconds = float(argv.pop(0).removeprefix('--deadline='))

pid, fd = pty.fork()
if pid == 0:
    os.execvp(argv[0], argv)

output = b''
deadline = time.time() + deadline_seconds
interrupted = False
timed_out = False
while True:
    if time.time() >= deadline:
        timed_out = True
        break
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
        deadline = time.time() + SHUTDOWN_SECONDS
if timed_out:
    # The command leads the pseudo-terminal's session, so its process group
    # is everything it started.
    try:
        os.killpg(pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
_, status = os.waitpid(pid, 0)
sys.stdout.write(output.decode(errors='replace'))
if timed_out:
    sys.exit(124)
sys.exit(os.waitstatus_to_exitcode(status) if os.WIFEXITED(status) else 128 + os.WTERMSIG(status))
