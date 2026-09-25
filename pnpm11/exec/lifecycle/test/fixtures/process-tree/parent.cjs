const { spawn } = require('node:child_process')
const path = require('node:path')

// The grandchild is spawned detached: a child spawned without `detached`
// joins the libuv Job Object of its parent on Windows and dies together with
// it, which would make the tree-kill assertion pass vacuously.
const child = spawn(process.execPath, [path.join(__dirname, 'child.cjs')], { detached: true, stdio: 'ignore' })
child.unref()
console.log(child.pid)
setInterval(() => {}, 1000)
