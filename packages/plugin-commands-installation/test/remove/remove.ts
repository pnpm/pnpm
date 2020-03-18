import PnpmError from '@pnpm/error'
import { remove } from '@pnpm/plugin-commands-installation'
import prepare, { preparePackages } from '@pnpm/prepare'
import { oneLine } from 'common-tags'
import test = require('tape')
import { DEFAULT_OPTS } from '../utils'

test('remove should fail if no dependency is specified for removal', async (t) => {
  prepare(t)

  let err!: PnpmError
  try {
    await remove.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    }, [])
  } catch (_err) {
    err = _err
  }
  t.equal(err.code, 'ERR_PNPM_MUST_REMOVE_SOMETHING')
  t.equal(err.message, 'At least one dependency name should be specified for removal')
  t.end()
})

test('remove should fail if the project has no dependencies at all', async (t) => {
  prepare(t)

  let err!: PnpmError
  try {
    await remove.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    }, ['express'])
  } catch (_err) {
    err = _err
  }
  t.equal(err.code, 'ERR_PNPM_REMOVE_FROM_EMPTY_PROJECT')
  t.equal(err.message, 'There are no dependencies to remove from')
  t.end()
})

test('remove should fail if the project does not have one of the removed dependencies', async (t) => {
  prepare(t, {
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
    } catch (_err) {
      err = _err
    }
    t.equal(err.code, 'ERR_PNPM_PKG_TO_REMOVE_NOT_FOUND')
    t.equal(
      err.message,
      oneLine`
        Some of the dependencies specified for deletion are not present:
        dev-dep-1, optional-dep-1. Next dependencies may be removed from
        dependencies: prod-dep-1, prod-dep-2`,
    )
  }
  {
    let err!: PnpmError
    try {
      await remove.handler({
        ...DEFAULT_OPTS,
        dir: process.cwd(),
        saveDev: true,
      }, ['prod-dep-1', 'dev-dep-1', 'optional-dep-1'])
    } catch (_err) {
      err = _err
    }
    t.equal(err.code, 'ERR_PNPM_PKG_TO_REMOVE_NOT_FOUND')
    t.equal(
      err.message,
      oneLine`
        Some of the dependencies specified for deletion are not present:
        prod-dep-1, optional-dep-1. Next dependencies may be removed from
        devDependencies: dev-dep-1, dev-dep-2`,
    )
  }
  {
    let err!: PnpmError
    try {
      await remove.handler({
        ...DEFAULT_OPTS,
        dir: process.cwd(),
        saveOptional: true,
      }, ['prod-dep-1', 'dev-dep-1', 'optional-dep-1'])
    } catch (_err) {
      err = _err
    }
    t.equal(err.code, 'ERR_PNPM_PKG_TO_REMOVE_NOT_FOUND')
    t.equal(
      err.message,
      oneLine`
        Some of the dependencies specified for deletion are not present:
        prod-dep-1, dev-dep-1. Next dependencies may be removed from
        optionalDependencies: optional-dep-1, optional-dep-2`,
    )
  }
  {
    let err!: PnpmError
    try {
      await remove.handler({
        ...DEFAULT_OPTS,
        dir: process.cwd(),
      }, ['express', 'prod-dep-1', 'dev-dep-1', 'optional-dep-1'])
    } catch (_err) {
      err = _err
    }
    t.equal(err.code, 'ERR_PNPM_PKG_TO_REMOVE_NOT_FOUND')
    t.equal(
      err.message,
      oneLine`
        Some of the dependencies specified for deletion are not present:
        express. Next dependencies may be removed: dev-dep-1, dev-dep-2,
        prod-dep-1, prod-dep-2, optional-dep-1, optional-dep-2`,
    )
  }
  t.end()
})
