import PnpmError from '@pnpm/error'
import {
  install,
  mutateModules,
  rebuild,
} from 'supi'
import createStoreController from '../createStoreController'
import findWorkspacePackages, { arrayOfLocalPackagesToMap } from '../findWorkspacePackages'
import getPinnedVersion from '../getPinnedVersion'
import getSaveType from '../getSaveType'
import { readImporterManifestOnly, tryReadImporterManifest } from '../readImporterManifest'
import requireHooks from '../requireHooks'
import { PnpmOptions } from '../types'
import updateToLatestSpecsFromManifest, { createLatestSpecs } from '../updateToLatestSpecsFromManifest'
import { recursive } from './recursive'

const OVERWRITE_UPDATE_OPTIONS = {
  allowNew: true,
  update: false,
}

/**
 * Perform installation.
 * @example
 *     installCmd([ 'lodash', 'foo' ], { silent: true })
 */
export default async function installCmd (
  input: string[],
  opts: PnpmOptions & {
    allowNew?: boolean,
    update?: boolean,
    useBetaCli?: boolean,
  },
  invocation?: string,
) {
  // `pnpm install ""` is going to be just `pnpm install`
  input = input.filter(Boolean)

  const dir = opts.dir || process.cwd()

  const localPackages = opts.linkWorkspacePackages && opts.workspaceDir
    ? arrayOfLocalPackagesToMap(
      await findWorkspacePackages(opts.workspaceDir, opts),
    )
    : undefined

  if (!opts.ignorePnpmfile) {
    opts.hooks = requireHooks(opts.lockfileDir || dir, opts)
  }
  const store = await createStoreController(opts)
  const installOpts = {
    ...opts,
    // In case installation is done in a multi-package repository
    // The dependencies should be built first,
    // so ignoring scripts for now
    ignoreScripts: !!localPackages || opts.ignoreScripts,
    localPackages,
    storeController: store.ctrl,
    storeDir: store.dir,

    forceHoistPattern: typeof opts.rawLocalConfig['hoist-pattern'] !== 'undefined' || typeof opts.rawLocalConfig['hoist'] !== 'undefined',
    forceIndependentLeaves: typeof opts.rawLocalConfig['independent-leaves'] !== 'undefined',
    forceShamefullyHoist: typeof opts.rawLocalConfig['shamefully-hoist'] !== 'undefined',
  }

  let { manifest, writeImporterManifest } = await tryReadImporterManifest(opts.dir, opts)
  if (manifest === null) {
    if (opts.update) {
      throw new PnpmError('NO_IMPORTER_MANIFEST', 'No package.json found')
    }
    manifest = {}
  }

  if (opts.update && opts.latest) {
    if (!input || !input.length) {
      input = updateToLatestSpecsFromManifest(manifest, opts.include)
    } else {
      input = createLatestSpecs(input, manifest)
    }
    delete installOpts.include
  }
  if (!input || !input.length) {
    if (invocation === 'add') {
      throw new PnpmError('MISSING_PACKAGE_NAME', '`pnpm add` requires the package name')
    }
    await install(manifest, installOpts)
  } else {
    const [updatedImporter] = await mutateModules([
      {
        allowNew: opts.allowNew,
        bin: installOpts.bin,
        dependencySelectors: input,
        manifest,
        mutation: 'installSome',
        peer: opts.savePeer,
        pinnedVersion: getPinnedVersion(opts),
        prefix: installOpts.dir,
        targetDependenciesField: getSaveType(installOpts),
      },
    ], installOpts)
    await writeImporterManifest(updatedImporter.manifest)
  }

  if (opts.linkWorkspacePackages && opts.workspaceDir) {
    // TODO: reuse somehow the previous read of packages
    // this is not optimal
    const allWorkspacePkgs = await findWorkspacePackages(opts.workspaceDir, opts)
    await recursive(allWorkspacePkgs, [], {
      ...opts,
      ...OVERWRITE_UPDATE_OPTIONS,
      ignoredPackages: new Set([dir]),
      packageSelectors: [
        {
          pattern: dir,
          scope: 'dependencies',
          selectBy: 'location',
        },
      ],
      workspaceDir: opts.workspaceDir, // Otherwise TypeScript doesn't understant that is is not undefined
    }, 'install', 'install')

    if (opts.ignoreScripts) return

    await rebuild(
      [
        {
          buildIndex: 0,
          manifest: await readImporterManifestOnly(opts.dir, opts),
          prefix: opts.dir,
        },
      ], {
        ...opts,
        pending: true,
        storeController: store.ctrl,
        storeDir: store.dir,
      },
    )
  }
}
