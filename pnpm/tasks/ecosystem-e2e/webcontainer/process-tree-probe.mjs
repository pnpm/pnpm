import assert from 'node:assert/strict'
import { createHostServices } from './runtime/host/operations.mjs'

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
  assert.throws(() => process.kill(grandchild, 0), { code: 'ESRCH' })
  console.log('Owned process tree cancellation passed')
} finally {
  await host.close()
}
