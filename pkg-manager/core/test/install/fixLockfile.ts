import path from 'path'
import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { install, MutatedProject, mutateModules } from '@pnpm/core'
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
        //   integrity: 'sha512-WBv5F9HrWTyG800cB9M3veCVkFahqXN7KA7c3VUCYZm/xhNzzIFiXiq+rZmj75j7GvWelN3YNrLX7FjtqBvhMw==',
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
    integrity: 'sha512-WBv5F9HrWTyG800cB9M3veCVkFahqXN7KA7c3VUCYZm/xhNzzIFiXiq+rZmj75j7GvWelN3YNrLX7FjtqBvhMw==',
  })
  expect(lockfile.packages?.['/core-js-pure/3.16.2']).toBeTruthy()
  expect(lockfile.packages?.['/core-js-pure/3.16.2']?.resolution).toEqual({
    integrity: 'sha512-oxKe64UH049mJqrKkynWp6Vu0Rlm/BTXO/bJZuN2mmR3RtOFNepLlSWDd1eo16PzHpQAoNG97rLU1V/YxesJjw==',
  })
  expect(lockfile.packages?.['/core-js-pure/3.16.2']?.requiresBuild).toBeTruthy()
  expect(lockfile.packages?.['/core-js-pure/3.16.2']?.dev).toBeTruthy()
})

