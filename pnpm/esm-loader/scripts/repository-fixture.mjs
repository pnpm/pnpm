import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import fs from 'node:fs'
import { createRequire } from 'node:module'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath, URL } from 'node:url'

export async function prepareRepository (repo, options = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-loader-battle-'))
  const projects = JSON.parse(execFileSync('pnpm', ['-r', 'list', '--depth=-1', '--json'], { cwd: repo, encoding: 'utf8' }))
  const workspacePaths = new Set(projects.map(project => project.path))
  workspacePaths.add(repo)
  const state = {
    repo, root, workspacePaths, ids: new Map(), missing: [], fileCount: 0, bytes: 0,
    manifest: { version: 1, storeDir: './store', packages: {} },
  }
  const entries = options.fullRepository ? [...workspacePaths] : [repo, path.join(repo, 'pnpm11/pnpm'), path.join(repo, 'pnpm11/cli/parse-cli-args')]
  for (const directory of entries) addPackage(state, directory)
  inheritWorkspaceDependencies(state.manifest)
  if (options.fullRepository) copyDirectory(repo, path.join(root, 'repo'))
  else copyWorkspaces(state)
  await bundleLoader(root)
  fs.writeFileSync(path.join(root, '.store-manifest.json'), JSON.stringify(state.manifest))
  fs.writeFileSync(path.join(root, 'repository-source.json'), JSON.stringify({ repo, packages: Object.fromEntries([...state.ids].map(([directory, id]) => [id, directory])) }))
  fs.writeFileSync(path.join(root, 'missing-dependencies.json'), JSON.stringify(state.missing, null, 2))
  fs.writeFileSync(path.join(root, 'repo/run-jest.cjs'), "require('jest').run(process.argv.slice(2))\n")
  assertIsolated(root)
  return state
}

function addPackage (state, directory) {
  if (state.ids.has(directory)) return state.ids.get(directory)
  const pkg = JSON.parse(fs.readFileSync(path.join(directory, 'package.json'), 'utf8').trimStart())
  const workspace = state.workspacePaths.has(directory)
  const id = workspace ? path.relative(state.repo, directory) || '.' : `${pkg.name}@${pkg.version}:${digest(directory, 'sha256').slice(0, 16)}`
  const entry = { dependencies: {} }
  state.ids.set(directory, id)
  state.manifest.packages[id] = entry
  if (typeof pkg.name === 'string' && findDependency(directory, pkg.name) === directory) entry.dependencies[pkg.name] = id
  if (workspace) entry.root = path.join('repo', path.relative(state.repo, directory))
  else entry.files = storeFiles(state, directory)
  const dependencies = { ...pkg.dependencies, ...pkg.optionalDependencies, ...pkg.peerDependencies, ...(workspace ? pkg.devDependencies : {}) }
  for (const name of Object.keys(dependencies)) {
    const resolved = findDependency(directory, name)
    if (resolved) entry.dependencies[name] = addPackage(state, resolved)
    else state.missing.push({ issuer: id, name })
  }
  return id
}

function storeFiles (state, directory) {
  const files = {}
  for (const [relative, filename] of walkFiles(directory)) {
    const source = fs.readFileSync(filename)
    const executable = (fs.statSync(filename).mode & 0o111) !== 0
    const hash = digest(source, 'sha512') + (executable ? '-exec' : '')
    files[relative] = hash
    const blob = path.join(state.root, 'store/files', hash.slice(0, 2), hash.slice(2))
    if (!fs.existsSync(blob)) {
      fs.mkdirSync(path.dirname(blob), { recursive: true })
      fs.writeFileSync(blob, source)
      state.bytes += source.length
    }
    state.fileCount++
  }
  return files
}

function digest (source, algorithm) {
  return createHash(algorithm).update(source).digest('hex')
}

function walkFiles (directory, prefix = '') {
  const files = []
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    if (excluded(entry.name)) continue
    const relative = prefix + entry.name
    const filename = path.join(directory, entry.name)
    if (entry.isDirectory()) files.push(...walkFiles(filename, relative + '/'))
    else if (entry.isFile() || (entry.isSymbolicLink() && fs.statSync(filename).isFile())) files.push([relative, filename])
  }
  return files
}

export function findDependency (issuer, name) {
  for (let directory = issuer; ; directory = path.dirname(directory)) {
    const filename = path.join(directory, 'node_modules', name, 'package.json')
    if (fs.existsSync(filename)) return fs.realpathSync(path.dirname(filename))
    if (path.dirname(directory) === directory) return null
  }
}

function copyWorkspaces ({ repo, root, manifest }) {
  fs.mkdirSync(path.join(root, 'repo'), { recursive: true })
  copyDirectory(path.join(repo, 'pnpm11'), path.join(root, 'repo/pnpm11'))
  for (const entry of Object.values(manifest.packages)) {
    if (!entry.root || entry.root === 'repo' || entry.root.startsWith(`repo${path.sep}pnpm11`)) continue
    copyDirectory(path.join(repo, path.relative('repo', entry.root)), path.join(root, entry.root))
  }
  for (const name of ['package.json', 'pnpm-workspace.yaml', 'pnpm-lock.yaml']) {
    fs.copyFileSync(path.join(repo, name), path.join(root, 'repo', name))
  }
}

function copyDirectory (source, target) {
  fs.cpSync(source, target, {
    recursive: true, verbatimSymlinks: true,
    filter: filename => !path.relative(source, filename).split(path.sep).some(excluded),
  })
}

function excluded (name) {
  return ['node_modules', '.git', 'target', '.cache', '.jest-cache', 'coverage', '_tmp'].includes(name)
}

export function assertIsolated (root, directory = root) {
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const filename = path.join(directory, entry.name)
    if (entry.name === 'node_modules') throw new Error(`Unexpected node_modules: ${filename}`)
    if (entry.isDirectory()) assertIsolated(root, filename)
    if (entry.isSymbolicLink()) {
      const target = path.resolve(path.dirname(filename), fs.readlinkSync(filename))
      if (!target.startsWith(root + path.sep)) throw new Error(`External symlink: ${filename} -> ${target}`)
    }
  }
}

function inheritWorkspaceDependencies (manifest) {
  const workspaces = Object.values(manifest.packages).filter(entry => entry.root).sort((first, second) => first.root.length - second.root.length)
  for (const entry of workspaces) {
    const parent = workspaces.findLast(candidate => entry.root.startsWith(candidate.root + path.sep))
    if (parent) entry.dependencies = { ...parent.dependencies, ...entry.dependencies }
  }
}

export async function bundleLoader (root) {
  fs.copyFileSync(new URL('./audit-cas.mjs', import.meta.url), path.join(root, 'audit-cas.mjs'))
  const require = createRequire(new URL('../../../pnpm11/pnpm/package.json', import.meta.url))
  await require('esbuild').build({
    entryPoints: [fileURLToPath(new URL('../register.mjs', import.meta.url))],
    bundle: true, platform: 'node', format: 'esm', outfile: path.join(root, 'loader.mjs'),
    banner: { js: "import { createRequire as bootstrapRequire } from 'node:module'; const require = bootstrapRequire(import.meta.url);" },
  })
}
