import PnpmError from '@pnpm/error'
import {
  createWorkspaceSpecs,
  updateToWorkspacePackagesFromManifest,
} from '@pnpm/plugin-commands-installation/lib/updateWorkspaceDependencies'
import test = require('tape')

const INCLUDE_ALL = {
  dependencies: true,
  devDependencies: true,
  optionalDependencies: true,
}

const WORKSPACE_PACKAGES = {
  bar: {
    '100.0.0': {
      dir: '',
      manifest: {
        name: 'foo',
        version: '100.0.0',
      },
    },
  },
  foo: {
    '100.0.0': {
      dir: '',
      manifest: {
        name: 'foo',
        version: '100.0.0',
      },
    },
  },
  qar: {
    '100.0.0': {
      dir: '',
      manifest: {
        name: 'foo',
        version: '100.0.0',
      },
    },
  },
}

test('updateToWorkspacePackagesFromManifest()', t => {
  const manifest = {
    dependencies: {
      alpha: '1.0.0',
      foo: '1.0.0',
    },
    devDependencies: {
      bar: '1.0.0',
      betta: '1.0.0',
    },
    optionalDependencies: {
      hamma: '1.0.0',
      qar: '1.0.0',
    },
  }
  t.deepEqual(updateToWorkspacePackagesFromManifest(
    manifest,
    INCLUDE_ALL,
    WORKSPACE_PACKAGES
  ), ['bar@workspace:*', 'foo@workspace:*', 'qar@workspace:*'])
  t.deepEqual(updateToWorkspacePackagesFromManifest(
    manifest,
    {
      dependencies: true,
      devDependencies: false,
      optionalDependencies: false,
    },
    WORKSPACE_PACKAGES
  ), ['foo@workspace:*'])
  t.end()
})

test('createWorkspaceSpecs', t => {
  t.deepEqual(createWorkspaceSpecs(['bar', 'foo@2', 'qar@workspace:3'], WORKSPACE_PACKAGES), ['bar@workspace:*', 'foo@workspace:2', 'qar@workspace:3'])
  let err!: PnpmError
  try {
    createWorkspaceSpecs(['express'], WORKSPACE_PACKAGES)
  } catch (_err) {
    err = _err
  }
  t.equal(err.code, 'ERR_PNPM_WORKSPACE_PACKAGE_NOT_FOUND')
  t.equal(err.message, '"express" not found in the workspace')
  t.end()
})
