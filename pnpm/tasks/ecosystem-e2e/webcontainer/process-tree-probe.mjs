import assert from 'node:assert/strict'
import { setTimeout } from 'node:timers/promises'

const { createHostServices } = await import(process.env.PNPM_WASM_HOST_MODULE ?? './runtime/host/operations.mjs')

const host = createHostServices()
try {
  const child = await host.dispatch({
    operation: 'process.spawn', program: 'node', stdout: 'pipe', stderr: 'inherit',
    args: ['-e', `
const child = require('node:child_process').spawn('node', ['-e', 'setInterval(() => {}, 1000)'], {stdio:'inherit'});
console.log(child.pid);
setInterval(() => {}, 1000);
`],
  })
  const output = await host.dispatch({ operation: 'stream.read', handle: child.stdout })
  const grandchild = Number(Buffer.from(output.bytes).toString().trim())
  assert.ok(Number.isInteger(grandchild) && grandchild > 0)
  await host.dispatch({ operation: 'process.kill', handle: child.handle, signal: 'SIGKILL' })
  const status = await host.dispatch({ operation: 'process.wait', handle: child.handle })
  assert.equal(status.signal, 'SIGKILL')
  // WebContainers deliver SIGKILL asynchronously: the grandchild can outlive the reaped child by a few milliseconds.
  assert.ok(await exitsWithin(grandchild, 5000), `grandchild ${grandchild} survived the owned process tree cancellation`)
  console.log('Owned process tree cancellation passed')
} finally {
  await host.close()
}

async function exitsWithin (pid, timeoutMs) {
  const deadline = Date.now() + timeoutMs
  do {
    try {
      process.kill(pid, 0)
    } catch (error) {
      if (error.code === 'ESRCH') return true
      throw error
    }
    await setTimeout(10)
  } while (Date.now() < deadline)
  return false
}
