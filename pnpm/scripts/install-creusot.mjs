// Install Creusot from a release of creusot-prebuilt, the nightly it was
// built against, and the SMT solvers it proves with. See `pnpm/VERIFICATION.md`.
//
// Every file this downloads is pinned in this script by its SHA-256, and a
// file that hashes to anything else is refused before it is unpacked. The
// release's table comes from its `release.json`, and the nightly's from Rust's
// channel manifest for that date. Both are read when the pins move, never when
// this runs. `creusot-install`, one of the pinned archives, pins the solvers.
//
// `--downloads <dir>` keeps each verified download in `dir`, and reuses a
// kept file only while it still matches its pin.
//
// The release's Why3 has its prefix compiled in, so `/opt/creusot-why3` has to
// exist and be writable first. `cargo creusot` is on PATH once the data
// directory's `bin` is.
import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { parseArgs } from 'node:util'

// Where an archive unpacks.
const DATA_HOME = 'data home'
const WHY3_PREFIX = 'why3 prefix'

const RELEASE = {
  url: 'https://github.com/KSXGitHub/creusot-prebuilt/releases/download/0.15.0-0',
  why3Prefix: '/opt/creusot-why3/_opam',
  archives: [
    {
      file: 'cargo-creusot-0.15.0-0-x86_64-unknown-linux-gnu.tar.gz',
      unpacksAt: DATA_HOME,
      sha256: '5d6fc7444d3ba971d82d6e00a7ae7284a4e115fd3cd26de27b96dc4d95e1e8fe',
    },
    {
      file: 'creusot-rustc-0.15.0-0-x86_64-unknown-linux-gnu.tar.gz',
      unpacksAt: DATA_HOME,
      sha256: 'f522e563ff7cf9eb04246cfd89d0797f964784fe35788699d5b7bc6eac71b98a',
    },
    {
      file: 'creusot-install-0.15.0-0-x86_64-unknown-linux-gnu.tar.gz',
      unpacksAt: DATA_HOME,
      sha256: '1e43f6070b3f2c4acc05de692eaf82440c2b7b8d517acce0bdd6be3318968be6',
    },
    {
      file: 'creusot-prelude-0.15.0-0-noarch.tar.gz',
      unpacksAt: DATA_HOME,
      sha256: 'bcd3828c507f4c0b401e0a8264f8dad56b7854421877e10ab33d23f6ec7911af',
    },
    {
      file: 'why3find-0.15.0-0-x86_64-unknown-linux-gnu.tar.gz',
      unpacksAt: DATA_HOME,
      sha256: '42d3a1350bc6a735107e1614790ace33203b6d138c0b6cd9b41b4cb9fcf97665',
    },
    {
      file: 'why3-0.15.0-0-x86_64-unknown-linux-gnu.tar.gz',
      unpacksAt: WHY3_PREFIX,
      sha256: '47019b5caef45f2fcb7374c6c7e6795e437cf8485c421b74db9a9397155f4071',
    },
  ],
}

// The release's `rust_channel`, as the components rustup's minimal profile
// installs. `cargo-creusot` selects it by this toolchain's name.
const NIGHTLY = {
  toolchain: 'nightly-2026-08-03-x86_64-unknown-linux-gnu',
  url: 'https://static.rust-lang.org/dist/2026-08-03',
  components: [
    {
      file: 'rustc-nightly-x86_64-unknown-linux-gnu.tar.xz',
      sha256: 'eb2e77ec4d5df53e28c295ace9d31e6443836a9a136095c31ad3eabc17ea3ee2',
    },
    {
      file: 'rust-std-nightly-x86_64-unknown-linux-gnu.tar.xz',
      sha256: '481433aa0c1a65ded03e4d9c5ca420095b8ed7c68b483dc97e39231fb6afa4e5',
    },
    {
      file: 'cargo-nightly-x86_64-unknown-linux-gnu.tar.xz',
      sha256: 'c195a52932c64994fd360cf4cbf95eff5ad98748e8024623cd7214619e26635b',
    },
  ],
}

// Where `cargo-creusot` looks for its tools, by the rules it follows.
export function dataHome (env = process.env, home = os.homedir()) {
  if (env.CREUSOT_DATA_HOME) return env.CREUSOT_DATA_HOME
  if (env.XDG_DATA_HOME) return path.join(env.XDG_DATA_HOME, 'creusot')
  return path.join(home, '.local', 'share', 'creusot')
}

// Where rustup keeps its toolchains, by the rules it follows.
export function rustupHome (env = process.env, home = os.homedir()) {
  return env.RUSTUP_HOME || path.join(home, '.rustup')
}

export function sha256 (bytes) {
  return createHash('sha256').update(bytes).digest('hex')
}

// The copy of `file` kept in `downloads`, or `undefined` when there is none or
// it no longer hashes to `expected`.
export function keptDownload (downloads, file, expected) {
  if (downloads == null) return undefined
  const kept = path.join(downloads, file)
  if (!fs.existsSync(kept)) return undefined
  const bytes = fs.readFileSync(kept)
  return sha256(bytes) === expected ? bytes : undefined
}

