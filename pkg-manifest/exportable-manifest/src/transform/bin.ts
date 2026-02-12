import { PnpmError } from '@pnpm/error'
import { type ProjectManifest } from '@pnpm/types'
import { type ExportedManifest } from './index.js'

type Input = Pick<ProjectManifest, 'bin'> & Pick<ExportedManifest, 'name'>
type Output<Manifest extends Input> = Omit<Manifest, 'bin'> & Pick<ExportedManifest, 'bin'>

export function transformBin<Manifest extends Input> (manifest: Manifest): Output<Manifest> {
  if (manifest.bin == null || typeof manifest.bin === 'object') return manifest as Output<Manifest>
  const { bin, ...rest } = manifest
  return {
    ...rest,
    bin: normalizeBinObject(manifest.name, bin),
  }
}

/**
 * The property `"bin"` of a `package.json` could be either an object or a string.
 * This function normalizes either forms into an object.
 */
export function normalizeBinObject (pkgName: string, bin: string | Record<string, string>): Record<string, string> {
  if (typeof bin === 'object') return bin
  const binName = normalizeBinName(pkgName)
  return { [binName]: bin }
}

function normalizeBinName (name: string): string {
  if (name[0] !== '@') return name
  const slashIndex = name.indexOf('/')
  if (slashIndex < 0) {
    throw new InvalidScopedPackageNameError(name)
  }
  return name.slice(slashIndex + 1)
}

export class InvalidScopedPackageNameError extends PnpmError {
  readonly invalidName: string
  constructor (invalidName: string) {
    super('INVALID_SCOPED_PACKAGE_NAME', `The name ${JSON.stringify(invalidName)} is not a valid scoped package name`)
    this.invalidName = invalidName
  }
}
