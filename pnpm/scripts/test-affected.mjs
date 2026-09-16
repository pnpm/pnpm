import { spawnSync } from 'node:child_process'
import console from 'node:console'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath, pathToFileURL } from 'node:url'

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
  // A dev-dependency of most crates: a change here can alter how any test in
  // the workspace behaves, not just this crate's own.
  'pnpm/crates/testing-utils/',
  // The packages the mocked registry serves, which nearly every end-to-end
  // install resolves against.
  'pnpr/.fixtures/',
]

/**
 * Test inputs that live outside the crate that reads them, mapped to the
 * packages that do.
 *
 * Cargo attributes a file to the manifest above it, so these would otherwise
 * be attributed to no crate at all and report that nothing changed.
 */
const EXTERNAL_TEST_INPUTS = [
  { prefix: 'fixtures/', packages: ['pnpm-deps-restorer'] },
  { prefix: 'pnpm11/installing/deps-installer/test/fixtures/patch-pkg/', packages: ['pnpm-cli'] },
  { prefix: 'pnpm11/deps/compliance/commands/test/sbom/fixtures/', packages: ['pnpm-cli'] },
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
    for (const input of EXTERNAL_TEST_INPUTS) {
      if (file.startsWith(input.prefix)) for (const name of input.packages) selected.add(name)
    }
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

/**
 * How many crates depend on each selected crate without being selected
 * themselves, keyed by crate name and counted transitively.
 *
 * Selection is crate-level, so a change to a widely used crate leaves its
 * dependents' tests unrun. Reporting the count is what lets the caller decide
 * whether to widen the selection or leave those tests to CI.
 */
export function unselectedDependents (selected, packages) {
  const dependents = new Map(packages.map(pkg => [pkg.name, new Set()]))
  for (const pkg of packages) {
    for (const dependency of pkg.dependencies) {
      dependents.get(dependency)?.add(pkg.name)
    }
  }
  const counts = new Map()
  for (const name of selected) {
    const seen = new Set()
    const pending = [name]
    while (pending.length > 0) {
      for (const dependent of dependents.get(pending.pop()) ?? []) {
        if (seen.has(dependent)) continue
        seen.add(dependent)
        pending.push(dependent)
      }
    }
    const unselected = [...seen].filter(dependent => !selected.includes(dependent))
    if (unselected.length > 0) counts.set(name, unselected.length)
  }
  return counts
}

/**
 * This script's own options, and every other argument in the order it was
 * given. `node:util`'s `parseArgs` cannot do this: in non-strict mode it
 * collects unknown flags into `values` and leaves their operands behind as
 * positionals, so `-p pnpm-cli` would reach nextest as a bare `pnpm-cli` test
 * name filter.
 */
export function parseOptions (argv) {
  const values = { base: 'main', print: false, help: false }
  const rest = []
  for (let index = 0; index < argv.length; index++) {
    const arg = argv[index]
    if (arg === '--') {
      rest.push(...argv.slice(index + 1))
      break
    }
    if (arg === '--print') values.print = true
    else if (arg === '--help' || arg === '-h') values.help = true
    else if (arg.startsWith('--base=')) values.base = arg.slice('--base='.length)
    else if (arg === '--base') {
      if (index + 1 === argv.length) throw new Error('--base needs a revision')
      values.base = argv[++index]
    } else rest.push(arg)
  }
  return { values, rest }
}

function main () {
  const { values, rest } = parseOptions(process.argv.slice(2))
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

  const manifests = workspaceManifests(repo)
  const packages = selectPackages(changed, manifests)
  if (packages.length === 0) {
    console.log('No Rust crates changed.')
    return 0
  }

  console.log(`Testing ${packages.length} crate(s) changed against ${values.base}:`)
  const dependents = unselectedDependents(packages, manifests)
  for (const name of packages) {
    const count = dependents.get(name)
    console.log(count == null ? `  ${name}` : `  ${name} (${count} crates depend on it; their tests are not selected)`)
  }
  if (!packages.includes('pnpm-cli') && !packages.every(isPnprPackage)) {
    console.log('\nThe CLI end-to-end suite is not in this selection. For a user-visible change, add')
    console.log("  -p pnpm-cli -E 'test(<area>::)'")
    console.log('with the suite modules that exercise the change.')
  }

  const runner = path.join(repo, 'pnpm/scripts/run-rust-tests.mjs')
  const nextestArgs = packages.flatMap(name => ['-p', name]).concat(rest)
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
  const names = new Set(metadata.packages.map(pkg => pkg.name))
  return metadata.packages.map(pkg => ({
    name: pkg.name,
    dir: path.relative(repo, path.dirname(pkg.manifest_path)).split(path.sep).join('/'),
    dependencies: [...new Set(pkg.dependencies.map(dependency => dependency.name).filter(name => names.has(name)))],
  }))
}

function changedFiles (repo, base) {
  // A base that git cannot resolve must not read as "nothing changed": that
  // reports success on a branch whose commits were never tested.
  const merged = run(repo, 'git', ['merge-base', base, 'HEAD'], { allowFailure: true }).trim()
  if (merged === '') {
    throw new Error(`no merge base between '${base}' and HEAD. Fetch the branch, or pass --base <revision>.`)
  }
  const committed = run(repo, 'git', ['diff', '--name-only', merged, 'HEAD'])
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
