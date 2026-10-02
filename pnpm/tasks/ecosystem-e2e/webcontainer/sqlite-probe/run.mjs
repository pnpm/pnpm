import { readFileSync } from 'node:fs'
import process from 'node:process'
import { WASI } from 'node:wasi'
import { fileURLToPath, URL } from 'node:url'

const { WebAssembly } = globalThis

const wasm = process.env.PNPM_SQLITE_PROBE_WASM ?? fileURLToPath(new URL('../../../../../target/wasm-sqlite-probe/sqlite-probe.wasm', import.meta.url))
const wasi = new WASI({
  version: 'preview1',
  args: ['sqlite-probe', ...process.argv.slice(2)],
  preopens: { '/': '/' },
  returnOnExit: true,
})
const instance = await WebAssembly.instantiate(await WebAssembly.compile(readFileSync(wasm)), {
  ...wasi.getImportObject(),
  wasi: {
    'thread-spawn' () { throw new Error('The SQLite storage probe must not spawn threads') },
  },
})
process.exitCode = wasi.start(instance)
