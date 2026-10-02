import { spawnSync } from 'node:child_process'
import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

import { instrumentAtomicWaits } from './instrument-atomics.mjs'
import { validateWabt } from './encode-wat.mjs'

const arguments_ = process.argv.slice(2)
if (arguments_.includes('--check') && arguments_.includes('--clippy')) throw new Error('Choose either --check or --clippy')
const command = arguments_.includes('--check') ? 'check' : arguments_.includes('--clippy') ? 'clippy' : 'build'
const cargoArguments = arguments_.filter(argument => argument !== '--check' && argument !== '--clippy')
if (command === 'build') validateWabt()

const root = fileURLToPath(new URL('../../', import.meta.url))
const sdk = process.env.WASI_SDK_PATH
if (!sdk) throw new Error('Set WASI_SDK_PATH to an extracted official WASI SDK 34 directory')
if (readFileSync(join(sdk, 'VERSION'), 'utf8').split('\n')[0].trim() !== '34.0') {
  throw new Error('The pnpm WebContainer build requires WASI SDK 34.0')
}
const flags = [
  '--cfg', 'tokio_unstable',
  '-C', 'link-arg=--export=malloc',
  '-C', 'link-arg=--export=free',
  '-C', 'link-arg=--export=__tls_base',
  '-C', 'link-arg=--export=__wasm_init_tls',
  '-L', join(sdk, 'share/wasi-sysroot/lib/wasm32-wasip1-threads'),
  ...['mman', 'getpid', 'signal', 'process-clocks'].flatMap(name => ['-l', `static=wasi-emulated-${name}`]),
]
const inherited = process.env.CARGO_ENCODED_RUSTFLAGS?.split('\x1f') ?? process.env.RUSTFLAGS?.split(/\s+/).filter(Boolean) ?? []
const result = spawnSync('cargo', [
  '+nightly-2026-08-27', command, '--locked', '--release', '--package', 'pnpm-cli', '--bin', 'pnpm', '--target', 'wasm32-wasip1-threads', ...cargoArguments,
], {
  cwd: root,
  stdio: 'inherit',
  env: {
    ...process.env,
    CC_wasm32_wasip1_threads: join(sdk, 'bin/clang'),
    AR_wasm32_wasip1_threads: join(sdk, 'bin/llvm-ar'),
    CFLAGS_wasm32_wasip1_threads: '--target=wasm32-wasip1-threads -pthread',
    LIBSQLITE3_FLAGS: '-USQLITE_THREADSAFE -DSQLITE_THREADSAFE=1',
    CARGO_ENCODED_RUSTFLAGS: [...inherited, ...flags].join('\x1f'),
  },
})
if (result.error) throw result.error
if (result.signal) throw new Error(`WASM CLI build terminated by ${result.signal}`)
process.exitCode = result.status

if (result.status === 0 && command === 'build') {
  const artifact = join(root, 'target', 'wasm32-wasip1-threads', 'release', 'pnpm.wasm')
  writeFileSync(artifact, await instrumentAtomicWaits(readFileSync(artifact)))
}