// Fails unless `root` holds only directories, regular files, and symlinks
// whose relative targets stay inside it, which is all a release archive
// holds.
export function checkTree (root, directory = root) {
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const entryPath = path.join(directory, entry.name)
    if (entry.isDirectory()) {
      checkTree(root, entryPath)
    } else if (entry.isSymbolicLink()) {
      const target = fs.readlinkSync(entryPath)
      const resolved = path.resolve(path.dirname(entryPath), target)
      if (path.isAbsolute(target) || !resolved.startsWith(root + path.sep)) {
        throw new Error(`${path.relative(root, entryPath)} points outside the archive, at ${target}`)
      }
    } else if (!entry.isFile()) {
      throw new Error(`${path.relative(root, entryPath)} is neither a file, a directory nor a symlink`)
    }
  }
}

async function download (url, file, expected, downloads) {
  const kept = keptDownload(downloads, file, expected)
  if (kept != null) return kept
  const response = await fetch(`${url}/${file}`)
  if (!response.ok) throw new Error(`${url}/${file} answered ${response.status} ${response.statusText}`)
  const bytes = Buffer.from(await response.arrayBuffer())
  const actual = sha256(bytes)
  if (actual !== expected) throw new Error(`${file} hashes to ${actual}, not the pinned ${expected}`)
  if (downloads != null) {
    fs.mkdirSync(downloads, { recursive: true })
    fs.writeFileSync(path.join(downloads, file), bytes)
  }
  return bytes
}

function run (command, args, env = process.env) {
  const result = spawnSync(command, args, { stdio: ['ignore', 'inherit', 'inherit'], env })
  if (result.error != null) throw result.error
  if (result.status !== 0) throw new Error(`${command} ${args.join(' ')} exited with ${result.status ?? result.signal}`)
}

function withScratch (callback) {
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-install-creusot-'))
  try {
    return callback(scratch)
  } finally {
    fs.rmSync(scratch, { recursive: true, force: true })
  }
}

// Unpack `archive`, named `file`, under `scratch` and return where it went.
function unpack (archive, file, scratch) {
  const archivePath = path.join(scratch, file)
  const unpacked = path.join(scratch, 'unpacked')
  fs.writeFileSync(archivePath, archive)
  fs.mkdirSync(unpacked)
  run('tar', ['-xf', archivePath, '-C', unpacked])
  checkTree(unpacked)
  return unpacked
}

async function installRelease (downloads, data) {
  const prefixes = { [DATA_HOME]: data, [WHY3_PREFIX]: RELEASE.why3Prefix }
  for (const archive of RELEASE.archives) {
    const prefix = prefixes[archive.unpacksAt]
    process.stderr.write(`${archive.file} into ${prefix}\n`)
    const bytes = await download(RELEASE.url, archive.file, archive.sha256, downloads)
    withScratch((scratch) => {
      const unpacked = unpack(bytes, archive.file, scratch)
      fs.mkdirSync(prefix, { recursive: true })
      fs.cpSync(unpacked, prefix, { recursive: true, force: true, verbatimSymlinks: true })
      // `cargo-creusot` spawns its tools from the data directory's `bin`.
      const bin = path.join(unpacked, 'bin')
      if (archive.unpacksAt !== WHY3_PREFIX || !fs.existsSync(bin)) return
      fs.mkdirSync(path.join(data, 'bin'), { recursive: true })
      for (const program of fs.readdirSync(bin)) {
        const link = path.join(data, 'bin', program)
        fs.rmSync(link, { force: true })
        fs.symlinkSync(path.join(prefix, 'bin', program), link)
      }
    })
  }
}

// Install the nightly's components into a new directory, then put it where
// rustup looks for the toolchain, in place of whatever was there.
async function installNightly (downloads) {
  const toolchains = path.join(rustupHome(), 'toolchains')
  const toolchain = path.join(toolchains, NIGHTLY.toolchain)
  process.stderr.write(`${NIGHTLY.toolchain} into ${toolchains}\n`)
  fs.mkdirSync(toolchains, { recursive: true })
  const staging = fs.mkdtempSync(`${toolchain}.staging-`)
  try {
    for (const component of NIGHTLY.components) {
      const bytes = await download(NIGHTLY.url, component.file, component.sha256, downloads)
      withScratch((scratch) => {
        const unpacked = unpack(bytes, component.file, scratch)
        const installer = path.join(unpacked, component.file.replace(/\.tar\.xz$/, ''), 'install.sh')
        run(installer, [`--prefix=${staging}`, '--disable-ldconfig'])
      })
    }
    fs.rmSync(toolchain, { recursive: true, force: true })
    fs.renameSync(staging, toolchain)
  } catch (error) {
    fs.rmSync(staging, { recursive: true, force: true })
    throw error
  }
}

async function main () {
  const { values } = parseArgs({ options: { downloads: { type: 'string' } } })
  const downloads = values.downloads == null ? undefined : path.resolve(values.downloads)
  const data = dataHome()
  await installRelease(downloads, data)
  await installNightly(downloads)
  run(path.join(data, 'bin', 'creusot-install'), ['provers'], { ...process.env, CREUSOT_DATA_HOME: data })
  process.stderr.write(`installed Creusot; put ${path.join(data, 'bin')} on PATH\n`)
}

if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) {
  try {
    await main()
  } catch (error) {
    process.stderr.write(`${path.basename(fileURLToPath(import.meta.url))}: ${error.message}\n`)
    process.exitCode = 1
  }
}
