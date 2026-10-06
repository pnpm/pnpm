import fs from 'node:fs'
import process from 'node:process'
import { fileURLToPath, URL } from 'node:url'

import esbuild from 'esbuild'

const result = await esbuild.build({
  write: false,
  entryPoints: [fileURLToPath(new URL('../install-register.mjs', import.meta.url))],
  bundle: true,
  minify: true,
  platform: 'node',
  format: 'esm',
  outfile: fileURLToPath(new URL('../../crates/deps-restorer/src/cas-loader.mjs.inc', import.meta.url)),
  banner: { js: "import { createRequire as bootstrapRequire } from 'node:module'; const require = bootstrapRequire(import.meta.url);" },
})

const output = result.outputFiles[0]
const tempPath = `${output.path}.${process.pid}.tmp`
fs.writeFileSync(tempPath, output.contents)
fs.renameSync(tempPath, output.path)
