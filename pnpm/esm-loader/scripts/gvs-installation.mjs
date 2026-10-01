import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'

import { parse as parseDependencyPath, refToRelative } from '../../../pnpm11/deps/path/lib/index.js'
import { readModulesManifest } from '../../../pnpm11/installing/modules-yaml/lib/index.js'
import { readCurrentLockfile, writeWantedLockfile } from '../../../pnpm11/lockfile/fs/lib/index.js'
import { lockfileYamlDump } from '../../../pnpm11/lockfile/fs/lib/write.js'
import { pruneSharedLockfile } from '../../../pnpm11/lockfile/pruner/lib/index.js'
import { nameVerFromPkgSnapshot } from '../../../pnpm11/lockfile/utils/lib/index.js'
import { readWorkspaceManifest } from '../../../pnpm11/workspace/workspace-manifest-reader/lib/index.js'
import { within } from '../store.mjs'
import { findDependency } from './repository-fixture.mjs'

export async function installOptOuts (source, manifest, names) {
  const selected = selectPackages(manifest, names)
  const modules = await readModulesManifest(path.join(source.repo, 'node_modules'))
  if (!modules) throw new Error(`No installed dependency graph in ${source.repo}`)
  const lockfile = await readCurrentLockfile(modules.virtualStoreDir, { ignoreIncompatible: false }) ??
    await readCurrentLockfile(path.join(source.repo, 'node_modules/.pnpm'), { ignoreIncompatible: false })
  if (!lockfile) throw new Error(`No installed lockfile in ${modules.virtualStoreDir}`)
  const packageManifest = JSON.parse(fs.readFileSync(path.join(source.repo, 'package.json'), 'utf8'))
  const config = { ...packageManifest.pnpm, ...await readWorkspaceManifest(source.repo) }
  const locations = snapshotLocations(lockfile, source.repo)
  const installation = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-loader-gvs-'))
  const { importer, aliases } = createSelectedImporter(selected, { source, locations })
  const selectedLockfile = pruneSharedLockfile({ ...lockfile, importers: { '.': importer } })
  // The frozen snapshots already include hook results; staging must not execute the hooks again.
  delete selectedLockfile.pnpmfileChecksum
  delete selectedLockfile.untrackedPnpmfileReadPackageHook
  rejectWorkspaceLinks(selectedLockfile)
  fs.writeFileSync(path.join(installation, 'package.json'), JSON.stringify({ private: true, dependencies: importer.specifiers }))
  const selectedConfig = pruneUnusedPatches(selectedLockfile, config)
  fs.writeFileSync(path.join(installation, 'pnpm-workspace.yaml'), lockfileYamlDump(installationConfig(selectedConfig, source.repo)))
  await writeWantedLockfile(installation, selectedLockfile)
  const output = execFileSync('pnpm', ['install', '--frozen-lockfile', '--ignore-scripts'], {
    cwd: installation, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024,
    env: { ...process.env, NODE_OPTIONS: '', NODE_PATH: '', CI: 'true' },
  })
  fs.writeFileSync(path.join(installation, 'install.log'), output)
  const roots = aliases.map(({ id, alias }) => ({ id, directory: findDependency(installation, alias) }))
  const closure = mapInstalledClosure(manifest, roots, { source, locations })
  const globalVirtualStoreDir = await verifyGlobalStore({ installation, config, repo: source.repo }, closure.materialized)
  return { installation, globalVirtualStoreDir, selected, ...closure }
}

function pruneUnusedPatches (lockfile, config) {
  const hashes = new Set(Object.keys(lockfile.packages ?? {}).map(id => parseDependencyPath(id).patchHash))
  lockfile.patchedDependencies = Object.fromEntries(Object.entries(lockfile.patchedDependencies ?? {}).filter(([, hash]) => hashes.has(`(patch_hash=${hash})`)))
  const patchedDependencies = Object.fromEntries(Object.entries(config.patchedDependencies ?? {}).filter(([name]) => Object.hasOwn(lockfile.patchedDependencies, name)))
  return { ...config, patchedDependencies }
}

function createSelectedImporter (selected, { source, locations }) {
  const importer = { specifiers: {}, dependencies: {} }
  const aliases = selected.map((id, index) => {
    const metadata = locations.get(source.packages[id])
    if (!metadata) throw new Error(`Cannot find the locked package context for ${id}`)
    const alias = `opt-out-${index}`
    importer.specifiers[alias] = `npm:${metadata.name}@${metadata.version}`
    importer.dependencies[alias] = metadata.depPath
    return { id, alias }
  })
  return { importer, aliases }
}

