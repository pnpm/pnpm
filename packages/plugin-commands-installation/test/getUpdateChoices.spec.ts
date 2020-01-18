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
            alias: 'qar',
            belongsTo: 'devDependencies' as const,
            current: '1.0.0',
            latestManifest: {
              name: 'qar',
              version: '1.2.0',
            },
            packageName: 'qar',
            wanted: '1.0.0',
          },
        ],
        prefix: 'project3',
      },
      {
        manifest: {},
        outdatedPackages: [
          {
            alias: 'zoo',
            belongsTo: 'devDependencies' as const,
            current: '1.1.0',
            latestManifest: {
              name: 'zoo',
              version: '1.2.0',
            },
            packageName: 'zoo',
            wanted: '1.1.0',
          },
        ],
        prefix: 'project4',
      },
      {
        manifest: {},
        outdatedPackages: [
          {
            alias: 'qaz',
            belongsTo: 'optionalDependencies' as const,
            current: '1.0.1',
            latestManifest: {
              name: 'qaz',
              version: '1.2.0',
            },
            packageName: 'qaz',
            wanted: '1.0.1',
          },
        ],
        prefix: 'project5',
      },
      {
        manifest: {},
        outdatedPackages: [
          {
            alias: 'qaz',
            belongsTo: 'devDependencies' as const,
            current: '1.0.1',
            latestManifest: {
              name: 'qaz',
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
        message: 'foo            1.0.0 ❯ 2.0.0 \n    foo (dev)      1.0.1 ❯ 1.2.0 ',
        name: 'foo',
      },
      {
        message: 'qar (dev)      1.0.0 ❯ 1.2.0 ',
        name: 'qar',
      },
      {
        message: 'qaz (optional) 1.0.1 ❯ 1.2.0 ',
        name: 'qaz',
      },
      {
        message: 'zoo (dev)      1.1.0 ❯ 1.2.0 ',
        name: 'zoo',
      },
    ],
  )
  t.end()
})
