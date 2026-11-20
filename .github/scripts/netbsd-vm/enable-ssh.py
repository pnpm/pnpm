"""Log in on a fresh anita-installed NetBSD VM's serial console and start sshd.

Usage: enable-ssh.py <serial-port> <authorized-key-file>
"""

import socket
import sys
import time

import pexpect.fdpexpect

port = int(sys.argv[1])
with open(sys.argv[2], encoding='utf-8') as key_file:
    authorized_key = key_file.read().strip()

deadline = time.monotonic() + 120
while True:
    try:
        console = socket.create_connection(('127.0.0.1', port))
        break
    except OSError:
        if time.monotonic() > deadline:
            raise
        time.sleep(1)

session = pexpect.fdpexpect.fdspawn(console.fileno(), timeout=600, encoding='utf-8', logfile=sys.stdout)
session.send('\r')
session.expect('login:')
session.sendline('root')
session.expect('# ')
for command in [
    'echo sshd=YES >> /etc/rc.conf && echo dhcpcd=YES >> /etc/rc.conf',
    'echo "PermitRootLogin prohibit-password" >> /etc/ssh/sshd_config',
    f'mkdir -p /root/.ssh && echo "{authorized_key}" > /root/.ssh/authorized_keys && chmod -R go= /root/.ssh',
    # The quotes keep the console's echo of this line from matching the marker.
    '/etc/rc.d/dhcpcd start && /etc/rc.d/sshd start && echo "SSH-""READY"',
]:
    session.sendline(command)
session.expect('SSH-READY')
