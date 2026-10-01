import { spawnSync } from 'node:child_process'
import console from 'node:console'
import { mkdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import process from 'node:process'
import { fileURLToPath, URL } from 'node:url'

const root = fileURLToPath(new URL('../../../../../', import.meta.url))
const sdk = process.env.WASI_SDK_PATH
if (!sdk) throw new Error('Set WASI_SDK_PATH to an extracted official WASI SDK directory')
const lockfile = readFileSync(join(root, 'Cargo.lock'), 'utf8')
const entry = lockfile.split('\n[[package]]').find(entry => entry.includes('\nname = "libsqlite3-sys"\n'))
const version = entry?.match(/\nversion = "([^"]+)"\n/)?.[1]
if (!version) throw new Error('Cannot find libsqlite3-sys in Cargo.lock')
const sqlite = join(root, '.pnpm', 'crates', 'crates-io', `libsqlite3-sys-${version}`, 'sqlite3')
const output = join(root, 'target', 'wasm-sqlite-probe')
mkdirSync(output, { recursive: true })
const wasm = join(output, 'sqlite-probe.wasm')
const result = spawnSync(join(sdk, 'bin', 'clang'), [
  '--target=wasm32-wasip1-threads', '-pthread', '-Wall', '-Wextra',
  '-Wl,--max-memory=268435456', '-DSQLITE_THREADSAFE=1', '-DSQLITE_OMIT_LOAD_EXTENSION',
  '-D_WASI_EMULATED_MMAN', '-D_WASI_EMULATED_GETPID', '-D_WASI_EMULATED_SIGNAL',
  '-D_WASI_EMULATED_PROCESS_CLOCKS', '-DLONGDOUBLE_TYPE=double',
  '-I', sqlite, fileURLToPath(new URL('./main.c', import.meta.url)), join(sqlite, 'sqlite3.c'),
  '-lwasi-emulated-mman', '-lwasi-emulated-getpid', '-lwasi-emulated-signal',
  '-lwasi-emulated-process-clocks', '-o', wasm,
], { cwd: root, stdio: 'inherit' })
if (result.error) throw result.error
if (result.signal) throw new Error(`SQLite WASM build terminated by ${result.signal}`)
process.exitCode = result.status
if (result.status === 0) console.log(wasm)
