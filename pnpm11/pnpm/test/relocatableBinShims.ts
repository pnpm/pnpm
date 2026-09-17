import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { tempDir } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from './utils/index.js'

const testOnPosix = process.platform === 'win32' ? test.skip : test

testOnPosix.each(['isolated', 'hoisted'])('workspace bin shims resolve their node paths after the project is moved (%s)', async (nodeLinker) => {
  tempDir()
  const container = process.cwd()
  const original = path.join(container, 'original project')
  const moved = path.join(container, 'moved project')
  fs.mkdirSync(path.join(original, 'tool'), { recursive: true })
  fs.mkdirSync(path.join(original, 'consumer'))
  fs.writeFileSync(path.join(original, 'package.json'), JSON.stringify({ private: true }))
  fs.writeFileSync(path.join(original, 'tool/package.json'), JSON.stringify({
    name: 'relocatable-tool',
    version: '1.0.0',
    bin: { 'relocatable-tool': 'bin.cjs' },
  }))
  fs.writeFileSync(path.join(original, 'tool/bin.cjs'), '#!/usr/bin/env node\nconsole.log(process.env.NODE_PATH)\n')
  fs.writeFileSync(path.join(original, 'consumer/package.json'), JSON.stringify({
    name: 'consumer',
    version: '1.0.0',
    dependencies: { 'relocatable-tool': 'workspace:*' },
  }))
  writeYamlFileSync(path.join(original, 'pnpm-workspace.yaml'), {
    packages: ['tool', 'consumer'],
    nodeLinker,
    preferSymlinkedExecutables: false,
    injectWorkspacePackages: true,
    dedupeInjectedDeps: false,
  })
  process.chdir(original)
  try {
    await execPnpm(['install', '--offline'])
  } finally {
    process.chdir(container)
  }
  fs.renameSync(original, moved)
  const alias = path.join(container, 'alias')
  fs.symlinkSync(moved, alias, 'dir')
  for (const root of [moved, alias]) {
    const binPath = nodeLinker === 'hoisted' ? 'node_modules/.bin/relocatable-tool' : 'consumer/node_modules/.bin/relocatable-tool'
    expect(fs.readFileSync(path.join(root, binPath), 'utf8')).not.toContain(original)
    const result = spawnSync(path.join(root, binPath), {
      env: { ...process.env, NODE_PATH: '' },
      encoding: 'utf8',
    })
    expect(result.error).toBeUndefined()
    expect(result.status).toBe(0)
    const nodePaths = result.stdout.trim().split(path.delimiter).filter(Boolean).map(entry => path.resolve(entry))
    if (nodeLinker === 'isolated') {
      expect(nodePaths.length).toBeGreaterThan(0)
    } else {
      expect(nodePaths).toEqual([])
    }
    for (const nodePath of nodePaths) {
      expect(nodePath.startsWith(`${moved}${path.sep}`)).toBe(true)
    }
  }
})
