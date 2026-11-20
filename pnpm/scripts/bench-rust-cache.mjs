import { spawnSync } from 'node:child_process'
import console from 'node:console'
import { createHash } from 'node:crypto'
import fs from 'node:fs'
import net from 'node:net'
import path from 'node:path'
import { performance } from 'node:perf_hooks'
import process from 'node:process'
import { parseArgs } from 'node:util'

const { values } = parseArgs({
  options: {
    package: { type: 'string', default: 'pnpm-graph-hasher' },
    source: { type: 'string', default: 'pnpm/crates/graph-hasher/src/lib.rs' },
    rounds: { type: 'string', default: '3' },
    output: { type: 'string' },
    help: { type: 'boolean' },
  },
})
if (values.help) {
  console.log('node pnpm/scripts/bench-rust-cache.mjs [--package NAME --source PATH] [--rounds 3] [--output NEW_DIRECTORY]')
  process.exit(0)
}
const rounds = Number(values.rounds)
if (!Number.isSafeInteger(rounds) || rounds < 1) throw new Error('--rounds must be a positive integer')
const repo = run('git', ['rev-parse', '--show-toplevel']).trim()
const revision = run('git', ['rev-parse', 'HEAD'], { cwd: repo }).trim()
const output = path.resolve(values.output ?? path.join(repo, 'bench-work-env', `rust-cache-${Date.now()}`))
fs.mkdirSync(path.dirname(output), { recursive: true })
fs.mkdirSync(output)
const worktrees = ['a', 'b'].map(name => path.join(output, name))
const modes = ['incremental', 'sccache', 'shared-target', 'reflink-snapshot']
const results = []
const metadata = {
  revision,
  package: values.package,
  source: values.source,
  rounds,
  rustc: run('rustc', ['-Vv']).trim(),
  cargo: run('cargo', ['--version']).trim(),
  sccache: run('sccache', ['--version']).trim(),
  platform: process.platform,
  arch: process.arch,
}
const env = Object.fromEntries(Object.entries(process.env).filter(([name]) =>
  !name.startsWith('SCCACHE_') && !name.startsWith('CARGO_PROFILE_') &&
  !['RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'CARGO_TARGET_DIR', 'CARGO_BUILD_TARGET_DIR',
    'CARGO_BUILD_BUILD_DIR', 'CARGO_BUILD_INCREMENTAL', 'CARGO_INCREMENTAL',
    'CARGO_ENCODED_RUSTFLAGS', 'RUSTFLAGS', 'CARGO_MAKEFLAGS'].includes(name)))
