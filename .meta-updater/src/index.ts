import fs from 'fs'
import path from 'path'
import { readWantedLockfile, type LockfileObject } from '@pnpm/lockfile.fs'
import { type ProjectId, type ProjectManifest } from '@pnpm/types'
import { createUpdateOptions, type FormatPluginFnOptions } from '@pnpm/meta-updater'
import { sortDirectKeys, sortKeysByPriority } from '@pnpm/object.key-sorting'
import { findWorkspacePackagesNoCheck } from '@pnpm/workspace.find-packages'
import { readWorkspaceManifest } from '@pnpm/workspace.read-manifest'
import isSubdir from 'is-subdir'
import { loadJsonFileSync } from 'load-json-file'
import semver from 'semver'
import normalizePath from 'normalize-path'
import { writeJsonFile } from 'write-json-file'

const CLI_PKG_NAME = 'pnpm'

export default async (workspaceDir: string) => { // eslint-disable-line
  const workspaceManifest = await readWorkspaceManifest(workspaceDir)!
  const pnpmManifest = loadJsonFileSync<ProjectManifest>(path.join(workspaceDir, 'pnpm/package.json'))
  const pnpmVersion = pnpmManifest!.version!
  const pnpmMajorNumber = pnpmVersion.split('.')[0]
  const pnpmMajorKeyword = `pnpm${pnpmMajorNumber}`
  const nextTag = `next-${pnpmMajorNumber}`
  const utilsDir = path.join(workspaceDir, '__utils__')
  const lockfile = await readWantedLockfile(workspaceDir, { ignoreIncompatible: false })
  if (lockfile == null) {
    throw new Error('no lockfile found')
  }
  const workspacePackages = await findWorkspacePackagesNoCheck(workspaceDir, { patterns: workspaceManifest?.packages })
  const workspacePackageNames = new Set(workspacePackages.map(pkg => pkg.manifest.name).filter(Boolean))
  return createUpdateOptions({
    'package.json': (manifest: ProjectManifest & { keywords?: string[] } | null, { dir }: { dir: string }) => {
      if (!manifest) {
        return manifest
      }
      if (manifest.name === 'monorepo-root') {
        manifest.scripts!['release'] = `pnpm --filter=@pnpm/exe publish --tag=${nextTag} --access=public && pnpm publish --filter=!pnpm --filter=!@pnpm/exe --access=public && pnpm publish --filter=pnpm --tag=${nextTag} --access=public`
        return sortKeysInManifest(manifest)
      }
      if (manifest.name && manifest.name !== CLI_PKG_NAME) {
        manifest.devDependencies = {
          ...manifest.devDependencies,
          [manifest.name]: 'workspace:*',
        }
      } else if (manifest.name === CLI_PKG_NAME && manifest.devDependencies) {
        delete manifest.devDependencies[manifest.name]
      }
      manifest.keywords = [
        'pnpm',
        pnpmMajorKeyword,
        ...Array.from(new Set((manifest.keywords ?? []).filter((keyword) => keyword !== 'pnpm' && !/^pnpm\d+$/.test(keyword)))).sort(),
      ]
      const smallestAllowedLibVersion = Number(pnpmMajorNumber) * 100
      const libMajorVersion = Number(manifest.version!.split('.')[0])
      if (manifest.name !== CLI_PKG_NAME) {
        if (!semver.prerelease(pnpmVersion) && (libMajorVersion < smallestAllowedLibVersion || libMajorVersion >= smallestAllowedLibVersion + 100)) {
          manifest.version = `${smallestAllowedLibVersion}.0.0`
        }
        for (const depType of ['dependencies', 'devDependencies', 'optionalDependencies'] as const) {
          if (!manifest[depType]) continue
          manifest[depType] = sortDirectKeys(manifest[depType])
          for (const depName of Object.keys(manifest[depType] ?? {})) {
            if (!manifest[depType]?.[depName].startsWith('workspace:')) {
              // @pnpm/scripts is used before packages are compiled, so its deps should use catalog: instead of workspace:*
              const useWorkspaceProtocol = workspacePackageNames.has(depName) && manifest.name !== '@pnpm/scripts'
              manifest[depType]![depName] = useWorkspaceProtocol ? 'workspace:*' : 'catalog:'
            }
          }
        }
      } else {
        for (const depType of ['devDependencies'] as const) {
          if (!manifest[depType]) continue
          for (const depName of Object.keys(manifest[depType] ?? {})) {
            if (!manifest[depType]?.[depName].startsWith('workspace:')) {
              manifest[depType]![depName] = 'catalog:'
            }
          }
        }

        // The main 'pnpm' package should not declare 'peerDependencies' or
        // 'optionalDependencies'. Consider moving to 'devDependencies' if the
        // dependency can be included in the esbuild bundle, or to
        // 'dependencies' if the dependency needs to be externalized and
        // resolved at runtime.
        delete manifest.peerDependencies
        delete manifest.optionalDependencies
      }
      if (manifest.peerDependencies?.['@pnpm/logger'] != null) {
        manifest.peerDependencies['@pnpm/logger'] = 'catalog:'
      }
      if (manifest.peerDependencies?.['@pnpm/worker'] != null) {
        manifest.peerDependencies['@pnpm/worker'] = 'workspace:^'
      }
      const isUtil = isSubdir(utilsDir, dir)
      if (manifest.name !== '@pnpm/make-dedicated-lockfile' && manifest.name !== '@pnpm/mount-modules' && !isUtil && manifest.name !== '@pnpm-private/updater') {
        for (const depType of ['dependencies', 'optionalDependencies'] as const) {
          if (manifest[depType]?.['@pnpm/logger']) {
            delete manifest[depType]!['@pnpm/logger']
          }
          if (manifest[depType]?.['@pnpm/worker']) {
            delete manifest[depType]!['@pnpm/worker']
          }
        }
      }
      if (dir.includes('artifacts') || manifest.name === '@pnpm/exe') {
        manifest.version = pnpmVersion
        if (manifest.name === '@pnpm/exe') {
          for (const depName of ['@pnpm/linux-arm64', '@pnpm/linux-x64', '@pnpm/win-x64', '@pnpm/win-arm64', '@pnpm/macos-x64', '@pnpm/macos-arm64']) {
            manifest.optionalDependencies![depName] = 'workspace:*'
          }
        }
        return sortKeysInManifest(manifest)
      }
      if (manifest.private === true || isUtil) {
        const relative = normalizePath(path.relative(workspaceDir, dir))
        manifest.repository = `https://github.com/pnpm/pnpm/tree/main/${relative}`
        return manifest
      }
      return updateManifest(workspaceDir, manifest, dir, nextTag)
    },
    'tsconfig.json': updateTSConfig.bind(null, {
      lockfile,
      workspaceDir,
    }),
    'cspell.json': (cspell: any) => { // eslint-disable-line
      if (cspell && typeof cspell === 'object' && 'words' in cspell && Array.isArray(cspell.words)) {
        cspell.words = cspell.words.sort((w1: string, w2: string) => w1.localeCompare(w2))
      }
      return cspell
    },
  })
}

