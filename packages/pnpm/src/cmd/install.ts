import PnpmError from '@pnpm/error'
import { getSaveType } from '@pnpm/utils'
import {
  install,
  mutateModules,
  rebuild,
} from 'supi'
import createStoreController from '../createStoreController'
import findWorkspacePackages, { arrayOfLocalPackagesToMap } from '../findWorkspacePackages'
import getPinnedVersion from '../getPinnedVersion'
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
  },
) {
  // `pnpm install ""` is going to be just `pnpm install`
  input = input.filter(Boolean)

  const prefix = opts.prefix || process.cwd()

  const localPackages = opts.linkWorkspacePackages && opts.workspacePrefix
    ? arrayOfLocalPackagesToMap(
      await findWorkspacePackages(opts.workspacePrefix, opts),
    )
    : undefined

  if (!opts.ignorePnpmfile) {
    opts.hooks = requireHooks(opts.lockfileDirectory || prefix, opts)
  }
  const store = await createStoreController(opts)
  const installOpts = {
    ...opts,
    // In case installation is done in a multi-package repository
    // The dependencies should be built first,
    // so ignoring scripts for now
    ignoreScripts: !!localPackages || opts.ignoreScripts,
    localPackages,
    store: store.path,
    storeController: store.ctrl,
  }

  let { manifest, writeImporterManifest } = await tryReadImporterManifest(opts.prefix, opts)
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
        prefix: installOpts.prefix,
        targetDependenciesField: getSaveType(installOpts),
      },
    ], installOpts)
    await writeImporterManifest(updatedImporter.manifest)
  }

  if (opts.linkWorkspacePackages && opts.workspacePrefix && manifest.name) {
    // TODO: reuse somehow the previous read of packages
    // this is not optimal
    const allWorkspacePkgs = await findWorkspacePackages(opts.workspacePrefix, opts)
    await recursive(allWorkspacePkgs, [], {
      ...opts,
      ...OVERWRITE_UPDATE_OPTIONS,
      ignoredPackages: new Set([prefix]),
      packageSelectors: [
        {
          matcher: manifest.name,
          scope: 'dependencies',
          selectBy: 'name',
        },
      ],
    }, 'install', 'install')

    if (opts.ignoreScripts) return

    await rebuild(
      [
        {
          buildIndex: 0,
          manifest: await readImporterManifestOnly(opts.prefix, opts),
          prefix: opts.prefix,
        },
      ], {
        ...opts,
        pending: true,
      } as any, // tslint:disable-line:no-any
    )
  }
}