test('--fix-lockfile should preserve all locked dependencies version', async () => {
  preparePackages([
    {
      location: '.',
      package: { name: 'root' },
    },
    {
      location: 'project-1',
      package: { name: 'project-1', dependencies: { '@babel/runtime-corejs3': '7.15.3' } },
    },
    {
      location: 'project-2',
      package: { name: 'project-2', dependencies: { '@babel/runtime-corejs3': '7.15.4' } },
    },
  ])

  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('.'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ]

  /**
   * project-1 depends on @babel/runtime-corejs3@7.15.3 while project-2 depends on @babel/runtime-corejs3@7.15.4,
   * and @babel/runtime-corejs3@7.15.3 depends on core-js-pure@3.17.2 while @babel/runtime-corejs3@7.15.4 depends on core-js-pure@3.17.3
   * --fix-lockfile should not change the locked dependency version and only adding missing fields in this scene
   */
  await writeYamlFile(WANTED_LOCKFILE, {
    lockfileVersion: 5.3,
    importers: {
      '.': {
        specifiers: {},
      },
      'project-1': {
        specifiers: {
          '@babel/runtime-corejs3': '7.15.3',
        },
        dependencies: {
          '@babel/runtime-corejs3': '7.15.3',
        },
      },
      'project-2': {
        specifiers: {
          '@babel/runtime-corejs3': '7.15.4',
        },
        dependencies: {
          '@babel/runtime-corejs3': '7.15.4',
        },
      },
    },
    packages: {
      '/@babel/runtime-corejs3/7.15.3': {
        resolution: { integrity: 'sha512-30A3lP+sRL6ml8uhoJSs+8jwpKzbw8CqBvDc1laeptxPm5FahumJxirigcbD2qTs71Sonvj1cyZB0OKGAmxQ+A==' },
        // engines: { node: '>=6.9.0' },
        dependencies: {
          'core-js-pure': '3.17.2',
          'regenerator-runtime': '0.13.9',
        },
        dev: false,
      },
      '/@babel/runtime-corejs3/7.15.4': {
        resolution: { integrity: 'sha512-lWcAqKeB624/twtTc3w6w/2o9RqJPaNBhPGK6DKLSiwuVWC7WFkypWyNg+CpZoyJH0jVzv1uMtXZ/5/lQOLtCg==' },
        engines: { node: '>=6.9.0' },
        dependencies: {
          'core-js-pure': '3.17.3',
          'regenerator-runtime': '0.13.9',
        },
        dev: false,
      },
      '/core-js-pure/3.17.2': {
        resolution: { integrity: 'sha512-2VV7DlIbooyTI7Bh+yzOOWL9tGwLnQKHno7qATE+fqZzDKYr6llVjVQOzpD/QLZFgXDPb8T71pJokHEZHEYJhQ==' },
        // requiresBuild: true,
        dev: false,
      },
      '/core-js-pure/3.17.3': {
      // resolution: { integrity: 'sha512-YusrqwiOTTn8058JDa0cv9unbXdIiIgcgI9gXso0ey4WgkFLd3lYlV9rp9n7nDCsYxXsMDTjA4m1h3T348mdlQ==' },
      // requiresBuild: true,
      // dev: false
      },
      '/regenerator-runtime/0.13.9': {
        resolution: { integrity: 'sha512-p3VT+cOEgxFsRRA9X4lkI1E+k2/CtnKtU4gcxyaCUreilL/vqI6CdZ3wxVUx3UOUg+gnUOQQcRI7BmSI656MYA==' },
      // dev: false
      },
    },
  }, { lineWidth: 1000 })

  await mutateModules(importers, await testDefaults({
    fixLockfile: true,
    lockfileOnly: true,
    allProjects: [
      {
        buildIndex: 0,
        manifest: {
          name: 'root',
          version: '1.0.0',
        },
        rootDir: path.resolve('.'),
      },
      {
        buildIndex: 0,
        manifest: {
          name: 'project-1',
          version: '1.0.0',
          dependencies: {
            '@babel/runtime-corejs3': '7.15.3',
          },
        },
        rootDir: path.resolve('project-1'),
      },
      {
        buildIndex: 0,
        manifest: {
          name: 'project-3',
          version: '1.0.0',
          dependencies: {
            '@babel/runtime-corejs3': '7.15.4',
          },
        },
        rootDir: path.resolve('project-2'),
      },
    ],
  }))

  const lockfile: Lockfile = await readYamlFile(WANTED_LOCKFILE)

  expect(Object.keys(lockfile.packages as PackageSnapshots).length).toBe(5)

  expect(lockfile.packages?.['/@babel/runtime-corejs3/7.15.3']).toBeTruthy()
  expect(lockfile.packages?.['/@babel/runtime-corejs3/7.15.3']?.resolution).toEqual({
    integrity: 'sha512-30A3lP+sRL6ml8uhoJSs+8jwpKzbw8CqBvDc1laeptxPm5FahumJxirigcbD2qTs71Sonvj1cyZB0OKGAmxQ+A==',
  })
  expect(lockfile.packages?.['/@babel/runtime-corejs3/7.15.3']?.engines).toEqual({
    node: '>=6.9.0',
  })
  expect(lockfile.packages?.['/@babel/runtime-corejs3/7.15.3']?.dev).toBeFalsy()

  expect(lockfile.packages?.['/@babel/runtime-corejs3/7.15.4']).toBeTruthy()
  expect(lockfile.packages?.['/@babel/runtime-corejs3/7.15.4']?.resolution).toEqual({
    integrity: 'sha512-lWcAqKeB624/twtTc3w6w/2o9RqJPaNBhPGK6DKLSiwuVWC7WFkypWyNg+CpZoyJH0jVzv1uMtXZ/5/lQOLtCg==',
  })
  expect(lockfile.packages?.['/@babel/runtime-corejs3/7.15.4']?.engines).toEqual({
    node: '>=6.9.0',
  })
  expect(lockfile.packages?.['/@babel/runtime-corejs3/7.15.4']?.dev).toBeFalsy()

  expect(lockfile.packages?.['/core-js-pure/3.17.2']).toBeTruthy()
  expect(lockfile.packages?.['/core-js-pure/3.17.2']?.resolution).toHaveProperty('integrity', 'sha512-2VV7DlIbooyTI7Bh+yzOOWL9tGwLnQKHno7qATE+fqZzDKYr6llVjVQOzpD/QLZFgXDPb8T71pJokHEZHEYJhQ==')
  expect(lockfile.packages?.['/core-js-pure/3.17.2']?.requiresBuild).toBeTruthy()
  expect(lockfile.packages?.['/core-js-pure/3.17.2']?.dev).toBeFalsy()

  expect(lockfile.packages?.['/core-js-pure/3.17.3']).toBeTruthy()
  expect(lockfile.packages?.['/core-js-pure/3.17.3']?.resolution).toEqual({
    integrity: 'sha512-YusrqwiOTTn8058JDa0cv9unbXdIiIgcgI9gXso0ey4WgkFLd3lYlV9rp9n7nDCsYxXsMDTjA4m1h3T348mdlQ==',
  })
  expect(lockfile.packages?.['/core-js-pure/3.17.3']?.requiresBuild).toBeTruthy()
  expect(lockfile.packages?.['/core-js-pure/3.17.3']?.dev).toBeFalsy()

  expect(lockfile.packages?.['/regenerator-runtime/0.13.9']).toBeTruthy()
  expect(lockfile.packages?.['/regenerator-runtime/0.13.9']?.resolution).toEqual({
    integrity: 'sha512-p3VT+cOEgxFsRRA9X4lkI1E+k2/CtnKtU4gcxyaCUreilL/vqI6CdZ3wxVUx3UOUg+gnUOQQcRI7BmSI656MYA==',
  })
  expect(lockfile.packages?.['/regenerator-runtime/0.13.9']?.dev).toBeFalsy()
})
