import assert from 'node:assert/strict'
import { execFile } from 'node:child_process'
import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { promisify } from 'node:util'

const artifact = fileURLToPath(new URL('../../target/wasm32-wasip1-threads/release/pnpm.wasm', import.meta.url))
const runner = new URL('./run.mjs', import.meta.url).href

test('repeated guest runs retain the event loop through shutdown and then release it', { timeout: 60000 }, async () => {
  const directory = await mkdtemp(path.join(tmpdir(), 'pnpm-wasm-liveness-'))
  try {
    await writeFile(path.join(directory, 'package.json'), JSON.stringify({
      private: true, scripts: { smoke: 'node -e "process.stdout.write(\'child-completed\')"' },
    }))
    const script = `
      import { runWasm } from ${JSON.stringify(runner)}
      for (let index = 0; index < 40; index++) {
        const code = await runWasm(${JSON.stringify(artifact)}, { args: ['pnpm', 'run', 'smoke'] })
        if (code !== 0) throw new Error('Guest failed: ' + code)
      }
      console.log('all-runs-completed')
    `
    const { stdout } = await promisify(execFile)(process.execPath, ['--input-type=module', '-e', script], { cwd: directory, timeout: 45000 })
    assert.match(stdout, /all-runs-completed/)
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})
