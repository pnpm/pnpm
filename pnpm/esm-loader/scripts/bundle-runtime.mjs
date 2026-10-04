import assert from 'node:assert/strict'
import { Buffer } from 'node:buffer'
import fs from 'node:fs'
import { createRequire } from 'node:module'
import process from 'node:process'
import { fileURLToPath, URL } from 'node:url'

const require = createRequire(new URL('../../../pnpm11/pnpm/package.json', import.meta.url))
const result = await require('esbuild').build({
  write: false,
  entryPoints: [fileURLToPath(new URL('../install-register.mjs', import.meta.url))],
  bundle: true,
  minify: true,
  platform: 'node',
  format: 'esm',
  outfile: fileURLToPath(new URL('../../../pnpm/crates/deps-restorer/src/cas-loader.mjs.inc', import.meta.url)),
  banner: { js: "import { createRequire as bootstrapRequire } from 'node:module'; const require = bootstrapRequire(import.meta.url);" },
})

const output = result.outputFiles[0]
if (process.argv.includes('--check')) {
  assert.deepEqual(fs.readFileSync(output.path), Buffer.from(output.contents), 'Regenerate the embedded CAS runtime with scripts/bundle-runtime.mjs')
} else {
  fs.writeFileSync(output.path, output.contents)
}
