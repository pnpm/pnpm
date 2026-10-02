import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { tempDir } from '@pnpm/prepare'
import type { DepPath, ProjectId, ProjectRootDir } from '@pnpm/types'

import { prepareBinPaths, runBuild } from '../../after-install/src/buildSinglePackage.js'
import { relinkBins } from '../../after-install/src/rebuildGraph.js'
import type { RebuildState } from '../../after-install/src/rebuildTypes.js'
import { relinkHoistedPackageBins } from '../../after-install/src/relinkHoistedPackageBins.js'

const toolDepPath = 'tool@1.0.0' as DepPath
const parentDepPath = 'parent@1.0.0' as DepPath
const rivalDepPath = 'rival@1.0.0' as DepPath

const testOnUnix = process.platform === 'win32' ? test.skip : test

testOnUnix('post-build relinking refreshes changed interpreters in project and dependency bins', async () => {
  const { modules, parent, executable, state } = fixture()
  await relinkBins(state, () => modules)
  fs.writeFileSync(executable, '#!/bin/sh\nprintf "built binary\\n"\n')
  state.builtDepPaths.add(toolDepPath)

  await prepareBinPaths({ depPath: parentDepPath, pkgRoot: parent }, state)
  const parentBuild = spawnSync(path.join(parent, 'node_modules/.bin/tool'), { encoding: 'utf8' })
  expect(parentBuild.status).toBe(0)
  expect(parentBuild.stdout).toBe('built binary\n')

  await relinkBins(state, () => modules)

  for (const bin of [path.join(modules, '.bin/tool'), path.join(parent, 'node_modules/.bin/tool')]) {
    const result = spawnSync(bin, { encoding: 'utf8' })
    expect(result.stderr).toBe('')
    expect(result.status).toBe(0)
    expect(result.stdout).toBe('built binary\n')
  }
})

testOnUnix('hoisted completed builds refresh shared launchers without rewriting siblings or collision winners', async () => {
  const { modules, parent, executable, state } = fixture()
  state.opts.nodeLinker = 'hoisted'
  state.opts.sideEffectsCacheWrite = false
  const tool = path.dirname(executable)
  const rival = path.join(modules, 'rival')
  const stable = path.join(modules, 'stable')
  for (const [pkgRoot, name, bin] of [[tool, 'tool', 'tool'], [rival, 'rival', 'tool'], [stable, 'stable', 'stable']]) {
    fs.mkdirSync(pkgRoot, { recursive: true })
    fs.writeFileSync(path.join(pkgRoot, 'package.json'), JSON.stringify({
      name, version: '1.0.0', bin: { [bin]: 'tool' }, scripts: { postinstall: 'node build.cjs' },
    }))
    fs.writeFileSync(path.join(pkgRoot, 'tool'), '#!/usr/bin/env node\nconsole.log("placeholder")\n', { mode: 0o755 })
    fs.writeFileSync(path.join(pkgRoot, 'build.cjs'), `require('fs').writeFileSync('tool', ${JSON.stringify('#!/bin/sh\nprintf "built binary\\n"\n')})`)
  }
  state.pkgSnapshots[toolDepPath].resolution = { type: 'directory', directory: '../tool' }
  state.pkgSnapshots[rivalDepPath] = { resolution: { type: 'directory', directory: '../rival' } }
  state.ctx.modulesFile = modulesManifest(modules)
  await relinkBins({ ...state, pkgSnapshots: {} }, () => modules)
  const stableBin = path.join(modules, '.bin/stable')
  fs.utimesSync(stableBin, 100, 100)
  await Promise.all([runBuild(toolDepPath, state), runBuild(rivalDepPath, state)])
  expect(state.builtDepPaths.has('tool@1.0.0')).toBe(true)
  const toolBin = path.join(modules, '.bin/tool')
  fs.utimesSync(toolBin, 100, 100)
  await runBuild(rivalDepPath, state)
  expect(fs.statSync(toolBin).mtimeMs).toBe(100000)
  expect(fs.statSync(stableBin).mtimeMs).toBe(100000)
  await relinkBins({ ...state, pkgSnapshots: {} }, () => modules)
  expect(fs.statSync(stableBin).mtimeMs).toBe(100000)

  await Promise.all([parent, path.join(modules, 'second-parent')].map(async pkgRoot => {
    const binPaths = await prepareBinPaths({ depPath: parentDepPath, pkgRoot }, state)
    const result = spawnSync('tool', {
      encoding: 'utf8',
      env: { ...process.env, PATH: [...binPaths, process.env.PATH ?? ''].join(path.delimiter) },
    })
    expect(result.stderr).toBe('')
    expect(result.status).toBe(0)
    expect(result.stdout).toBe('built binary\n')
  }))
})

