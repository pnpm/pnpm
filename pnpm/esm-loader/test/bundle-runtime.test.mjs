import { execFileSync } from 'node:child_process'
import process from 'node:process'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'

test('the committed loader matches the runtime sources', () => {
  execFileSync(process.execPath, [fileURLToPath(new URL('../scripts/bundle-runtime.mjs', import.meta.url)), '--check'], { stdio: 'inherit' })
})
