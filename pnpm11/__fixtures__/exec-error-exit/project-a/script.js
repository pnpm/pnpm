import { spawn } from 'child_process'
// The server is spawned detached: a child spawned without `detached` joins
// the libuv Job Object of its parent on Windows and is killed together with
// it, so only a detached grandchild can actually be orphaned.
const child = spawn(process.execPath, ['./server.js'], { detached: true, stdio: 'ignore' })
child.unref()
setInterval(() => {}, 1000)
