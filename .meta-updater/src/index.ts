import fs from 'fs'
import path from 'path'
import { readWantedLockfile, type Lockfile } from '@pnpm/lockfile-file'
import { type ProjectId, type ProjectManifest } from '@pnpm/types'
import { createUpdateOptions, type FormatPluginFnOptions } from '@pnpm/meta-updater'
import isSubdir from 'is-subdir'
import loadJsonFile from 'load-json-file'
import normalizePath from 'normalize-path'
import exists from 'path-exists'
import writeJsonFile from 'write-json-file'

const NEXT_TAG = 'next-9'
const CLI_PKG_NAME = 'pnpm'

export default async (workspaceDir: string) => {
  const pnpmManifest = loadJsonFile.sync<any>(path.join(workspaceDir, 'pnpm/package.json'))
  const pnpmVersion = pnpmManifest!['version'] // eslint-disable-line
  const pnpmMajorKeyword = `pnpm${pnpmVersion.split('.')[0]}`
  const utilsDir = path.join(workspaceDir, '__utils__')
  const lockfile = await readWantedLockfile(workspaceDir, { ignoreIncompatible: false })
  if (lockfile == null) {
    throw new Error('no lockfile found')
  }
  return createUpdateOptions({
    'package.json': (manifest: ProjectManifest & { keywords?: string[] } | null, { dir }) => {
      if (!manifest) {
        return manifest;
      }
      if (manifest.name === 'monorepo-root') {
        manifest.scripts!['release'] = `pnpm --filter=@pnpm/exe publish --tag=${NEXT_TAG} --access=public && pnpm publish --filter=!pnpm --filter=!@pnpm/exe --access=public && pnpm publish --filter=pnpm --tag=${NEXT_TAG} --access=public`
        return manifest
      }
      if (manifest.name && manifest.name !== CLI_PKG_NAME) {
        manifest.devDependencies = {
          ...manifest.devDependencies,
          [manifest.name]: `workspace:*`,
        }
      } else if (manifest.name === CLI_PKG_NAME && manifest.devDependencies) {
        delete manifest.devDependencies[manifest.name]
      }
      if (manifest.private || isSubdir(utilsDir, dir)) return manifest
      manifest.keywords = [
        pnpmMajorKeyword,
        ...(manifest.keywords ?? []).filter((keyword) => !/^pnpm[0-9]+$/.test(keyword)),
      ]
      if (manifest.name !== CLI_PKG_NAME) {
        for (const depType of ['dependencies', 'devDependencies', 'optionalDependencies'] as const) {
          if (!manifest[depType]) continue
          for (const depName of Object.keys(manifest[depType] ?? {})) {
            if (!manifest[depType]?.[depName].startsWith('workspace:')) {
              manifest[depType]![depName] = 'catalog:'
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
        for (const depType of ['dependencies', 'optionalDependencies'] as const) {
          if (!manifest[depType]) continue
          for (const depName of Object.keys(manifest[depType] ?? {})) {
            if (manifest[depType]?.[depName] === 'catalog:') {
              throw new Error('The pnpm CLI package cannot have "catalog:" in prod deps as publish-packed does not support them currently')
            }
          }
        }
      }
      if (dir.includes('artifacts') || manifest.name === '@pnpm/exe') {
        manifest.version = pnpmVersion
        if (manifest.name === '@pnpm/exe') {
          for (const depName of ['@pnpm/linux-arm64', '@pnpm/linux-x64', '@pnpm/win-x64', '@pnpm/win-arm64', '@pnpm/macos-x64', '@pnpm/macos-arm64']) {
            manifest.optionalDependencies![depName] = `workspace:*`
          }
        }
        return manifest
      }
      return updateManifest(workspaceDir, manifest, dir)
    },
    'tsconfig.json': updateTSConfig.bind(null, {
      lockfile,
      workspaceDir,
    }),
    'cspell.json': (cspell: any) => {
      if (cspell?.words) {
        cspell.words = cspell.words.sort()
      }
      return cspell
    },
  })
}

async function updateTSConfig (
  context: {
    lockfile: Lockfile
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
    if (!await exists(path.join(linkedPkgDir, 'tsconfig.json'))) continue
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
    if (!await exists(testDir)) {
      return
    }

    await writeJsonFile(path.join(dir, 'test/tsconfig.json'), {
      extends: '../tsconfig.json',
      compilerOptions: {
        // The composite flag allows projects to be specified as references in
        // other projects. Test projects should not be dependencies of other
        // files and don't need composite set.
        //
        // Some tests perform a relative import into the "src" directory to test
        // non-publicly exported idenfiers.
        //
        //   import { foo } from "../src/foo"
        //
        // Instead of:
        //
        //   import { foo } from "@pnpm/example"
        //
        // The relative "../src" imports would error if composite is enabled
        // since composite would require files in "src" to be part of the test
        // project.
        composite: false,
        noEmit: true,

        rootDir: '.'
      },
      include: [
        '**/*.ts',
        path.relative(testDir, path.join(context.workspaceDir, '__typings__/**/*.d.ts')),
      ],
      references: (tsConfig as any)?.compilerOptions?.composite === false
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
          path: path.relative(testDir, path.join(dir, relPath))
        }))

        // If the main project is composite (the more common case), we can
        // simply reference that. The main project will have more project
        // references that will apply to the tests too.
        //
        // The project reference allows editor features like Go to Definition
        // jump to files in src for imports using the current package's name
        // (ex: @pnpm/config).
        : [{ path: ".." }]
    }, { indent: 2 })
  }

  await Promise.all([
    writeTestTsconfig(),
    writeJsonFile(path.join(dir, 'tsconfig.lint.json'), {
      extends: './tsconfig.json',
      include: [
        'src/**/*.ts',
        'test/**/*.ts',
        path.relative(dir, path.join(context.workspaceDir, '__typings__/**/*.d.ts')),
      ],
    }, { indent: 2 })
  ])
  return {
    ...tsConfig,
    extends: '@pnpm/tsconfig',
    compilerOptions: {
      ...(tsConfig as any)['compilerOptions'],
      rootDir: 'src',
    },
    references: linkValues.map(path => ({ path })),
  }
}

