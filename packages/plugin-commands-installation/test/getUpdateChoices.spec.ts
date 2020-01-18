import test = require('tape')
import getUpdateChoices from '../lib/getUpdateChoices'

test('getUpdateChoices()', (t) => {
  t.deepEqual(
    getUpdateChoices([
      {
        manifest: {},
        outdatedPackages: [
          {
            alias: 'foo',
            belongsTo: 'dependencies' as const,
            current: '1.0.0',
            latestManifest: {
              name: 'foo',
              version: '2.0.0',
            },
            packageName: 'foo',
            wanted: '1.0.0',
          },
        ],
        prefix: 'project1',
      },
      {
        manifest: {},
        outdatedPackages: [
          {
            alias: 'foo',
            belongsTo: 'devDependencies' as const,
            current: '1.0.0',
            latestManifest: {
              name: 'foo',
              version: '2.0.0',
            },
            packageName: 'foo',
            wanted: '1.0.0',
          },
        ],
        prefix: 'project2',
      },
      {
        manifest: {},
        outdatedPackages: [
          {
            alias: 'foo',
            belongsTo: 'devDependencies' as const,
            current: '1.0.0',
            latestManifest: {
              name: 'foo',
              version: '1.2.0',
            },
            packageName: 'foo',
            wanted: '1.0.0',
          },
        ],
        prefix: 'project3',
      },
      {
        manifest: {},
        outdatedPackages: [
          {
            alias: 'foo',
            belongsTo: 'devDependencies' as const,
            current: '1.1.0',
            latestManifest: {
              name: 'foo',
              version: '1.2.0',
            },
            packageName: 'foo',
            wanted: '1.1.0',
          },
        ],
        prefix: 'project4',
      },
      {
        manifest: {},
        outdatedPackages: [
          {
            alias: 'foo',
            belongsTo: 'optionalDependencies' as const,
            current: '1.0.1',
            latestManifest: {
              name: 'foo',
              version: '1.2.0',
            },
            packageName: 'foo',
            wanted: '1.0.1',
          },
        ],
        prefix: 'project5',
      },
      {
        manifest: {},
        outdatedPackages: [
          {
            alias: 'foo',
            belongsTo: 'devDependencies' as const,
            current: '1.0.1',
            latestManifest: {
              name: 'foo',
              version: '1.2.0',
            },
            packageName: 'foo',
            wanted: '1.0.1',
          },
        ],
        prefix: 'project6',
      },
    ]),
    [
      {
        choices: [
          {
            message: 'foo 1.0.0 ❯ 2.0.0',
            name: 'foo',
          },
        ],
        name: 'dependencies',
      },
      {
        choices: [
          {
            message: 'foo 1.0.1 ❯ 1.2.0',
            name: 'foo',
          },
        ],
        name: 'optionalDependencies',
      },
      {
        choices: [
          {
            message: 'foo 1.0.0 ❯ 1.2.0\n    foo 1.1.0 ❯ 1.2.0',
            name: 'foo',
          },
        ],
        name: 'devDependencies',
      },
    ],
  )
  t.end()
})
