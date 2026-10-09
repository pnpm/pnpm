// Run `cargo dylint` with rustup's proxies first on `PATH`.
//
// cargo-dylint runs `rustup` itself and builds its lint library with the
// nightly the library pins, which only rustup's `cargo` selects. pnpm runs
// scripts with the workspace toolchain first on `PATH`, ahead of rustup.
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'

const exe = process.platform === 'win32' ? '.exe' : ''

const pathKey = Object.keys(process.env).find((key) => key.toUpperCase() === 'PATH') ?? 'PATH'
const searchPath = process.env[pathKey] ?? ''
const proxies = searchPath
  .split(path.delimiter)
  .find((dir) => path.isAbsolute(dir) && ['rustup', 'cargo'].every((name) => fs.existsSync(path.join(dir, name + exe))))
if (proxies == null) {
  console.error('cargo-dylint needs rustup, and no directory on PATH holds both rustup and cargo.')
  process.exit(1)
}

const result = spawnSync(path.join(proxies, 'cargo' + exe), ['dylint', ...process.argv.slice(2)], {
  stdio: 'inherit',
  env: { ...process.env, [pathKey]: [proxies, searchPath].join(path.delimiter) },
})
if (result.error != null) throw result.error
process.exit(result.status ?? 1)
