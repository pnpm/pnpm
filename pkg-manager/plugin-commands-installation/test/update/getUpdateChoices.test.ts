import chalk from 'chalk'
import { getUpdateChoices } from '../../lib/update/getUpdateChoices.js'

test('getUpdateChoices()', () => {
  expect(
    getUpdateChoices([
      {
        alias: 'foo',
        belongsTo: 'dependencies' as const,
        current: '1.0.0',
        latestManifest: {
          name: 'foo',
          version: '2.0.0',
          homepage: 'https://pnpm.io/',
        },
        packageName: 'foo',
        wanted: '1.0.0',
      },
      {
        alias: 'foo',
        belongsTo: 'devDependencies' as const,
        current: '1.0.0',
        latestManifest: {
          name: 'foo',
          version: '2.0.0',
          repository: {
            url: 'git://github.com/pnpm/pnpm.git',
          },
        },
        packageName: 'foo',
        wanted: '1.0.0',
      },
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
    ], false))
    .toStrictEqual([
      {
        name: '[dependencies]',
        message: 'dependencies',
        choices: [
          {
            name: 'Package                                                    Current   Target            URL              ',
            disabled: true,
            hint: '',
            value: '',
          },
          {
            message: `foo                                                          1.0.0 ❯ ${chalk.redBright.bold('2.0.0')}             https://pnpm.io/ `,
            value: 'foo',
            name: 'foo',
          },
        ],
      },
      {
        name: '[devDependencies]',
        message: 'devDependencies',
        choices: [
          {
            name: 'Package                                                    Current   Target            URL ',
            disabled: true,
            hint: '',
            value: '',
          },
          {
            message: `qar                                                          1.0.0 ❯ 1.${chalk.yellowBright.bold('2.0')}                 `,
            name: 'qar',
            value: 'qar',
          },
          {
            message: `zoo                                                          1.1.0 ❯ 1.${chalk.yellowBright.bold('2.0')}                 `,
            name: 'zoo',
            value: 'zoo',
          },
          {
            message: `foo                                                          1.0.1 ❯ 1.${chalk.yellowBright.bold('2.0')}                 `,
            name: 'foo',
            value: 'foo',
          },
        ],
      },
      {
        name: '[optionalDependencies]',
        message: 'optionalDependencies',
        choices: [
          {
            name: 'Package                                                    Current   Target            URL ',
            disabled: true,
            hint: '',
            value: '',
          },
          {
            message: `qaz                                                          1.0.1 ❯ 1.${chalk.yellowBright.bold('2.0')}                 `,
            name: 'qaz',
            value: 'qaz',
          },
        ],
      },
    ])
})

