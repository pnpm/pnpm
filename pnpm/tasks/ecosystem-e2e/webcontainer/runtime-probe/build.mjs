import { spawnSync } from 'node:child_process'
import console from 'node:console'
import process from 'node:process'
import { copyFileSync, cpSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath, URL } from 'node:url'

import { instrumentAtomicWaits } from '../../../../wasm/instrument-atomics.mjs'
import { validateWabt } from '../../../../wasm/encode-wat.mjs'

validateWabt()

const directory = dirname(fileURLToPath(import.meta.url))
const root = fileURLToPath(new URL('../../../../../', import.meta.url))
const generated = join(root, 'target', 'wasm-runtime-probe')
const sdk = process.env.WASI_SDK_PATH
if (!sdk) throw new Error('Set WASI_SDK_PATH to WASI SDK 34.0 for the SQLite runtime probe')
const lockfile = readFileSync(join(root, 'Cargo.lock'), 'utf8')
const patches = readFileSync(join(root, 'Cargo.toml'), 'utf8').split('[patch.crates-io]')[1].split('\n[')[0]
const manifest = `[package]
name = "pnpm-wasm-runtime-probe"
version = "0.0.0"
edition = "2024"

[workspace]

[patch.crates-io]
${patches}

[dependencies]
tokio = { version = "=${lockedVersion('tokio')}", features = ["rt", "rt-multi-thread", "macros", "fs", "io-util", "sync", "time"] }
rayon = "=${lockedVersion('rayon')}"
rusqlite = "=${lockedVersion('rusqlite')}"
miette = "7.6.0"
pnpm-wasm-host = { path = ${JSON.stringify(join(root, 'pnpm/crates/wasm-host'))} }
pnpm-http = { path = ${JSON.stringify(join(root, 'pnpm/crates/http'))} }
pnpm-network = { path = ${JSON.stringify(join(root, 'pnpm/crates/network'))} }
pnpm-fs = { path = ${JSON.stringify(join(root, 'pnpm/crates/fs'))} }
pnpm-store-dir = { path = ${JSON.stringify(join(root, 'pnpm/crates/store-dir'))} }
pnpm-package-manifest = { path = ${JSON.stringify(join(root, 'pnpm/crates/package-manifest'))} }
pnpm-which = { path = ${JSON.stringify(join(root, 'pnpm/crates/which'))} }
pnpm-process = { path = ${JSON.stringify(join(root, 'pnpm/crates/process'))} }
serde_json = "=${lockedVersion('serde_json')}"
`
mkdirSync(join(generated, 'src'), { recursive: true })
cpSync(join(root, 'vendor'), join(generated, 'vendor'), { recursive: true })
writeFileSync(join(generated, 'Cargo.toml'), manifest)
copyFileSync(join(root, 'Cargo.lock'), join(generated, 'Cargo.lock'))
copyFileSync(join(directory, 'main.rs'), join(generated, 'src', 'main.rs'))
copyFileSync(join(directory, 'http_upload.rs'), join(generated, 'src', 'http_upload.rs'))
copyFileSync(join(directory, 'process.rs'), join(generated, 'src', 'process.rs'))
copyFileSync(join(directory, 'store.rs'), join(generated, 'src', 'store.rs'))
copyFileSync(join(directory, 'rollback.rs'), join(generated, 'src', 'rollback.rs'))
copyFileSync(join(directory, 'which.rs'), join(generated, 'src', 'which.rs'))
const flags = [
  '--cfg', 'tokio_unstable',
  '-L', join(sdk, 'share/wasi-sysroot/lib/wasm32-wasip1-threads'),
  ...['mman', 'getpid', 'signal', 'process-clocks'].flatMap(name => ['-l', `static=wasi-emulated-${name}`]),
  '-C', 'link-arg=--export=malloc',
  '-C', 'link-arg=--export=free',
  '-C', 'link-arg=--export=__tls_base',
  '-C', 'link-arg=--export=__wasm_init_tls',
]
const inherited = process.env.CARGO_ENCODED_RUSTFLAGS?.split('\x1f') ?? process.env.RUSTFLAGS?.split(/\s+/).filter(Boolean) ?? []
const result = spawnSync('cargo', [
  '+nightly-2026-08-27', 'build', '--offline', '--manifest-path', join(generated, 'Cargo.toml'),
  '--target', 'wasm32-wasip1-threads', '--target-dir', generated,
], {
  cwd: root,
  stdio: 'inherit',
  env: { ...process.env, PNPM_REPO_ROOT: root,
    CC_wasm32_wasip1_threads: join(sdk, 'bin/clang'),
    AR_wasm32_wasip1_threads: join(sdk, 'bin/llvm-ar'),
    CFLAGS_wasm32_wasip1_threads: '--target=wasm32-wasip1-threads -pthread',
    LIBSQLITE3_FLAGS: '-USQLITE_THREADSAFE -DSQLITE_THREADSAFE=1',
    CARGO_ENCODED_RUSTFLAGS: [...inherited, ...flags].join('\x1f') },
})
if (result.error) throw result.error
if (result.signal) throw new Error(`WASM runtime probe build terminated by ${result.signal}`)
process.exitCode = result.status
if (result.status === 0) {
  const artifact = join(generated, 'wasm32-wasip1-threads', 'debug', 'pnpm-wasm-runtime-probe.wasm')
  writeFileSync(artifact, await instrumentAtomicWaits(readFileSync(artifact)))
  console.log(artifact)
}

function lockedVersion (name) {
  const entry = lockfile.split('\n[[package]]').find(entry => entry.includes(`\nname = "${name}"\n`))
  const version = entry?.match(/\nversion = "([^"]+)"\n/)?.[1]
  if (!version) throw new Error(`Cannot find ${name} in the workspace Cargo.lock`)
  return version
}