async function verifyGlobalStore ({ installation, config, repo }, materialized) {
  const installedModules = await readModulesManifest(path.join(installation, 'node_modules'))
  if (!installedModules) throw new Error(`Missing installation state in ${installation}`)
  const globalVirtualStoreDir = fs.realpathSync(config.globalVirtualStoreDir
    ? path.resolve(repo, config.globalVirtualStoreDir)
    : path.join(installedModules.storeDir, 'links'))
  for (const directory of Object.values(materialized)) {
    if (!within(globalVirtualStoreDir, directory)) throw new Error(`Package was not installed into GVS: ${directory}`)
  }
  return globalVirtualStoreDir
}

function selectPackages (manifest, names) {
  if (names.length === 0) throw new Error('Select at least one package to opt out of CAS loading')
  const selected = new Set()
  for (const name of names) {
    const matches = Object.keys(manifest.packages).filter(id => manifest.packages[id].files && (id === name || id.startsWith(name + '@')))
    if (matches.length === 0) throw new Error(`No stored package matches ${name}`)
    for (const id of matches) selected.add(id)
  }
  return [...selected]
}

function snapshotLocations (lockfile, repo) {
  const locations = new Map()
  const pending = Object.entries(lockfile.importers).map(([relative, snapshot]) => ({ directory: path.resolve(repo, relative), snapshot }))
  while (pending.length > 0) {
    const { directory, snapshot } = pending.pop()
    const dependencies = { ...snapshot.dependencies, ...snapshot.devDependencies, ...snapshot.optionalDependencies }
    for (const [alias, reference] of Object.entries(dependencies)) {
      const depPath = refToRelative(reference, alias)
      const pkgSnapshot = lockfile.packages?.[depPath]
      if (!pkgSnapshot) continue
      const installed = findDependency(directory, alias)
      if (!installed || locations.has(installed)) continue
      locations.set(installed, { depPath, pkgSnapshot, ...nameVerFromPkgSnapshot(depPath, pkgSnapshot) })
      pending.push({ directory: installed, snapshot: pkgSnapshot })
    }
  }
  return locations
}

function installationConfig (config, repo) {
  const patchedDependencies = Object.fromEntries(Object.entries(config.patchedDependencies ?? {}).map(([name, filename]) => [name, path.resolve(repo, filename)]))
  return {
    ...config, packages: [], patchedDependencies,
    virtualStoreType: 'global', storeDir: config.storeDir && path.resolve(repo, config.storeDir),
    virtualStoreDir: undefined, globalVirtualStoreDir: config.globalVirtualStoreDir && path.resolve(repo, config.globalVirtualStoreDir),
    ignoreScripts: true, ignorePnpmfile: true,
    nodeLinker: 'isolated', hoistPattern: [], publicHoistPattern: [],
  }
}

function rejectWorkspaceLinks (lockfile) {
  for (const [id, pkg] of Object.entries(lockfile.packages ?? {})) {
    if (id.startsWith('file:') || pkg.resolution?.type === 'directory') {
      throw new Error(`Opt-out graph contains a local package: ${id}`)
    }
    for (const target of Object.values({ ...pkg.dependencies, ...pkg.optionalDependencies })) {
      if (target.startsWith('link:') || target.startsWith('file:')) {
        throw new Error(`Opt-out graph contains a local dependency: ${id} -> ${target}`)
      }
    }
  }
}

function mapInstalledClosure (manifest, roots, { source, locations }) {
  const materialized = new Map()
  const unlockedDependencies = []
  const skippedOptionalDependencies = []
  const pending = [...roots]
  while (pending.length > 0) {
    const { id, directory } = pending.pop()
    if (!directory) throw new Error(`Missing installed GVS package for ${id}`)
    if (materialized.has(id)) {
      if (materialized.get(id) !== directory) throw new Error(`Conflicting GVS contexts for ${id}`)
      continue
    }
    const pkg = manifest.packages[id]
    if (!pkg.files) throw new Error(`Opt-out graph reaches workspace package ${id}`)
    materialized.set(id, directory)
    const snapshot = locations.get(source.packages[id])?.pkgSnapshot
    for (const [name, target] of Object.entries(pkg.dependencies)) {
      if (target === id) continue
      if (!Object.hasOwn(snapshot?.dependencies ?? {}, name) && !Object.hasOwn(snapshot?.optionalDependencies ?? {}, name)) {
        unlockedDependencies.push({ issuer: id, name, target })
        continue
      }
      const dependencyRoot = findDependency(directory, name)
      if (!dependencyRoot && Object.hasOwn(snapshot.optionalDependencies ?? {}, name)) {
        skippedOptionalDependencies.push({ issuer: id, name, target })
        continue
      }
      pending.push({ id: target, directory: dependencyRoot })
    }
  }
  return { materialized: Object.fromEntries(materialized), unlockedDependencies, skippedOptionalDependencies }
}