test('getUpdateChoices() with provenance column', () => {
  expect(
    getUpdateChoices([
      {
        alias: 'foo',
        belongsTo: 'dependencies' as const,
        current: '1.0.0',
        latestManifest: {
          name: 'foo',
          version: '2.0.0',
          homepage: 'https://pnpm.io/',
        },
        packageName: 'foo',
        wanted: '1.0.0',
      },
      {
        alias: 'qax',
        belongsTo: 'devDependencies' as const,
        current: '1.0.1',
        currentManifest: {
          name: 'qax',
          version: '1.0.1',
          _npmUser: {
            name: 'test-publisher',
            email: 'publisher@example.com',
            trustedPublisher: {
              id: 'test-provider',
              oidcConfigId: 'oidc:test-config-123',
            },
          },
          dist: {
            attestations: {
              provenance: {
                predicateType: 'https://slsa.dev/provenance/v1',
              },
            },
            shasum: 'abc123',
            tarball: 'https://registry.npmjs.org/qax/-/qax-1.0.1.tgz',
          },
        },
        latestManifest: {
          name: 'qax',
          version: '1.2.0',
          dist: {
            attestations: {
              provenance: {
                predicateType: 'https://slsa.dev/provenance/v1',
              },
            },
            shasum: 'def456',
            tarball: 'https://registry.npmjs.org/qax/-/qax-1.2.0.tgz',
          },
        },
        packageName: 'qax',
        wanted: '1.0.1',
      },
      {
        alias: 'qac',
        belongsTo: 'devDependencies' as const,
        current: '1.0.1',
        currentManifest: {
          name: 'qac',
          version: '1.0.1',
          dist: {
            attestations: {
              provenance: {
                predicateType: 'https://slsa.dev/provenance/v1',
              },
            },
            shasum: 'ghi789',
            tarball: 'https://registry.npmjs.org/qac/-/qac-1.0.1.tgz',
          },
        },
        latestManifest: {
          name: 'qac',
          version: '1.2.0',
        },
        packageName: 'qac',
        wanted: '1.0.1',
      },
      {
        alias: 'qar',
        belongsTo: 'devDependencies' as const,
        current: '1.0.0',
        currentManifest: {
          name: 'qar',
          version: '1.0.0',
          _npmUser: {
            name: 'test-publisher',
            email: 'publisher@example.com',
            trustedPublisher: {
              id: 'test-provider',
              oidcConfigId: 'oidc:test-config-123',
            },
          },
          dist: {
            attestations: {
              provenance: {
                predicateType: 'https://slsa.dev/provenance/v1',
              },
            },
            shasum: 'jkl012',
            tarball: 'https://registry.npmjs.org/qar/-/qar-1.0.0.tgz',
          },
        },
        latestManifest: {
          name: 'qar',
          version: '1.2.0',
          _npmUser: {
            name: 'test-publisher',
            email: 'publisher@example.com',
            trustedPublisher: {
              id: 'test-provider',
              oidcConfigId: 'oidc:test-config-123',
            },
          },
          dist: {
            attestations: {
              provenance: {
                predicateType: 'https://slsa.dev/provenance/v1',
              },
            },
            shasum: 'mno345',
            tarball: 'https://registry.npmjs.org/qar/-/qar-1.2.0.tgz',
          },
        },
        packageName: 'qar',
        wanted: '1.0.0',
      },
      {
        alias: 'zoo',
        belongsTo: 'devDependencies' as const,
        current: '1.1.0',
        currentManifest: {
          name: 'zoo',
          version: '1.1.0',
        },
        latestManifest: {
          name: 'zoo',
          version: '1.2.0',
          dist: {
            attestations: {
              provenance: {
                predicateType: 'https://slsa.dev/provenance/v1',
              },
            },
            shasum: 'pqr678',
            tarball: 'https://registry.npmjs.org/zoo/-/zoo-1.2.0.tgz',
          },
        },
        packageName: 'zoo',
        wanted: '1.1.0',
      },
      {
        alias: 'qaz',
        belongsTo: 'optionalDependencies' as const,
        current: '1.0.1',
        currentManifest: {
          name: 'qaz',
          version: '1.0.1',
          dist: {
            attestations: {
              provenance: {
                predicateType: 'https://slsa.dev/provenance/v1',
              },
            },
            shasum: 'stu901',
            tarball: 'https://registry.npmjs.org/qaz/-/qaz-1.0.1.tgz',
          },
        },
        latestManifest: {
          name: 'qaz',
          version: '1.2.0',
          _npmUser: {
            name: 'test-publisher',
            email: 'publisher@example.com',
            trustedPublisher: {
              id: 'test-provider',
              oidcConfigId: 'oidc:test-config-123',
            },
          },
          dist: {
            attestations: {
              provenance: {
                predicateType: 'https://slsa.dev/provenance/v1',
              },
            },
            shasum: 'vwx234',
            tarball: 'https://registry.npmjs.org/qaz/-/qaz-1.2.0.tgz',
          },
        },
        packageName: 'qaz',
        wanted: '1.0.1',
      },
    ], false, 'no-downgrade'))
    .toStrictEqual([
      {
        name: '[dependencies]',
        message: 'dependencies',
        choices: [
          {
            name: 'Package                                                    Current   Target            URL                Provenance ',
            disabled: true,
            hint: '',
            value: '',
          },
          {
            message: `foo                                                          1.0.0 ❯ ${chalk.redBright.bold('2.0.0')}             https://pnpm.io/   none       `,
            value: 'foo',
            name: 'foo',
          },
        ],
      },
      {
        name: '[devDependencies]',
        message: 'devDependencies',
        choices: [
          {
            name: 'Package                                                    Current   Target            URL   Provenance       ',
            disabled: true,
            hint: '',
            value: '',
          },
          {
            message: `qax                                                          1.0.1 ❯ 1.${chalk.yellowBright.bold('2.0')}                   ${chalk.red('provenance')}       `,
            name: 'qax',
            value: 'qax',
          },
          {
            message: `qac                                                          1.0.1 ❯ 1.${chalk.yellowBright.bold('2.0')}                   ${chalk.red('none')}             `,
            name: 'qac',
            value: 'qac',
          },
          {
            message: `qar                                                          1.0.0 ❯ 1.${chalk.yellowBright.bold('2.0')}                   ${chalk.green('trustedPublisher')} `,
            name: 'qar',
            value: 'qar',
          },
          {
            message: `zoo                                                          1.1.0 ❯ 1.${chalk.yellowBright.bold('2.0')}                   ${chalk.green('provenance')}       `,
            name: 'zoo',
            value: 'zoo',
          },
        ],
      },
      {
        name: '[optionalDependencies]',
        message: 'optionalDependencies',
        choices: [
          {
            name: 'Package                                                    Current   Target            URL   Provenance       ',
            disabled: true,
            hint: '',
            value: '',
          },
          {
            message: `qaz                                                          1.0.1 ❯ 1.${chalk.yellowBright.bold('2.0')}                   ${chalk.green('trustedPublisher')} `,
            name: 'qaz',
            value: 'qaz',
          },
        ],
      },
    ])
})
