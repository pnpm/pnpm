import { PnpmError } from '@pnpm/error'

export class CatalogVersionMismatchError extends PnpmError {
  public catalogDep: string
  public wantedDep: string
  constructor (
    opts: {
      catalogDep: string
      wantedDep: string
    }
  ) {
    super('CATALOG_VERSION_MISMATCH', 'Wanted dependency outside the version range defined in catalog')
    this.catalogDep = opts.catalogDep
    this.wantedDep = opts.wantedDep
  }
}