async function updateTSConfig (
  context: {
    lockfile: LockfileObject
    workspaceDir: string
  },
  tsConfig: object | null,
  {
    dir,
    manifest,
  }: FormatPluginFnOptions
): Promise<object | null> {
  if (tsConfig == null) return tsConfig
  if (manifest.name === '@pnpm/tsconfig') return tsConfig
  if (manifest.name === '@pnpm-private/typecheck') return tsConfig
  const relative = normalizePath(path.relative(context.workspaceDir, dir)) as ProjectId
  const importer = context.lockfile.importers[relative]
  if (!importer) return tsConfig
  const deps = {
    ...importer.dependencies,
    ...importer.devDependencies,
  }
  const linkValues: string[] = []
  for (const [depName, spec] of Object.entries(deps)) {
    if (!spec.startsWith('link:') || spec.length === 5) continue
    const relativePath = spec.slice(5)
    const linkedPkgDir = path.join(dir, relativePath)
    if (!fs.existsSync(path.join(linkedPkgDir, 'tsconfig.json'))) continue
    if (!isSubdir(context.workspaceDir, linkedPkgDir)) continue
    if (
      depName === '@pnpm/package-store' && (
        manifest.name === '@pnpm/git-fetcher' ||
        manifest.name === '@pnpm/tarball-fetcher' ||
        manifest.name === '@pnpm/package-requester'
      ) ||
      depName === 'pnpm' && manifest.name === '@pnpm/make-dedicated-lockfile'
    ) {
      // This is to avoid a circular graph (which TypeScript references do not support.
      continue
    }
    linkValues.push(relativePath)
  }
  linkValues.sort()

  async function writeTestTsconfig () {
    const testDir = path.join(dir, 'test')
    if (!fs.existsSync(testDir)) {
      return
    }

    await writeJsonFile(path.join(dir, 'test/tsconfig.json'), {
      extends: '../tsconfig.json',
      compilerOptions: {
        noEmit: false,
        outDir: '../node_modules/.test.lib',
        rootDir: '..',
        isolatedModules: true,
      },
      include: [
        '**/*.ts',
        normalizePath(path.relative(testDir, path.join(context.workspaceDir, '__typings__/**/*.d.ts'))),
      ],
      references: (tsConfig as any)?.compilerOptions?.composite === false // eslint-disable-line
        // If composite is explicitly set to false, we can't add the main
        // tsconfig.json as a project reference. Only composite enabled projects
        // can be referenced by definition. Instead, we have to add all the
        // project references directly. Note that this check is approximate. The
        // main tsconfig.json could inherit another conifg that sets composite
        // to be false.
        //
        // The link values are relative to the current packages root. We'll need
        // to re-compute them based off of the "test" directory, which is one
        // directory deeper. In practice the path.relative(...) call below just
        // prepends another "../" to the relPath, but let's use the correct
        // methods to be defensive against future changes to testDir, dir, or
        // relPath.
        ? linkValues.map(relPath => ({
          path: normalizePath(path.relative(testDir, path.join(dir, relPath))),
        }))

        // If the main project is composite (the more common case), we can
        // simply reference that. The main project will have more project
        // references that will apply to the tests too.
        //
        // The project reference allows editor features like Go to Definition
        // jump to files in src for imports using the current package's name
        // (ex: @pnpm/config).
        : [{ path: '..' }],
    }, { indent: 2 })
  }

  await Promise.all([
    writeTestTsconfig(),
    writeJsonFile(path.join(dir, 'tsconfig.lint.json'), {
      extends: './tsconfig.json',
      include: [
        'src/**/*.ts',
        'test/**/*.ts',
        normalizePath(path.relative(dir, path.join(context.workspaceDir, '__typings__/**/*.d.ts'))),
      ],
    }, { indent: 2 }),
  ])
  return {
    ...tsConfig,
    extends: '@pnpm/tsconfig',
    compilerOptions: {
      ...(tsConfig as any)['compilerOptions'], // eslint-disable-line
      rootDir: 'src',
    },
    references: linkValues.map(path => ({ path })),
  }
}