let registryMockPort = 7769

type UpdatedManifest = ProjectManifest & Record<string, unknown>

async function updateManifest (workspaceDir: string, manifest: ProjectManifest, dir: string): Promise<UpdatedManifest> {
  const relative = normalizePath(path.relative(workspaceDir, dir))
  let scripts: Record<string, string>
  switch (manifest.name) {
  case '@pnpm/lockfile-types':
    scripts = { ...manifest.scripts }
    break
  case '@pnpm/headless':
  case '@pnpm/outdated':
  case '@pnpm/package-requester':
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
    // @pnpm/core tests currently works only with port 4873 due to the usage of
    // the next package: pkg-with-tarball-dep-from-registry
    const port = manifest.name === '@pnpm/core' ? 4873 : ++registryMockPort
    scripts = {
      ...(manifest.scripts as Record<string, string>),
    }
    scripts.test = 'pnpm run compile && pnpm run _test'
    scripts._test = `cross-env PNPM_REGISTRY_MOCK_PORT=${port} jest`
    break
  }
  default:
    if (await exists(path.join(dir, 'test'))) {
      scripts = {
        ...(manifest.scripts as Record<string, string>),
        _test: 'jest',
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
    manifest.publishConfig!.tag = NEXT_TAG
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
  scripts.compile = 'tsc --build && pnpm run lint --fix'
  delete scripts.tsc
  let homepage: string
  let repository: string | { type: 'git', url: string }
  if (manifest.name === CLI_PKG_NAME) {
    homepage = 'https://pnpm.io'
    repository = {
      type: 'git',
      url: 'git+https://github.com/pnpm/pnpm.git',
    }
    scripts.compile += ' && rimraf dist bin/nodes && pnpm run bundle \
&& shx cp -r node-gyp-bin dist/node-gyp-bin \
&& shx cp -r node_modules/@pnpm/tabtab/lib/templates dist/templates \
&& shx cp -r node_modules/ps-list/vendor dist/vendor \
&& shx cp pnpmrc dist/pnpmrc'
  } else {
    scripts.prepublishOnly = 'pnpm run compile'
    homepage = `https://github.com/pnpm/pnpm/blob/main/${relative}#readme`
    repository = `https://github.com/pnpm/pnpm/blob/main/${relative}`
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
  return {
    ...manifest,
    bugs: {
      url: 'https://github.com/pnpm/pnpm/issues',
    },
    engines: {
      node: '>=18.12',
    },
    files,
    funding: 'https://opencollective.com/pnpm',
    homepage,
    license: 'MIT',
    repository,
    scripts,
    exports: {
      '.': manifest.name === 'pnpm' ? './package.json' : './lib/index.js',
    },
  }
}
