import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import { prepareEmpty } from '@pnpm/prepare'
import { install } from 'supi'
import writeYamlFile from 'write-yaml-file'
import readYamlFile from 'read-yaml-file'
import { Lockfile, PackageSnapshots } from '@pnpm/lockfile-file'
import { testDefaults } from '../utils'

test('fix broken lockfile with --fix-lockfile', async () => {
  prepareEmpty()

  await writeYamlFile(WANTED_LOCKFILE, {
    dependencies: {
      '@types/semver': '5.3.31',
    },
    devDependencies: {
      fsevents: '2.3.2',
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/@types/semver/5.3.31': {
        // resolution: {
        //   integrity: 'sha1-uZnX2TX0P1IHsBsA094ghS9Mp18=',
        // },
      },
      '/core-js-pure/3.16.2': {
        resolution: {
          integrity: 'sha512-oxKe64UH049mJqrKkynWp6Vu0Rlm/BTXO/bJZuN2mmR3RtOFNepLlSWDd1eo16PzHpQAoNG97rLU1V/YxesJjw==',
        },
        // requiresBuild: true,
        // dev: true
      },
    },
    specifiers: {
      '@types/semver': '^5.3.31',
      fsevents: '^2.3.2',
    },
  }, { lineWidth: 1000 })

  await install({
    dependencies: {
      '@types/semver': '^5.3.31',
    },
    devDependencies: {
      'core-js-pure': '^3.16.2',
    },
  }, await testDefaults({ fixLockfile: true }))

  const lockfile: Lockfile = await readYamlFile(WANTED_LOCKFILE)
  expect(Object.keys(lockfile.packages as PackageSnapshots).length).toBe(2)
  expect(lockfile.packages?.['/@types/semver/5.3.31']).toBeTruthy()
  expect(lockfile.packages?.['/@types/semver/5.3.31']?.resolution).toEqual({
    integrity: 'sha1-uZnX2TX0P1IHsBsA094ghS9Mp18=',
  })
  expect(lockfile.packages?.['/core-js-pure/3.16.2']).toBeTruthy()
  expect(lockfile.packages?.['/core-js-pure/3.16.2']?.resolution).toEqual({
    integrity: 'sha512-oxKe64UH049mJqrKkynWp6Vu0Rlm/BTXO/bJZuN2mmR3RtOFNepLlSWDd1eo16PzHpQAoNG97rLU1V/YxesJjw==',
  })
  expect(lockfile.packages?.['/core-js-pure/3.16.2']?.requiresBuild).toBeTruthy()
  expect(lockfile.packages?.['/core-js-pure/3.16.2']?.dev).toBeTruthy()
})