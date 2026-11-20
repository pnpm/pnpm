import assert from 'node:assert/strict'
const test = async (name, run) => { const cleanup = []; try { await run({ after: callback => cleanup.push(callback) }); console.log(name) } finally { for (const callback of cleanup) await callback() } }

import { createHostServices } from './runtime/host/operations.mjs'

await test('shell emulator preserves environment, quoting, pipes, args and exit status', async t => {
  const host = createHostServices()
  t.after(() => host.close())
  const child = await host.dispatch({
    operation: 'shell.spawn',
    script: 'echo "$PNPM_WASM_SHELL_VALUE:$0" | node -e "process.stdin.pipe(process.stdout)"; exit 17',
    args: ['literal a b'], env: { ...process.env, PNPM_WASM_SHELL_VALUE: 'value a b' },
    cwd: process.cwd(), stdin: 'ignore',
  })
  const [stdout, stderr, status] = await Promise.all([
    readAll(host, child.stdout), readAll(host, child.stderr),
    host.dispatch({ operation: 'process.wait', handle: child.handle }),
  ])
  console.log({stdout,stderr,status})
  assert.equal(stdout, 'value a b:literal a b\n')
  assert.equal(stderr, '')
  assert.equal(status.code, 17)
})

await test('shell emulator reports syntax errors before spawning a process', async t => {
  const host = createHostServices()
  t.after(() => host.close())
  await assert.rejects(host.dispatch({ operation: 'shell.spawn', script: 'echo "unterminated' }), { code: 'ESHELLPARSE' })
})

async function readAll (host, handle) {
  const chunks = []
  while (true) {
    // eslint-disable-next-line no-await-in-loop -- Each read advances the same output stream.
    const { bytes, done } = await host.dispatch({ operation: 'stream.read', handle })
    if (done) return Buffer.concat(chunks).toString()
    chunks.push(bytes)
  }
}
