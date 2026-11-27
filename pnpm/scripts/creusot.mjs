// Run `cargo creusot` on every workspace member that takes `creusot-std`
// under `[target.'cfg(creusot)'.dependencies]`, one member at a time.
// Arguments go to each run, so `--no-cache` and `--replay` work as they do
// on `cargo creusot` itself.
//
// A run fails unless it proved every Coma file it generated for the member.
// `cargo creusot` exits 0 with a warning when why3find cannot read the files,
// as with a Why3 whose plugins came from another build, so its exit status
// alone would pass a run that proved nothing.
//
// `cargo creusot` builds with the nightly its release pins, which only
// rustup's `cargo` selects, so rustup's proxies go first on `PATH` as in
// `cargo-dylint.mjs`. See `pnpm/VERIFICATION.md`.
import { execFileSync, spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath, pathToFileURL } from 'node:url'

const exe = process.platform === 'win32' ? '.exe' : ''

// Arguments with which a run proves nothing by design.
const NO_PROOF_ARGS = ['--only', '--dry-run-why3find', '--ide-always', '--help', '-h']

export function verifiedPackages (metadata) {
  return metadata.packages
    .filter((pkg) => pkg.dependencies.some((dependency) => dependency.name === 'creusot-std' && dependency.target === 'cfg(creusot)'))
    .map((pkg) => pkg.name)
    .sort()
}

export function provedFileCount (output) {
  const match = /^Proved \((\d+) files?\)/m.exec(output)
  return match == null ? 0 : Number(match[1])
}

export function expectsProofs (args) {
  return !args.some((arg) => NO_PROOF_ARGS.some((flag) => arg === flag || arg.startsWith(`${flag}=`)))
}

function comaFileCount (directory) {
  if (!fs.existsSync(directory)) return 0
  return fs.readdirSync(directory, { recursive: true }).filter((entry) => String(entry).endsWith('.coma')).length
}

function main () {
  const pathKey = Object.keys(process.env).find((key) => key.toUpperCase() === 'PATH') ?? 'PATH'
  const searchPath = process.env[pathKey] ?? ''
  const proxies = searchPath
    .split(path.delimiter)
    .find((dir) => path.isAbsolute(dir) && ['rustup', 'cargo'].every((name) => fs.existsSync(path.join(dir, name + exe))))
  if (proxies == null) {
    throw new Error('cargo creusot needs rustup, and no directory on PATH holds both rustup and cargo')
  }
  const env = { ...process.env, [pathKey]: [proxies, searchPath].join(path.delimiter) }
  const cargo = path.join(proxies, 'cargo' + exe)
  const metadata = JSON.parse(execFileSync(cargo, ['metadata', '--format-version', '1', '--no-deps'], {
    encoding: 'utf8',
    env,
    maxBuffer: 256 * 1024 * 1024,
  }))
  const packages = verifiedPackages(metadata)
  if (packages.length === 0) throw new Error('no workspace member takes creusot-std under cfg(creusot)')
  const args = process.argv.slice(2)
  for (const name of packages) {
    process.stderr.write(`cargo creusot -p ${name}\n`)
    const result = spawnSync(cargo, ['creusot', '-p', name, ...args], {
      stdio: ['inherit', 'pipe', 'inherit'],
      encoding: 'utf8',
      env,
      maxBuffer: 256 * 1024 * 1024,
    })
    if (result.error != null) throw result.error
    process.stdout.write(result.stdout)
    if (result.status !== 0) {
      process.exitCode = result.status ?? 1
      return
    }
    if (!expectsProofs(args)) continue
    const generated = comaFileCount(path.join(metadata.workspace_root, 'verif', `${name.replaceAll('-', '_')}_rlib`))
    const proved = provedFileCount(result.stdout)
    if (generated === 0 || proved !== generated) {
      throw new Error(`cargo creusot proved ${proved} of the ${generated} Coma files it generated for ${name}`)
    }
  }
}

if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) {
  try {
    main()
  } catch (error) {
    process.stderr.write(`${path.basename(fileURLToPath(import.meta.url))}: ${error.message}\n`)
    process.exitCode = 1
  }
}
