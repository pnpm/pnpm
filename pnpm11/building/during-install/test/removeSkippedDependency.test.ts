import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { expect, test } from '@jest/globals'

import type { DependenciesGraphNode } from '../lib/buildGraph.js'
import { removeSkippedOptionalDependency } from '../lib/removeSkippedDependency.js'

test('removeSkippedOptionalDependency() removes the package and the links to it', async () => {
  const lockfileDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-skipped-'))
  try {
    const pkgDir = path.join(lockfileDir, 'node_modules/.pnpm/foo@1.0.0/node_modules/foo')
    await fs.promises.mkdir(pkgDir, { recursive: true })
    const modulesDir = path.join(lockfileDir, 'node_modules')
    await fs.promises.symlink(pkgDir, path.join(modulesDir, 'foo'), 'junction')
    // A link whose target runs through a regular file must not abort the cleanup.
    await fs.promises.writeFile(path.join(lockfileDir, 'file'), '')
    await fs.promises.symlink(path.join(lockfileDir, 'file/child'), path.join(modulesDir, 'bar'), 'junction')

    await removeSkippedOptionalDependency(
      { depPath: 'foo@1.0.0', dir: pkgDir } as unknown as DependenciesGraphNode<string>,
      { lockfileDir, linkedModulesDirs: [modulesDir] }
    )

    expect(fs.existsSync(pkgDir)).toBe(false)
    expect(fs.lstatSync(path.join(modulesDir, 'foo'), { throwIfNoEntry: false })).toBeUndefined()
    expect(fs.lstatSync(path.join(modulesDir, 'bar'), { throwIfNoEntry: false })).toBeDefined()
  } finally {
    await fs.promises.rm(lockfileDir, { recursive: true, force: true })
  }
})
