import fs from 'node:fs'
import path from 'node:path'

import { type LockfileObject, readWantedLockfile } from '@pnpm/lockfile.fs'
import { createFormat, createUpdateOptions, type FormatPluginFnOptions } from '@pnpm/meta-updater'
import { sortDirectKeys, sortKeysByPriority } from '@pnpm/object.key-sorting'
import type { ProjectId, ProjectManifest } from '@pnpm/types'
import { findWorkspaceProjectsNoCheck } from '@pnpm/workspace.projects-reader'
import { readWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'
import { isSubdir } from 'is-subdir'
import { loadJsonFileSync } from 'load-json-file'
import normalizePath from 'normalize-path'
import semver from 'semver'
import { writeJsonFile } from 'write-json-file'

const CLI_PKG_NAME = 'pnpm'

// Experimental packages that are versioned independently on the 0.0.x track
// and should not be normalized to the pnpm major version.
const EXPERIMENTAL_PKGS = new Set([
  '@pnpm/pnpr.client',
])

// Files that must be packed with mode 0755 in both `pnpm` and `@pnpm/exe`.
// `@pnpm/exe` ships the same `dist/` tree as `pnpm`, so the two manifests'
// `publishConfig.executableFiles` lists must stay identical — otherwise the
// shims end up packed at 0644 in one of the tarballs (see #11483).
const PUBLISH_EXECUTABLE_FILES = [
  './dist/node-gyp-bin/node-gyp',
  './dist/node-gyp-bin/node-gyp.cmd',
  './dist/node_modules/node-gyp/bin/node-gyp.js',
]

// Packages whose tests spawn the local pnpm CLI binary (pnpm/bin/pnpm.mjs)
// and therefore need the CLI bundle (pnpm/dist/pnpm.mjs) to be built first.
const PKGS_NEEDING_CLI_COMPILE = new Set([
  '@pnpm/building.commands',
  '@pnpm/cache.commands',
  '@pnpm/deps.inspection.commands',
  '@pnpm/exec.commands',
  '@pnpm/lockfile.make-dedicated-lockfile',
  '@pnpm/releasing.commands',
  '@pnpm/releasing.exportable-manifest',
  '@pnpm/store.commands',
])

export default async (workspaceDir: string) => { // eslint-disable-line
  const workspaceManifest = await readWorkspaceManifest(workspaceDir)!
  const pnpmManifest = loadJsonFileSync<ProjectManifest>(path.join(workspaceDir, 'pnpm11/pnpm/package.json'))
  const pnpmVersion = pnpmManifest!.version!
  const pnpmMajorNumber = pnpmVersion.split('.')[0]
  const pnpmMajorKeyword = `pnpm${pnpmMajorNumber}`
  const nextTag = `next-${pnpmMajorNumber}`
  const utilsDir = path.join(workspaceDir, 'pnpm11/__utils__')
  const lockfile = await readWantedLockfile(workspaceDir, { ignoreIncompatible: false })
  if (lockfile == null) {
    throw new Error('no lockfile found')
  }
  const workspacePackages = await findWorkspaceProjectsNoCheck(workspaceDir, { patterns: workspaceManifest?.packages })
  const workspacePackageNames = new Set(workspacePackages.map(pkg => pkg.manifest.name).filter(Boolean))
  const nodeRuntimeVersion = readNodeRuntimeVersion(workspaceDir)
  return createUpdateOptions({
    formats: { '.yml': yamlTextFormat },
    files: {
      'package.json': (manifest: ProjectManifest & { keywords?: string[] } | null, { dir }: { dir: string }) => {
        if (!manifest) {
          return manifest
        }
        if (manifest.name === 'monorepo-root') {
          manifest.scripts!['release'] = `pn --filter=@pnpm/exe run build-artifacts && pn --filter=@pnpm/exe publish --tag=${nextTag} --access=public --provenance && pn publish --filter=!pnpm --filter=!@pnpm/exe --access=public --provenance && pn publish --filter=pnpm --tag=${nextTag} --access=public --provenance`
          syncNodeRuntimeInScripts(manifest, nodeRuntimeVersion)
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
          if (!semver.prerelease(pnpmVersion) && !EXPERIMENTAL_PKGS.has(manifest.name!) && (libMajorVersion < smallestAllowedLibVersion || libMajorVersion >= smallestAllowedLibVersion + 100)) {
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
        if (manifest.name !== '@pnpm/lockfile.make-dedicated-lockfile' && manifest.name !== '@pnpm/modules-mounter.daemon' && !isUtil && manifest.name !== '@pnpm-private/updater') {
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
            for (const depName of [
              '@pnpm/linux-arm64',
              '@pnpm/linux-x64',
              '@pnpm/linuxstatic-arm64',
              '@pnpm/linuxstatic-x64',
              '@pnpm/macos-arm64',
              '@pnpm/win-arm64',
              '@pnpm/win-x64',
            ]) {
              manifest.optionalDependencies![depName] = 'workspace:*'
            }
            manifest.publishConfig ??= {}
            manifest.publishConfig.executableFiles = [...PUBLISH_EXECUTABLE_FILES]
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
      // GitHub Actions workflows pin the Node.js version the repo's CI, release,
      // and benchmark jobs run on. Keep those pins on the same major as
      // `devEngines.runtime` in sync with it; the lower-bound matrix entries
      // (older majors) are deliberate and left untouched.
      '.github/workflows/ci.yml': (content: string | null) => syncNodeVersionInWorkflow(content, nodeRuntimeVersion),
      '.github/workflows/release.yml': (content: string | null) => syncNodeVersionInWorkflow(content, nodeRuntimeVersion),
      '.github/workflows/benchmark.yml': (content: string | null) => syncNodeVersionInWorkflow(content, nodeRuntimeVersion),
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
      depName === '@pnpm/store.controller' && (
        manifest.name === '@pnpm/fetching.git-fetcher' ||
        manifest.name === '@pnpm/fetching.tarball-fetcher' ||
        manifest.name === '@pnpm/installing.package-requester'
      ) ||
      depName === 'pnpm' && manifest.name === '@pnpm/lockfile.make-dedicated-lockfile'
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
        normalizePath(path.relative(testDir, path.join(context.workspaceDir, 'pnpm11/__typings__/**/*.d.ts'))),
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
        // (ex: @pnpm/config.reader).
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
        normalizePath(path.relative(dir, path.join(context.workspaceDir, 'pnpm11/__typings__/**/*.d.ts'))),
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

// `devEngines.runtime` in the root manifest is the single source of truth for
// the Node.js version the repo builds with. Returns it only when it pins a
// concrete `node` version.
function readNodeRuntimeVersion (workspaceDir: string): string | undefined {
  const manifest = loadJsonFileSync<{ devEngines?: { runtime?: { name?: string, version?: string } } }>(path.join(workspaceDir, 'package.json'))
  const runtime = manifest?.devEngines?.runtime
  return runtime?.name === 'node' ? runtime.version : undefined
}

// Keep the Node.js version pinned in scripts (e.g. `pnx node@runtime:26.3.0`)
// in sync with `devEngines.runtime`.
function syncNodeRuntimeInScripts (manifest: ProjectManifest, version: string | undefined): void {
  if (!version || !manifest.scripts) return
  for (const [scriptName, scriptCmd] of Object.entries(manifest.scripts)) {
    manifest.scripts[scriptName] = scriptCmd.replace(/node@runtime:[\d.]+/g, `node@runtime:${version}`)
  }
}

// Pin the Node.js version in a workflow's YAML to `devEngines.runtime`, but
// only the entries sharing its major (e.g. `node@26.0.0` and `'26.0.0'` when
// the runtime is `26.3.0`). Older-major matrix entries are deliberate
// lower-bound test targets and are left untouched. The file is rewritten as
// text so comments and formatting are preserved.
function syncNodeVersionInWorkflow (content: string | null, version: string | undefined): string | null {
  if (content == null || !version) return content
  const major = version.split('.')[0]
  return content.replace(new RegExp(`\\b${major}\\.\\d+\\.\\d+\\b`, 'g'), version)
}

const yamlTextFormat = createFormat<string>({
  read: ({ resolvedPath }) => fs.readFileSync(resolvedPath, 'utf8'),
  update: (actual, updater, options) => updater(actual, options),
  equal: (expected, actual) => expected === actual,
  write: (expected, { resolvedPath }) => {
    fs.writeFileSync(resolvedPath, expected)
  },
})

const registryMockPortForCore = 7769

async function updateManifest (workspaceDir: string, manifest: ProjectManifest, dir: string, nextTag: string): Promise<ProjectManifest> {
  const relative = normalizePath(path.relative(workspaceDir, dir))
  let scripts: Record<string, string>
  let preset = '@pnpm/jest-config'
  switch (manifest.name) {
    case '@pnpm/lockfile.types':
      scripts = { ...manifest.scripts }
      break
    case '@pnpm/building.commands':
    case '@pnpm/installing.deps-restorer':
    case '@pnpm/installing.env-installer':
    case '@pnpm/deps.inspection.outdated':
    case '@pnpm/installing.package-requester':
    case '@pnpm/cache.commands':
    case '@pnpm/plugin-commands-import':
    case '@pnpm/installing.commands':
    case '@pnpm/deps.inspection.commands':
    case '@pnpm/patching.commands':
    case '@pnpm/registry-access.commands':
    case '@pnpm/releasing.commands':
    case '@pnpm/exec.commands':
    case '@pnpm/store.commands':
    case '@pnpm/deps.compliance.commands':
    case CLI_PKG_NAME:
    case '@pnpm/installing.deps-installer': {
      preset = '@pnpm/jest-config/with-registry'
      scripts = {
        ...(manifest.scripts as Record<string, string>),
      }
      scripts.test = 'pn compile && pn .test'
      if (manifest.name === '@pnpm/installing.deps-installer') {
      // @pnpm/installing.deps-installer tests currently works only with port 7769 due to the usage of
      // the next package: pkg-with-tarball-dep-from-registry
      //
      // deepRecursive resolves @teambit/bit's enormous circular/peer graph and
      // needs ~3.6 GB on its own — enough to fit Node's default ~4 GB heap, but
      // not with the memory the other test files leave behind in the same
      // process (Jest's `--experimental-vm-modules` registry isn't reclaimed
      // between files). Run it in a dedicated jest process (`.test:heavy`) so it
      // gets the whole heap to itself, and run the rest (`.test:rest`) in a
      // separate process with it excluded.
        const heavyTestPath = 'test/install/deepRecursive.ts'
        const testEnv = `cross-env PNPM_REGISTRY_MOCK_PORT=${registryMockPortForCore} NODE_OPTIONS="$NODE_OPTIONS --experimental-vm-modules --disable-warning=ExperimentalWarning --disable-warning=DEP0169"`
        scripts['.test'] = 'pn .test:heavy && pn .test:rest'
        scripts['.test:heavy'] = `${testEnv} jest ${heavyTestPath}`
        scripts['.test:rest'] = `${testEnv} jest "^(?!.*deepRecursive)"`
      } else {
        scripts['.test'] = 'cross-env NODE_OPTIONS="$NODE_OPTIONS --experimental-vm-modules --disable-warning=ExperimentalWarning --disable-warning=DEP0169" jest'
      }
      break
    }
    default:
      if (fs.existsSync(path.join(dir, 'test'))) {
        scripts = {
          ...(manifest.scripts as Record<string, string>),
          '.test': 'cross-env NODE_OPTIONS="$NODE_OPTIONS --experimental-vm-modules --disable-warning=ExperimentalWarning --disable-warning=DEP0169" jest',
          test: 'pn compile && pn .test',
        }
      } else {
        scripts = {
          ...(manifest.scripts as Record<string, string>),
          test: 'pn compile',
        }
      }
      break
  }
  if (manifest.name && PKGS_NEEDING_CLI_COMPILE.has(manifest.name)) {
    scripts.test = 'pn compile && pn --filter=pnpm compile && pn .test'
  }
  // Clean up old underscore-prefixed script names
  delete scripts._test
  delete scripts._compile
  if (manifest.name === CLI_PKG_NAME) {
    manifest.publishConfig!.tag = nextTag
    manifest.publishConfig!.executableFiles = [...PUBLISH_EXECUTABLE_FILES]
  }
  if (scripts['.test']) {
    if (scripts.pretest) {
      scripts['.test'] = `pn pretest && ${scripts['.test']}`
    }
    if (scripts.posttest) {
      scripts['.test'] = `${scripts['.test']} && pn posttest`
    }
    if (manifest.name === '@pnpm/server') {
      scripts['.test'] += ' --detectOpenHandles'
    }
  }
  scripts.compile = 'tsgo --build && pn lint --fix'
  delete scripts.tsc
  if (scripts.start && scripts.start.includes('tsc --watch')) {
    scripts.start = scripts.start.replace('tsc --watch', 'tsgo --watch')
  }
  if (scripts['.compile'] && scripts['.compile'].includes('tsc --build')) {
    scripts['.compile'] = scripts['.compile'].replace('tsc --build', 'tsgo --build')
  }
  let homepage: string
  let repository: string | { type: 'git', url: string, directory: 'pnpm11/pnpm' }
  if (manifest.name === CLI_PKG_NAME) {
    homepage = 'https://pnpm.io'
    repository = {
      type: 'git',
      url: 'git+https://github.com/pnpm/pnpm.git',
      directory: 'pnpm11/pnpm',
    }
    scripts.compile += ' && rimraf dist bin/nodes && pn bundle \
&& shx cp -r node-gyp-bin dist/node-gyp-bin \
&& shx cp -r node_modules/@pnpm/tabtab/lib/templates dist/templates \
&& shx cp -r node_modules/ps-list/vendor dist/vendor \
&& shx cp pnpmrc dist/pnpmrc'
  } else {
    scripts.prepublishOnly = 'tsgo --build'
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
        ...(manifest as any).jest, // eslint-disable-line
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