Object.assign(env, { RUSTC_WRAPPER: '', RUSTC_WORKSPACE_WRAPPER: '', RUSTFLAGS: '' })
fs.writeFileSync(path.join(output, 'sccache.toml'), '')
const registered = []
try {
  for (const worktree of worktrees) {
    run('git', ['worktree', 'add', '--detach', worktree, revision], { cwd: repo })
    registered.push(worktree)
  }
  const sources = worktrees.map(worktree => {
    const worktreeRoot = fs.realpathSync(worktree)
    const source = fs.realpathSync(path.resolve(worktreeRoot, values.source))
    if (!source.startsWith(`${worktreeRoot}${path.sep}`)) throw new Error('--source must be inside the worktree')
    return source
  })
  const original = fs.readFileSync(sources[1], 'utf8')
  const sourceAtRevision = revision => `${original}\npub fn pnpm_cache_bench_revision() -> u8 { ${revision} }\n`
  for (let round = 0; round < rounds; round++) {
    // Rotate the first mode to reduce systematic filesystem-cache ordering bias.
    const ordered = modes.slice(round % modes.length).concat(modes.slice(0, round % modes.length))
    for (const mode of ordered) {
      const directory = path.join(output, `round-${round + 1}`, mode)
      fs.mkdirSync(directory, { recursive: true })
      for (const source of sources) fs.writeFileSync(source, sourceAtRevision(0))
      const targets = mode === 'shared-target'
        ? [path.join(directory, 'target'), path.join(directory, 'target')]
        : ['target-a', 'target-b'].map(name => path.join(directory, name))
      const buildEnv = { ...env, CARGO_INCREMENTAL: mode === 'sccache' ? '0' : '1' }
      if (mode === 'sccache') {
        Object.assign(buildEnv, {
          RUSTC_WRAPPER: 'sccache',
          SCCACHE_DIR: path.join(directory, 'sccache'),
          SCCACHE_CONF: path.join(output, 'sccache.toml'),
          SCCACHE_CACHED_CONF: path.join(directory, 'cached-config'),
          SCCACHE_SERVER_PORT: String(await unusedPort()),
          SCCACHE_CACHE_SIZE: '10G',
          SCCACHE_IDLE_TIMEOUT: '0',
        })
        run('sccache', ['--start-server'], { env: buildEnv })
      }
      const result = { round: round + 1, mode, builds: [] }
      try {
        build(0, 'first-a')
        build(0, 'warm-a')
        if (mode === 'reflink-snapshot') {
          const start = performance.now()
          run('cp', ['--archive', '--reflink=always', targets[0], targets[1]])
          result.snapshotSeconds = (performance.now() - start) / 1000
        }
        build(1, 'first-b')
        result.storage = await measureContent([...new Set(targets)])
        fs.writeFileSync(sources[1], sourceAtRevision(1))
        build(1, 'edit-b')
        build(0, 'return-a')
        build(1, 'warm-b')
        if (mode === 'sccache') {
          result.sccache = JSON.parse(run('sccache', ['--show-stats', '--stats-format', 'json'], { env: buildEnv }))
          result.sccacheStorage = await measureContent([path.join(directory, 'sccache')])
        }
        results.push(result)
        fs.writeFileSync(path.join(output, 'results.json'), `${JSON.stringify({ metadata, results }, null, 2)}\n`)
      } finally {
        for (const worktree of worktrees) run('git', ['restore', '--', values.source], { cwd: worktree })
        if (mode === 'sccache') run('sccache', ['--stop-server'], { env: buildEnv })
      }

      function build(index, phase) {
        const start = performance.now()
        const stdout = run('cargo', ['build', '--locked', '--offline', '-p', values.package, '--message-format=json'], {
          cwd: worktrees[index],
          env: { ...buildEnv, CARGO_TARGET_DIR: targets[index], CARGO_BUILD_BUILD_DIR: targets[index] },
          stderrFile: path.join(directory, `${phase}.log`),
        })
        const seconds = (performance.now() - start) / 1000
        const artifacts = stdout.split('\n').filter(Boolean).map(line => JSON.parse(line))
          .filter(message => message.reason === 'compiler-artifact')
        const compiled = artifacts.filter(artifact => !artifact.fresh).map(artifact => artifact.package_id)
        const library = artifacts.find(artifact => artifact.target.name === values.package.replaceAll('-', '_') &&
          artifact.filenames.some(file => file.endsWith('.rlib')))
        if (!library) throw new Error('--package must name a library with an rlib output')
        const libraryPath = library.filenames.find(file => file.endsWith('.rlib'))
        const probeSource = path.join(directory, 'probe.rs')
        const probeBinary = path.join(directory, 'probe')
        fs.writeFileSync(probeSource, 'fn main() { println!("{}", cache_probe::pnpm_cache_bench_revision()); }\n')
        run('rustc', [probeSource, '--edition=2024', '--extern', `cache_probe=${libraryPath}`,
          '-L', `dependency=${path.join(path.dirname(libraryPath), 'deps')}`, '-o', probeBinary], { cwd: repo, env })
        const actualRevision = Number(run(probeBinary, []).trim())
        const expectedRevision = index === 1 && ['edit-b', 'warm-b'].includes(phase) ? 1 : 0
        const correct = actualRevision === expectedRevision
        const item = { phase, seconds, fresh: artifacts.length - compiled.length, compiled, expectedRevision, actualRevision, correct }
        result.builds.push(item)
        console.log(`round ${round + 1} ${mode} ${phase}: ${seconds.toFixed(3)}s, ${compiled.length} compiled, ${item.fresh} fresh, ${correct ? 'correct' : `WRONG OUTPUT: expected ${expectedRevision}, got ${actualRevision}`}`)
      }
    }
  }
} finally {
  for (const worktree of registered) run('git', ['worktree', 'remove', '--force', worktree], { cwd: repo })
}
console.log(`Results and build logs: ${output}`)

function run(command, args, options = {}) {
  const { stderrFile, ...spawnOptions } = options
  const result = spawnSync(command, args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, ...spawnOptions })
  if (stderrFile) fs.writeFileSync(stderrFile, result.stderr ?? '')
  if (result.error) throw result.error
  if (result.status !== 0) throw new Error(`${command} ${args.join(' ')} failed (${result.status}):\n${result.stderr}`)
  return result.stdout
}

async function unusedPort() {
  const server = net.createServer()
  await new Promise((resolve, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', resolve)
  })
  const port = server.address().port
  await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()))
  return port
}

async function measureContent(roots) {
  let logicalBytes = 0
  let uniqueBytes = 0
  let files = 0
  const hashes = new Set()
  const inodes = new Set()
  let allocatedBytes = 0
  let inodeBytes = 0
  for (const root of roots) await walk(root)
  return { files, logicalBytes, inodeBytes, uniqueBytes, duplicateBytes: inodeBytes - uniqueBytes, allocatedBytes }

  async function walk(directory) {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const file = path.join(directory, entry.name)
      if (entry.isDirectory()) {
        await walk(file)
      } else if (entry.isFile()) {
        const stat = fs.statSync(file)
        const inode = `${stat.dev}:${stat.ino}`
        if (!inodes.has(inode)) {
          allocatedBytes += stat.blocks * 512
          inodeBytes += stat.size
        }
        inodes.add(inode)
        const hash = createHash('sha512')
        for await (const chunk of fs.createReadStream(file)) hash.update(chunk)
        const key = `${hash.digest('hex')}:${Boolean(stat.mode & 0o111)}`
        if (!hashes.has(key)) uniqueBytes += stat.size
        hashes.add(key)
        logicalBytes += stat.size
        files++
      }
    }
  }
}
