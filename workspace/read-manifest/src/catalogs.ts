import { InvalidWorkspaceManifestError } from './errors/InvalidWorkspaceManifestError'

export interface WorkspaceNamedCatalogs {
  readonly [catalogName: string]: WorkspaceCatalog
}

export interface WorkspaceCatalog {
  readonly [dependencyName: string]: string
}

export function assertValidWorkspaceManifestCatalog (manifest: { packages?: readonly string[], catalog?: unknown }): asserts manifest is { catalog?: WorkspaceCatalog } {
  if (manifest.catalog == null) {
    return
  }

  if (Array.isArray(manifest.catalog)) {
    throw new InvalidWorkspaceManifestError('Expected catalog field to be an object, but found - array')
  }

  if (typeof manifest.catalog !== 'object') {
    throw new InvalidWorkspaceManifestError(`Expected catalog field to be an object, but found - ${typeof manifest.catalog}`)
  }

  for (const [alias, specifier] of Object.entries(manifest.catalog)) {
    if (typeof specifier !== 'string') {
      throw new InvalidWorkspaceManifestError(`Invalid catalog entry for ${alias}. Expected string, but found: ${typeof specifier}`)
    }
  }
}

export function assertValidWorkspaceManifestCatalogs (manifest: { packages?: readonly string[], catalogs?: unknown }): asserts manifest is { catalogs?: WorkspaceNamedCatalogs } {
  if (manifest.catalogs == null) {
    return
  }

  if (Array.isArray(manifest.catalogs)) {
    throw new InvalidWorkspaceManifestError('Expected catalogs field to be an object, but found - array')
  }

  if (typeof manifest.catalogs !== 'object') {
    throw new InvalidWorkspaceManifestError(`Expected catalogs field to be an object, but found - ${typeof manifest.catalogs}`)
  }

  for (const [catalogName, catalog] of Object.entries(manifest.catalogs)) {
    if (Array.isArray(catalog)) {
      throw new InvalidWorkspaceManifestError(`Expected named catalog ${catalogName} to be an object, but found - array`)
    }

    if (typeof catalog !== 'object') {
      throw new InvalidWorkspaceManifestError(`Expected named catalog ${catalogName} to be an object, but found - ${typeof catalog}`)
    }

    for (const [alias, specifier] of Object.entries(catalog)) {
      if (typeof specifier !== 'string') {
        throw new InvalidWorkspaceManifestError(`Catalog '${catalogName}' has invalid entry '${alias}'. Expected string specifier, but found: ${typeof specifier}`)
      }
    }
  }
}
