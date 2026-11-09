import fs from 'node:fs'
import path from 'node:path'

import { type LockfileObject, type ProjectSnapshot, readWantedLockfile } from '@pnpm/lockfile.fs'
import { createFormat, createUpdateOptions, type FormatPluginFnOptions } from '@pnpm/meta-updater'
import { sortDirectKeys } from '@pnpm/object.key-sorting'
import type { ProjectId, ProjectManifest } from '@pnpm/types'
import { findWorkspaceProjectsNoCheck } from '@pnpm/workspace.projects-reader'
import { readWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'
import { isSubdir } from 'is-subdir'
import { loadJsonFileSync } from 'load-json-file'
import normalizePath from 'normalize-path'
import { writeJsonFile } from 'write-json-file'

import { NPM_SIGNING_KEYS_FILE, type NpmSigningKey, renderNpmSigningKeysModule, replaceRustNpmSigningKeys } from './npmSigningKeys.js'
import { PUBLISH_EXECUTABLE_FILES, sortKeysInManifest, updateManifest } from './updateManifest.js'

const CLI_PKG_NAME = 'pnpm'

// These packages have their own release metadata and build pipelines;
// the TypeScript package entry points, engine range, and Jest scripts do not apply.
const NON_TYPESCRIPT_PKGS = new Set([
  // A TypeScript package, but released on its own and built with its own
  // tsconfig rather than the pnpm v11 conventions.
  'get-pnpm',
  'pacquet',
  '@pnpm/napi',
  '@pnpm/pnpr',
  '@pnpm/pnpr-ui',
  '@pnpm/esm-loader',
])

// Files that must be packed with mode 0755 in both `pnpm` and `@pnpm/exe`.
// `@pnpm/exe` ships the same `dist/` tree as `pnpm`, so the two manifests'
// `publishConfig.executableFiles` lists must stay identical — otherwise the
// shims end up packed at 0644 in one of the tarballs (see pnpm/pnpm#11483).
export default async (workspaceDir: string) => { // eslint-disable-line
  const workspaceManifest = await readWorkspaceManifest(workspaceDir)!
  const pnpmManifest = loadJsonFileSync<ProjectManifest>(path.join(workspaceDir, 'pnpm11/pnpm/package.json'))
  const pnpmVersion = pnpmManifest!.version!
  // The Rust products embed the versions their release builds report and
  // verify. `pnpm version -r` bumps only the npm wrapper manifests, so those
  // are the source of truth; the Rust-source handlers below mirror them, and
  // `meta-updater --test` (run in pre-push and CI) fails a missed sync before
  // it can reach the release workflow.
  const rustCliVersion = loadJsonFileSync<ProjectManifest>(path.join(workspaceDir, 'pnpm/npm/pnpm/package.json'))!.version!
  const pnprVersion = loadJsonFileSync<ProjectManifest>(path.join(workspaceDir, 'pnpr/npm/pnpr/package.json'))!.version!
  const npmSigningKeys = loadJsonFileSync<NpmSigningKey[]>(path.join(workspaceDir, NPM_SIGNING_KEYS_FILE))
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
    formats: { '.yml': textFormat, '.rs': textFormat, '.toml': textFormat, '.lock': textFormat, '.ts': textFormat },
    files: {
      'package.json': (manifest: ProjectManifest & { keywords?: string[] } | null, { dir }: { dir: string }) => {
        if (!manifest) {
          return manifest
        }
        if (manifest.name && NON_TYPESCRIPT_PKGS.has(manifest.name)) {
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
        if (manifest.name !== CLI_PKG_NAME) {
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
          if (manifest.name === '@pnpm/exe') {
            for (const depName of [
              '@pnpm/linux-arm64',
              '@pnpm/linux-x64',
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
      // Mirror the bumped npm wrapper versions into the Rust sources their
      // release builds report and verify (see rustCliVersion / pnprVersion).
      'pnpm/crates/config/src/defaults.rs': (content: string | null) =>
        replaceRustVersion(content, /(pub const PNPM_VERSION: &str = )"[^"]*"/, `$1"${rustCliVersion}"`),
      'pnpr/crates/pnpr/Cargo.toml': (content: string | null) =>
        replaceRustVersion(content, /^(version\s*=\s*)"[^"]*"/m, `$1"${pnprVersion}"`),
      'Cargo.lock': (content: string | null) =>
        replaceRustVersion(content, /(\[\[package\]\]\nname = "pnpr"\nversion = )"[^"]*"/, `$1"${pnprVersion}"`),
      // The npm signing keys each product pins, from the one committed list.
      // pnpm keeps trusting the keys npm withdrew; get-pnpm, an installer of
      // current releases only, does not.
      'pnpm11/deps/security/signatures/src/npmSigningKeys.ts': (content: string | null) =>
        content == null ? content : renderNpmSigningKeysModule(npmSigningKeys, { currentOnly: false }),
      'pnpm/npm/get-pnpm/src/npmSigningKeys.ts': (content: string | null) =>
        content == null ? content : renderNpmSigningKeysModule(npmSigningKeys, { currentOnly: true }),
      'pnpm/crates/cli/src/cli_args/self_update/verify_engine/signatures.rs': (content: string | null) =>
        replaceRustNpmSigningKeys(content, npmSigningKeys),
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
  if (shouldKeepTSConfig(manifest.name)) return tsConfig
  const relative = normalizePath(path.relative(context.workspaceDir, dir)) as ProjectId
  const importer = context.lockfile.importers[relative]
  if (!importer) return tsConfig
  const linkValues = findTypeScriptReferences(context.workspaceDir, dir, importer, manifest.name)
  await writeTsConfigFiles(context.workspaceDir, dir, tsConfig, linkValues)
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

function shouldKeepTSConfig (name: string | undefined): boolean {
  return name === '@pnpm/tsconfig' || name === '@pnpm-private/typecheck' ||
    (name != null && NON_TYPESCRIPT_PKGS.has(name))
}

function findTypeScriptReferences (
  workspaceDir: string,
  dir: string,
  importer: ProjectSnapshot,
  projectName: string | undefined
): string[] {
  const deps = { ...importer.dependencies, ...importer.devDependencies }
  return Object.entries(deps)
    .filter(([depName, spec]) => {
      if (!spec.startsWith('link:') || spec.length === 5) return false
      const linkedPkgDir = path.join(dir, spec.slice(5))
      return fs.existsSync(path.join(linkedPkgDir, 'tsconfig.json')) &&
        isSubdir(workspaceDir, linkedPkgDir) &&
        !isExcludedTypeScriptReference(depName, projectName)
    })
    .map(([, spec]) => spec.slice(5))
    .sort()
}

function isExcludedTypeScriptReference (dependencyName: string, projectName: string | undefined): boolean {
  return dependencyName === '@pnpm/store.controller' && [
    '@pnpm/fetching.git-fetcher',
    '@pnpm/fetching.tarball-fetcher',
    '@pnpm/installing.package-requester',
  ].includes(projectName ?? '') ||
    dependencyName === 'pnpm' && projectName === '@pnpm/lockfile.make-dedicated-lockfile'
}

async function writeTsConfigFiles (
  workspaceDir: string,
  dir: string,
  tsConfig: object,
  linkValues: string[]
): Promise<void> {
  const testDir = path.join(dir, 'test')
  const testConfig = fs.existsSync(testDir)
    ? writeTestTsConfig(workspaceDir, dir, testDir, tsConfig, linkValues)
    : Promise.resolve()
  await Promise.all([
    testConfig,
    writeJsonFile(path.join(dir, 'tsconfig.lint.json'), {
      extends: './tsconfig.json',
      include: [
        'src/**/*.ts',
        'test/**/*.ts',
        normalizePath(path.relative(dir, path.join(workspaceDir, 'pnpm11/__typings__/**/*.d.ts'))),
      ],
    }, { indent: 2 }),
  ])
}

async function writeTestTsConfig (
  workspaceDir: string,
  dir: string,
  testDir: string,
  tsConfig: object,
  linkValues: string[]
): Promise<void> {
  await writeJsonFile(path.join(testDir, 'tsconfig.json'), {
    extends: '../tsconfig.json',
    compilerOptions: {
      noEmit: false,
      outDir: '../node_modules/.test.lib',
      rootDir: '..',
      isolatedModules: true,
    },
    include: [
      '**/*.ts',
      normalizePath(path.relative(testDir, path.join(workspaceDir, 'pnpm11/__typings__/**/*.d.ts'))),
    ],
    references: getTestReferences(tsConfig, dir, testDir, linkValues),
  }, { indent: 2 })
}

function getTestReferences (tsConfig: object, dir: string, testDir: string, linkValues: string[]): Array<{ path: string }> {
  const compilerOptions = (tsConfig as any)?.compilerOptions // eslint-disable-line
  if (compilerOptions?.composite !== false) return [{ path: '..' }]
  return linkValues.map(relativePath => ({
    path: normalizePath(path.relative(testDir, path.join(dir, relativePath))),
  }))
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

// Rewrites a version literal in a Rust source file to match the npm wrapper.
// `meta-updater` passes `null` for the packages that don't have the file (only
// the workspace root does), which are left untouched; a present file that has
// lost the expected pattern is a hard error so a renamed constant can't
// silently stop being synced.
function replaceRustVersion (content: string | null, pattern: RegExp, replacement: string): string | null {
  if (content == null) return content
  if (!pattern.test(content)) {
    throw new Error(`Pattern ${pattern} not found`)
  }
  return content.replace(pattern, replacement)
}

const textFormat = createFormat<string>({
  read: ({ resolvedPath }) => fs.readFileSync(resolvedPath, 'utf8'),
  update: (actual, updater, options) => updater(actual, options),
  equal: (expected, actual) => expected === actual,
  write: (expected, { resolvedPath }) => {
    fs.writeFileSync(resolvedPath, expected)
  },
})