const registryMockPortForCore = 7769

async function updateManifest (workspaceDir: string, manifest: ProjectManifest, dir: string, nextTag: string): Promise<ProjectManifest> {
  const relative = normalizePath(path.relative(workspaceDir, dir))
  let scripts: Record<string, string>
  let preset = '@pnpm/jest-config'
  switch (manifest.name) {
  case '@pnpm/lockfile.types':
    scripts = { ...manifest.scripts }
    break
  case '@pnpm/exec.build-commands':
  case '@pnpm/config.deps-installer':
  case '@pnpm/headless':
  case '@pnpm/outdated':
  case '@pnpm/package-requester':
  case '@pnpm/cache.commands':
  case '@pnpm/plugin-commands-import':
  case '@pnpm/plugin-commands-installation':
  case '@pnpm/plugin-commands-listing':
  case '@pnpm/plugin-commands-outdated':
  case '@pnpm/plugin-commands-patching':
  case '@pnpm/plugin-commands-publishing':
  case '@pnpm/plugin-commands-rebuild':
  case '@pnpm/plugin-commands-script-runners':
  case '@pnpm/plugin-commands-store':
  case '@pnpm/plugin-commands-deploy':
  case CLI_PKG_NAME:
  case '@pnpm/core': {
    preset = '@pnpm/jest-config/with-registry'
    scripts = {
      ...(manifest.scripts as Record<string, string>),
    }
    scripts.test = 'pnpm run compile && pnpm run _test'
    if (manifest.name === '@pnpm/core') {
      // @pnpm/core tests currently works only with port 7769 due to the usage of
      // the next package: pkg-with-tarball-dep-from-registry
      scripts._test = `cross-env PNPM_REGISTRY_MOCK_PORT=${registryMockPortForCore} NODE_OPTIONS="$NODE_OPTIONS --experimental-vm-modules" jest`
    } else {
      scripts._test = 'cross-env NODE_OPTIONS="$NODE_OPTIONS --experimental-vm-modules" jest'
    }
    break
  }
  default:
    if (fs.existsSync(path.join(dir, 'test'))) {
      scripts = {
        ...(manifest.scripts as Record<string, string>),
        _test: 'cross-env NODE_OPTIONS="$NODE_OPTIONS --experimental-vm-modules" jest',
        test: 'pnpm run compile && pnpm run _test',
      }
    } else {
      scripts = {
        ...(manifest.scripts as Record<string, string>),
        test: 'pnpm run compile',
      }
    }
    break
  }
  if (manifest.name === CLI_PKG_NAME) {
    manifest.publishConfig!.tag = nextTag
  }
  if (scripts._test) {
    if (scripts.pretest) {
      scripts._test = `pnpm pretest && ${scripts._test}`
    }
    if (scripts.posttest) {
      scripts._test = `${scripts._test} && pnpm posttest`
    }
    if (manifest.name === '@pnpm/server') {
      scripts._test += ' --detectOpenHandles'
    }
  }
  scripts.compile = 'tsgo --build && pnpm run lint --fix'
  delete scripts.tsc
  if (scripts.start && scripts.start.includes('tsc --watch')) {
    scripts.start = scripts.start.replace('tsc --watch', 'tsgo --watch')
  }
  if (scripts._compile && scripts._compile.includes('tsc --build')) {
    scripts._compile = scripts._compile.replace('tsc --build', 'tsgo --build')
  }
  let homepage: string
  let repository: string | { type: 'git', url: string, directory: 'pnpm' }
  if (manifest.name === CLI_PKG_NAME) {
    homepage = 'https://pnpm.io'
    repository = {
      type: 'git',
      url: 'git+https://github.com/pnpm/pnpm.git',
      directory: 'pnpm',
    }
    scripts.compile += ' && rimraf dist bin/nodes && pnpm run bundle \
&& shx cp -r node-gyp-bin dist/node-gyp-bin \
&& shx cp -r node_modules/@pnpm/tabtab/lib/templates dist/templates \
&& shx cp -r node_modules/ps-list/vendor dist/vendor \
&& shx cp pnpmrc dist/pnpmrc'
  } else {
    scripts.prepublishOnly = 'pnpm run compile'
    homepage = `https://github.com/pnpm/pnpm/tree/main/${relative}#readme`
    repository = `https://github.com/pnpm/pnpm/tree/main/${relative}`
  }
  if (scripts.lint) {
    if (fs.existsSync(path.join(dir, 'test'))) {
      scripts.lint = 'eslint "src/**/*.ts" "test/**/*.ts"'
    } else {
      scripts.lint = 'eslint "src/**/*.ts"'
    }
  }
  const files: string[] = []
  if (manifest.name === CLI_PKG_NAME || manifest.name?.endsWith('/pnpm')) {
    files.push('dist')
    files.push('!dist/**/*.map')
    files.push('bin')
  } else {
    // the order is important
    files.push('lib')
    files.push('!*.map')
    if (manifest.bin) {
      files.push('bin')
    }
  }
  if (manifest.dependencies?.['@types/ramda']) {
    // We should never release @types/ramda as a prod dependency as it breaks the bit repository.
    manifest.devDependencies = {
      ...manifest.devDependencies,
      '@types/ramda': manifest.dependencies['@types/ramda'],
    }
    delete manifest.dependencies['@types/ramda']
  }
  if (scripts.test) {
    Object.assign(manifest, {
      jest: {
        preset,
      },
    })
  }
  return sortKeysInManifest({
    ...manifest,
    type: 'module',
    bugs: {
      url: 'https://github.com/pnpm/pnpm/issues',
    },
    engines: { node: '>=22.13' },
    files,
    funding: 'https://opencollective.com/pnpm',
    homepage,
    license: 'MIT',
    repository,
    scripts,
    exports: {
      ...manifest.exports,
      '.': manifest.name === 'pnpm' ? './package.json' : './lib/index.js',
    },
  })
}

const priority = Object.fromEntries([
  // Metadata
  'name',
  'private',
  'version',
  'description',
  'keywords',
  'license',
  'author',
  'contributors',
  'funding',
  'repository',
  'homepage',
  'bugs',

  // Package Behavior
  'type',
  'main',
  'types',
  'module',
  'browser',
  'exports',
  'files',
  'bin',
  'man',
  'directories',
  'unpkg',

  // Scripts & Configuration
  'scripts',
  'config',

  // Dependencies
  'dependencies',
  'peerDependencies',
  'optionalDependencies',
  'devDependencies',
  'bundledDependencies', // alias: bundleDependencies

  // Engines & Compatibility
  'engines',
  'os',
  'cpu',

  // pnpm/yarn/npm specific fields
  'pnpm',
  'packageManager',
].map((key, index) => [key, index]))

function sortKeysInManifest (manifest: ProjectManifest): ProjectManifest {
  return sortKeysByPriority({ priority }, manifest)
}
