import { spawnSync } from 'node:child_process'
import console from 'node:console'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { parseArgs } from 'node:util'

// Files that change how every crate compiles or how every test runs. A change
// to one of them can break any test in the workspace, so there is no honest
// subset to run. Formatting and lint configuration is left out: neither can
// change a test result.
const WORKSPACE_WIDE = [
  'Cargo.toml',
  'Cargo.lock',
  'rust-toolchain.toml',
  '.cargo/',
  '.config/nextest.toml',
  'pnpm/scripts/run-rust-tests.mjs',
]

export function workspaceWideChanges (files) {
  return files.filter(file => WORKSPACE_WIDE.some(entry =>
    entry.endsWith('/') ? file.startsWith(entry) : file === entry))
}

/**
 * The crates to test for a set of changed files, as package names.
 *
 * `manifests` pairs each workspace package name with its directory relative to
 * the repository root. A file belongs to the package with the longest matching
 * directory, so a crate nested inside another crate's directory wins over its
 * parent.
 */
export function selectPackages (files, manifests) {
  const selected = new Set()
  for (const file of files) {
    const owner = manifests
      .filter(({ dir }) => file === dir || file.startsWith(`${dir}/`))
      .sort((a, b) => b.dir.length - a.dir.length)[0]
    if (owner != null) selected.add(owner.name)
  }
  // Cargo unifies features across the selected packages, so a lone `pnpr-*`
  // crate builds without the backend features `pnpr` enables by default and
  // its backend tests skip without saying so.
  if ([...selected].some(isPnprPackage)) {
    for (const { name } of manifests) {
      if (isPnprPackage(name)) selected.add(name)
    }
  }
  return [...selected].sort()
}

export function isPnprPackage (name) {
  return name.startsWith('pnpr') || name === 'pnpm-registry-mock'
}

function main () {
  const { values, positionals } = parseArgs({
    allowPositionals: true,
    strict: false,
    options: {
      base: { type: 'string', default: 'main' },
      print: { type: 'boolean', default: false },
      help: { type: 'boolean', default: false },
    },
  })
  if (values.help) {
    console.log(`node pnpm/scripts/test-affected.mjs [--base main] [--print] [<nextest args>]

Runs the tests of every crate the working tree changes relative to --base.
Selection is crate-level: a crate's whole test set runs, or none of it.`)
    return 0
  }

  const repo = spawnSync('git', ['rev-parse', '--show-toplevel'], { encoding: 'utf8' }).stdout.trim()
  const changed = changedFiles(repo, values.base)
  if (changed.length === 0) {
    console.log(`No files changed against ${values.base}.`)
    return 0
  }

  const global = workspaceWideChanges(changed)
  if (global.length > 0) {
    console.error(`These files affect every crate in the workspace:\n${global.map(file => `  ${file}`).join('\n')}\n`)
    console.error('There is no meaningful subset to run. Use `just ready`.')
    return 1
  }

  const packages = selectPackages(changed, workspaceManifests(repo))
  if (packages.length === 0) {
    console.log('No Rust crates changed.')
    return 0
  }

  console.log(`Testing ${packages.length} crate(s) changed against ${values.base}:`)
  for (const name of packages) console.log(`  ${name}`)
  if (!packages.includes('pnpm-cli') && !packages.every(isPnprPackage)) {
    console.log('\nThe CLI end-to-end suite is not in this selection. For a user-visible change, add')
    console.log("  -p pnpm-cli -E 'test(<area>::)'")
    console.log('with the suite modules that exercise the change.')
  }

  const runner = path.join(repo, 'pnpm/scripts/run-rust-tests.mjs')
  const nextestArgs = packages.flatMap(name => ['-p', name]).concat(positionals)
  if (values.print) {
    console.log(`\nnode ${path.relative(repo, runner)} ${nextestArgs.join(' ')}`)
    return 0
  }

  const result = spawnSync('node', [runner, ...nextestArgs], { stdio: 'inherit' })
  if (result.error != null) throw result.error
  return result.status ?? 1
}

function workspaceManifests (repo) {
  const metadata = JSON.parse(run(repo, 'cargo', ['metadata', '--format-version', '1', '--no-deps', '--offline']))
  return metadata.packages.map(pkg => ({
    name: pkg.name,
    dir: path.relative(repo, path.dirname(pkg.manifest_path)).split(path.sep).join('/'),
  }))
}

function changedFiles (repo, base) {
  const merged = run(repo, 'git', ['merge-base', base, 'HEAD'], { allowFailure: true }).trim()
  const committed = merged === '' ? '' : run(repo, 'git', ['diff', '--name-only', merged, 'HEAD'])
  const working = run(repo, 'git', ['diff', '--name-only', 'HEAD'])
  const untracked = run(repo, 'git', ['ls-files', '--others', '--exclude-standard'])
  return [...new Set(`${committed}\n${working}\n${untracked}`.split('\n').filter(line => line !== ''))]
}

function run (repo, command, args, { allowFailure = false } = {}) {
  const result = spawnSync(command, args, { encoding: 'utf8', cwd: repo })
  if (result.error != null) throw result.error
  if (result.status !== 0) {
    if (allowFailure) return ''
    throw new Error(`${command} ${args.join(' ')} failed:\n${result.stderr}`)
  }
  return result.stdout
}

if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) {
  try {
    process.exitCode = main()
  } catch (error) {
    process.stderr.write(`${path.basename(fileURLToPath(import.meta.url))}: ${error.message}\n`)
    process.exitCode = 1
  }
}
