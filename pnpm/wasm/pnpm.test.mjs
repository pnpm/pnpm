import assert from 'node:assert/strict'
import { execFile } from 'node:child_process'
import { copyFile, mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { test } from 'node:test'
import { promisify } from 'node:util'

const executeFile = promisify(execFile)

test('installed launchers use their sibling artifact despite inherited parent runtime variables', async t => {
  const directory = await mkdtemp(path.join(tmpdir(), 'pnpm-wasm-launcher-'))
  t.after(() => rm(directory, { recursive: true, force: true }))
  await writeFile(path.join(directory, 'run.mjs'), `
export async function runWasm(artifact, options) {
  console.log(JSON.stringify({artifact, ...options}));
  return 0;
}
`)
  await Promise.all(['pnpm.mjs', 'pnpx.mjs'].map(async alias => {
    const executable = path.join(directory, alias)
    await copyFile(new URL('./pnpm.mjs', import.meta.url), executable)
    const { stdout } = await executeFile(process.execPath, [executable, '--version'], {
      env: { ...process.env, PNPM_WASM_ARTIFACT: '/different-pnpm/pnpm.wasm' },
    })
    assert.deepEqual(JSON.parse(stdout), {
      artifact: path.join(directory, 'pnpm.wasm'), executable, args: [executable, '--version'],
    })
  }))
})
