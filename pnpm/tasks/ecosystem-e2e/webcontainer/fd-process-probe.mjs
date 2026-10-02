import assert from 'node:assert/strict'
import fs from 'node:fs'

const { createHostServices } = await import(new URL('./runtime/host/operations.mjs', import.meta.url))
const directory = fs.mkdtempSync('/tmp/pnpm-process-descriptor-')
const path = `${directory}/output`
const descriptor = fs.openSync(path, 'w')
let released = false
const host = createHostServices({ acquireDescriptor (guest) {
  assert.equal(guest, 42)
  return { fd: descriptor, release () { released = true } }
} })
try {
  const child = await host.dispatch({
    operation: 'process.spawn', program: 'node', args: ['-e', "process.stdout.write('redirected'); process.exitCode = 0"],
    stdout: { fd: 42 }, stdin: 'ignore', stderr: 'inherit',
  })
  assert.equal((await host.dispatch({ operation: 'process.wait', handle: child.handle })).code, 0)
  assert.equal(fs.readFileSync(path, 'utf8'), 'redirected')
  assert.equal(released, true)
  fs.writeSync(descriptor, '-caller')
  assert.equal(fs.readFileSync(path, 'utf8'), 'redirected-caller')
  console.log('WebContainer pipe-based descriptor redirection passed')
} finally {
  await host.close()
  fs.closeSync(descriptor)
  fs.rmSync(directory, { recursive: true, force: true })
}
