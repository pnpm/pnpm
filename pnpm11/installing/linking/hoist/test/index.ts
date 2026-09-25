import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { hoist } from '@pnpm/installing.linking.hoist'
import type { DepPath, ProjectId } from '@pnpm/types'
import { resolveLinkTarget } from 'resolve-link-target'
import { symlinkDir } from 'symlink-dir'

test('concurrent hoists can replace the same stale dependency link', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-concurrent-hoist-'))
  const modulesDir = path.join(root, 'node_modules')
  const virtualStoreDir = path.join(modulesDir, '.pnpm')
  const privateHoistedModulesDir = path.join(virtualStoreDir, 'node_modules')
  const oldTarget = path.join(virtualStoreDir, 'dep@1.0.0/node_modules/dep')
  const target = path.join(virtualStoreDir, 'dep@2.0.0/node_modules/dep')
  fs.mkdirSync(oldTarget, { recursive: true })
  fs.mkdirSync(target, { recursive: true })
  fs.writeFileSync(path.join(target, 'index.js'), 'module.exports = 2')
  const link = path.join(privateHoistedModulesDir, 'dep')
  await symlinkDir(oldTarget, link)

  const opts = {
    graph: {
      parent: {
        dir: path.join(virtualStoreDir, 'parent@1.0.0/node_modules/parent'),
        children: { dep: 'dep' },
        optionalDependencies: new Set<string>(),
        hasBin: false,
        name: 'parent',
        depPath: 'parent@1.0.0' as DepPath,
      },
      dep: {
        dir: target,
        children: {},
        optionalDependencies: new Set<string>(),
        hasBin: false,
        name: 'dep',
        depPath: 'dep@2.0.0' as DepPath,
      },
    },
    directDepsByImporterId: { ['.' as ProjectId]: new Map([['parent', 'parent']]) },
    skipped: new Set<DepPath>(),
    privateHoistPattern: ['*'],
    publicHoistPattern: [],
    privateHoistedModulesDir,
    publicHoistedModulesDir: modulesDir,
    virtualStoreDir,
    virtualStoreDirMaxLength: 120,
  }
  try {
    const results = await Promise.allSettled(Array.from({ length: 32 }, () => hoist(opts)))
    expect(results.filter(result => result.status === 'rejected')).toStrictEqual([])
    expect(await resolveLinkTarget(link)).toBe(target)
    expect(fs.readFileSync(path.join(link, 'index.js'), 'utf8')).toBe('module.exports = 2')
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})