testOnUnix('final relinking refreshes publicly hoisted built transitive commands', async () => {
  const { modules, executable, state } = fixture()
  state.ctx.currentLockfile.importers = {}
  state.ctx.modulesFile = modulesManifest(modules)
  state.ctx.modulesFile.hoistedDependencies[toolDepPath] = { tool: 'public' }
  await relinkBins(state, () => modules)
  fs.writeFileSync(executable, '#!/bin/sh\nprintf "built binary\\n"\n')
  state.builtDepPaths.add(toolDepPath)
  await relinkBins(state, () => modules)
  const result = spawnSync(path.join(modules, '.bin/tool'), { encoding: 'utf8' })
  expect(result.stderr).toBe('')
  expect(result.status).toBe(0)
  expect(result.stdout).toBe('built binary\n')
})

testOnUnix('hoisted refresh plans keep duplicate placements in their own physical directories', async () => {
  const { modules, state } = fixture()
  const roots = [path.join(modules, 'tool'), path.join(modules, 'parent/node_modules/tool')]
  for (const [index, pkgRoot] of roots.entries()) {
    fs.mkdirSync(pkgRoot, { recursive: true })
    fs.writeFileSync(path.join(pkgRoot, 'package.json'), JSON.stringify({ name: 'tool', version: '1.0.0', bin: 'tool' }))
    fs.writeFileSync(path.join(pkgRoot, 'tool'), `#!/usr/bin/env node\nconsole.log(${index})\n`, { mode: 0o755 })
  }
  await relinkHoistedPackageBins(roots, state)
  for (const [index, pkgRoot] of roots.entries()) {
    const result = spawnSync(path.join(path.dirname(pkgRoot), '.bin/tool'), { encoding: 'utf8' })
    expect(result.status).toBe(0)
    expect(result.stdout).toBe(`${index}\n`)
  }
})

function fixture () {
  const root = tempDir(false)
  const modules = path.join(root, 'node_modules')
  const tool = path.join(modules, 'tool')
  const parent = path.join(modules, 'parent')
  fs.mkdirSync(tool, { recursive: true })
  fs.mkdirSync(parent, { recursive: true })
  fs.writeFileSync(path.join(tool, 'package.json'), JSON.stringify({ name: 'tool', version: '1.0.0', bin: 'tool' }))
  fs.writeFileSync(path.join(parent, 'package.json'), JSON.stringify({ name: 'parent', version: '1.0.0', dependencies: { tool: '1.0.0' } }))
  const executable = path.join(tool, 'tool')
  fs.writeFileSync(executable, '#!/usr/bin/env node\nconsole.log("placeholder")\n', { mode: 0o755 })
  const state: RebuildState = {
    pkgSnapshots: {
      [toolDepPath]: { resolution: { integrity: 'sha512-test' } },
      [parentDepPath]: { resolution: { integrity: 'sha512-test' }, dependencies: { tool: '1.0.0' } },
    },
    ctx: {
      projects: { '.': { id: '.' as ProjectId, rootDir: root as ProjectRootDir } },
      modulesFile: null,
      rootModulesDir: modules,
      virtualStoreDir: path.join(modules, '.pnpm'),
      extraBinPaths: [],
      extraNodePaths: [],
      pkgsToRebuild: new Set(),
      skipped: new Set(),
      currentLockfile: { lockfileVersion: '9.0', importers: { ['.' as ProjectId]: { specifiers: { tool: '1.0.0' }, dependencies: { tool: '1.0.0' } } } },
    },
    opts: { nodeLinker: 'isolated', lockfileDir: root, unsafePerm: true } as RebuildState['opts'],
    depGraph: {},
    depsStateCache: {},
    nodeVersion: undefined,
    ignoredPkgs: new Set(),
    storeIndex: undefined,
    builtDepPaths: new Set(),
    pkgsThatWereRebuilt: new Set(),
    allowBuild: () => true,
    gvsDirByDepPath: new Map([[parentDepPath, root]]),
    warn: () => {},
  }
  return { modules, parent, executable, state }
}

function modulesManifest (modules: string): NonNullable<RebuildState['ctx']['modulesFile']> {
  return {
    hoistedLocations: { [toolDepPath]: ['node_modules/tool'], [rivalDepPath]: ['node_modules/rival'] },
    hoistedDependencies: {},
    included: { dependencies: true, devDependencies: true, optionalDependencies: true },
    layoutVersion: 5,
    packageManager: 'pnpm@11.0.0',
    pendingBuilds: [],
    prunedAt: new Date(0).toUTCString(),
    skipped: [],
    storeDir: path.join(modules, '../store'),
    virtualStoreDir: path.join(modules, '.pnpm'),
    virtualStoreDirMaxLength: 120,
  }
}
