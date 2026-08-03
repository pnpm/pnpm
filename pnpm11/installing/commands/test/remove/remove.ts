import { readFile } from 'node:fs/promises'

import { expect, test } from '@jest/globals'
import type { PnpmError } from '@pnpm/error'
import { remove } from '@pnpm/installing.commands'
import { prepare, preparePackages } from '@pnpm/prepare'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'

import { DEFAULT_OPTS } from '../utils/index.js'

test('remove should fail if no dependency is specified for removal', async () => {
  prepare()

  let err!: PnpmError
  try {
    await remove.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    }, [])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_MUST_REMOVE_SOMETHING')
  expect(err.message).toBe('At least one dependency name should be specified for removal')
})

test('remove should fail if the project has no dependencies at all', async () => {
  prepare()

  {
    let err!: PnpmError
    try {
      await remove.handler({
        ...DEFAULT_OPTS,
        dir: process.cwd(),
      }, ['express'])
    } catch (_err: any) { // eslint-disable-line
      err = _err
    }
    expect(err.code).toBe('ERR_PNPM_CANNOT_REMOVE_MISSING_DEPS')
    expect(err.message).toBe("Cannot remove 'express': project has no dependencies of any kind")
  }
  {
    let err!: PnpmError
    try {
      await remove.handler({
        ...DEFAULT_OPTS,
        dir: process.cwd(),
        saveProd: true,
      }, ['express'])
    } catch (_err: any) { // eslint-disable-line
      err = _err
    }
    expect(err.code).toBe('ERR_PNPM_CANNOT_REMOVE_MISSING_DEPS')
    expect(err.message).toBe("Cannot remove 'express': project has no 'dependencies'")
  }
  {
    let err!: PnpmError
    try {
      await remove.handler({
        ...DEFAULT_OPTS,
        dir: process.cwd(),
        saveDev: true,
      }, ['express'])
    } catch (_err: any) { // eslint-disable-line
      err = _err
    }
    expect(err.code).toBe('ERR_PNPM_CANNOT_REMOVE_MISSING_DEPS')
    expect(err.message).toBe("Cannot remove 'express': project has no 'devDependencies'")
  }
  {
    let err!: PnpmError
    try {
      await remove.handler({
        ...DEFAULT_OPTS,
        dir: process.cwd(),
        saveOptional: true,
      }, ['express'])
    } catch (_err: any) { // eslint-disable-line
      err = _err
    }
    expect(err.code).toBe('ERR_PNPM_CANNOT_REMOVE_MISSING_DEPS')
    expect(err.message).toBe("Cannot remove 'express': project has no 'optionalDependencies'")
  }
})

test('remove expands dependency glob patterns', async () => {
  prepare({
    dependencies: {
      'is-negative': '1.0.0',
      'is-positive': '1.0.0',
    },
  })

  await remove.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, ['is-neg*'])

  const manifest = JSON.parse(await readFile('package.json', 'utf8'))
  expect(manifest.dependencies).toStrictEqual({
    'is-positive': '1.0.0',
  })
})

test('recursive remove with dependency glob patterns respects the selected dependency field', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'is-negative': '1.0.0',
      },
      devDependencies: {
        'is-positive': '1.0.0',
      },
    },
  ])

  const { allProjects, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])

  await remove.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    saveProd: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['is-*'])

  const manifest = JSON.parse(await readFile('project-1/package.json', 'utf8'))
  expect(manifest.dependencies).toBeUndefined()
  expect(manifest.devDependencies).toStrictEqual({
    'is-positive': '1.0.0',
  })
})

test('remove should fail if the project does not have one of the removed dependencies', async () => {
  prepare({
    dependencies: {
      'prod-dep-1': '1.0.0',
      'prod-dep-2': '1.0.0',
    },
    devDependencies: {
      'dev-dep-1': '1.0.0',
      'dev-dep-2': '1.0.0',
    },
    optionalDependencies: {
      'optional-dep-1': '1.0.0',
      'optional-dep-2': '1.0.0',
    },
  })

  {
    let err!: PnpmError
    try {
      await remove.handler({
        ...DEFAULT_OPTS,
        dir: process.cwd(),
        saveProd: true,
      }, ['prod-dep-1', 'dev-dep-1', 'optional-dep-1'])
    } catch (_err: any) { // eslint-disable-line
      err = _err
    }
    expect(err.code).toBe('ERR_PNPM_CANNOT_REMOVE_MISSING_DEPS')
    expect(err.message).toBe('Cannot remove \'dev-dep-1\', \'optional-dep-1\': \
no such dependencies found in \'dependencies\'')
    expect(err.hint).toBe('Available dependencies: prod-dep-1, prod-dep-2')
  }
  {
    let err!: PnpmError
    try {
      await remove.handler({
        ...DEFAULT_OPTS,
        dir: process.cwd(),
        saveDev: true,
      }, ['prod-dep-1', 'dev-dep-1', 'optional-dep-1'])
    } catch (_err: any) { // eslint-disable-line
      err = _err
    }
    expect(err.code).toBe('ERR_PNPM_CANNOT_REMOVE_MISSING_DEPS')
    expect(err.message).toBe('Cannot remove \'prod-dep-1\', \'optional-dep-1\': \
no such dependencies found in \'devDependencies\'')
    expect(err.hint).toBe('Available dependencies: dev-dep-1, dev-dep-2')
  }
  {
    let err!: PnpmError
    try {
      await remove.handler({
        ...DEFAULT_OPTS,
        dir: process.cwd(),
        saveOptional: true,
      }, ['prod-dep-1', 'dev-dep-1', 'optional-dep-1'])
    } catch (_err: any) { // eslint-disable-line
      err = _err
    }
    expect(err.code).toBe('ERR_PNPM_CANNOT_REMOVE_MISSING_DEPS')
    expect(err.message).toBe('Cannot remove \'prod-dep-1\', \'dev-dep-1\': \
no such dependencies found in \'optionalDependencies\'')
    expect(err.hint).toBe('Available dependencies: optional-dep-1, optional-dep-2')
  }
  {
    let err!: PnpmError
    try {
      await remove.handler({
        ...DEFAULT_OPTS,
        dir: process.cwd(),
      }, ['express', 'prod-dep-1', 'dev-dep-1', 'optional-dep-1'])
    } catch (_err: any) { // eslint-disable-line
      err = _err
    }
    expect(err.code).toBe('ERR_PNPM_CANNOT_REMOVE_MISSING_DEPS')
    expect(err.message).toBe("Cannot remove 'express': no such dependency found")
    expect(err.hint).toBe('Available dependencies: dev-dep-1, dev-dep-2, \
prod-dep-1, prod-dep-2, optional-dep-1, optional-dep-2')
  }
})
