import { spawn } from 'node:child_process'
import { once } from 'node:events'
import { setTimeout } from 'node:timers/promises'

const child = spawn('node', ['-e', `
const grandchild = require('node:child_process').spawn('node', ['-e', 'setInterval(() => {}, 1000)']);
console.log(JSON.stringify({parent:process.pid,child:grandchild.pid}));
setInterval(() => {}, 1000);
`], { detached: true, stdio: ['ignore', 'pipe', 'pipe'] })
let descendant
const exited = once(child, 'exit')
try {
  const [chunk] = await once(child.stdout, 'data')
  const pids = JSON.parse(chunk.toString())
  descendant = pids.child
  console.log('detached process tree:', pids)
  try {
    process.kill(-child.pid, 'SIGTERM')
    const outcome = await Promise.race([exited, setTimeout(2000, 'timeout')])
    console.log('process-group SIGTERM result:', outcome)
    let descendantAlive = true
    try { process.kill(descendant, 0) } catch (error) {
      if (error.code !== 'ESRCH') throw error
      descendantAlive = false
    }
    console.log('descendant alive after group signal:', descendantAlive)
  } catch (error) {
    console.log('process-group signal unsupported:', error.code, error.message)
    process.kill(child.pid, 'SIGTERM')
    console.log('direct-parent signal result:', await exited)
    let descendantAlive = true
    try { process.kill(descendant, 0) } catch (error) {
      if (error.code !== 'ESRCH') throw error
      descendantAlive = false
    }
    console.log('descendant alive after parent signal:', descendantAlive)
  }
} finally {
  for (const pid of [descendant, child.pid].filter(Number.isInteger)) {
    try { process.kill(pid, 'SIGKILL') } catch (error) {
      if (error.code !== 'ESRCH') throw error
    }
  }
  await exited
}
console.log('process-group capability probe completed')
