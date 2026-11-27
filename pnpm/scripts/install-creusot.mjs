// Install Creusot from a release of creusot-prebuilt: every asset the release
// holds, the nightly it was built against, and the SMT solvers it proves
// with. See `pnpm/VERIFICATION.md`.
//
// The release describes itself in `release.json`, which this pins by its
// SHA-256. That file records every asset's checksum, so the one pin covers
// the whole release.
//
// The release's Why3 has its prefix compiled in, so the directory the
// release names, `/opt/creusot-why3`, has to exist and be writable first.
// `cargo creusot` is on PATH once the data directory's `bin` is.
import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath, pathToFileURL } from 'node:url'

const RELEASE = {
  repository: 'KSXGitHub/creusot-prebuilt',
  tag: '0.15.0-0',
  metadataSha256: '3a028ec45b2ce00b3a2e3a4a992043cffde49929e9fcd25239835cdb0a130a9d',
}

// The `prefix` an asset in `release.json` unpacks at.
const DATA_HOME_PREFIX = 'creusot-data-home'
const WHY3_PREFIX = 'why3-prefix'

export function parseRelease (text) {
  const release = JSON.parse(text)
  for (const key of ['rust_channel', 'why3_prefix']) {
    if (typeof release[key] !== 'string') throw new Error(`release.json has no string ${key}`)
  }
  if (!Array.isArray(release.assets) || release.assets.length === 0) {
    throw new Error('release.json lists no assets')
  }
  for (const asset of release.assets) {
    for (const key of ['name', 'file', 'prefix', 'sha256']) {
      if (typeof asset[key] !== 'string') throw new Error(`an asset in release.json has no string ${key}`)
    }
  }
  return release
}

// Where `cargo-creusot` looks for its tools, by the rules it follows.
export function dataHome (env = process.env, home = os.homedir()) {
  if (env.CREUSOT_DATA_HOME) return env.CREUSOT_DATA_HOME
  if (env.XDG_DATA_HOME) return path.join(env.XDG_DATA_HOME, 'creusot')
  return path.join(home, '.local', 'share', 'creusot')
}

export function sha256 (bytes) {
  return createHash('sha256').update(bytes).digest('hex')
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

async function download (name) {
  const url = `https://github.com/${RELEASE.repository}/releases/download/${RELEASE.tag}/${name}`
  const response = await fetch(url)
  if (!response.ok) throw new Error(`${url} answered ${response.status} ${response.statusText}`)
  return Buffer.from(await response.arrayBuffer())
}

function run (command, args, env = process.env) {
  const result = spawnSync(command, args, { stdio: ['ignore', 'inherit', 'inherit'], env })
  if (result.error != null) throw result.error
  if (result.status !== 0) throw new Error(`${command} ${args.join(' ')} exited with ${result.status ?? result.signal}`)
}

// Unpack the archive at `prefix` and return the entries of its `bin`.
function unpack (archive, prefix, scratch) {
  const archivePath = path.join(scratch, 'asset.tar.gz')
  const unpacked = path.join(scratch, 'unpacked')
  fs.writeFileSync(archivePath, archive)
  fs.mkdirSync(unpacked)
  run('tar', ['-xzf', archivePath, '-C', unpacked])
  checkTree(unpacked)
  fs.mkdirSync(prefix, { recursive: true })
  fs.cpSync(unpacked, prefix, { recursive: true, force: true, verbatimSymlinks: true })
  const bin = path.join(unpacked, 'bin')
  return fs.existsSync(bin) ? fs.readdirSync(bin) : []
}

async function main () {
  const metadata = await download('release.json')
  if (sha256(metadata) !== RELEASE.metadataSha256) {
    throw new Error(`release.json of ${RELEASE.tag} hashes to ${sha256(metadata)}, not the pinned ${RELEASE.metadataSha256}`)
  }
  const release = parseRelease(metadata.toString('utf8'))
  const data = dataHome()
  const prefixes = { [DATA_HOME_PREFIX]: data, [WHY3_PREFIX]: release.why3_prefix }
  for (const asset of release.assets) {
    const prefix = prefixes[asset.prefix]
    if (prefix == null) throw new Error(`${asset.name} unpacks at the unknown prefix ${asset.prefix}`)
    process.stderr.write(`${asset.name} into ${prefix}\n`)
    const archive = await download(asset.file)
    if (sha256(archive) !== asset.sha256) {
      throw new Error(`${asset.file} hashes to ${sha256(archive)}, not the ${asset.sha256} release.json records`)
    }
    const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-install-creusot-'))
    try {
      const programs = unpack(archive, prefix, scratch)
      // `cargo-creusot` spawns its tools from the data directory's `bin`.
      if (asset.prefix === WHY3_PREFIX) {
        fs.mkdirSync(path.join(data, 'bin'), { recursive: true })
        for (const program of programs) {
          const link = path.join(data, 'bin', program)
          fs.rmSync(link, { force: true })
          fs.symlinkSync(path.join(prefix, 'bin', program), link)
        }
      }
    } finally {
      fs.rmSync(scratch, { recursive: true, force: true })
    }
  }
  run('rustup', ['toolchain', 'install', release.rust_channel, '--profile', 'minimal', '--no-self-update'])
  run(path.join(data, 'bin', 'creusot-install'), ['provers'], { ...process.env, CREUSOT_DATA_HOME: data })
  process.stderr.write(`installed creusot-prebuilt ${RELEASE.tag}; put ${path.join(data, 'bin')} on PATH\n`)
}

if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) {
  try {
    await main()
  } catch (error) {
    process.stderr.write(`${path.basename(fileURLToPath(import.meta.url))}: ${error.message}\n`)
    process.exitCode = 1
  }
}
