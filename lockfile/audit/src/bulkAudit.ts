// cspell:ignore metavuln
// cspell:ignore metavulns
// cspell:ignore vulns
import path from 'path'
import { satisfies } from 'semver'
import Calculator from '@npmcli/metavuln-calculator'
import type { BulkAuditTree, BulkAuditNode } from './lockfileToBulkAuditTree.js'
import type { DepPath, PackageManifest } from '@pnpm/types'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { depPathToFilename } from '@pnpm/dependency-path'
import { safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import { getSpecFromPackageManifest } from '@pnpm/manifest-utils'
import { pickPackageFromMeta, pickVersionByVersionRange } from '@pnpm/npm-resolver/pickPackageFromMeta'
import { parseBareSpecifier, type PackageMeta } from '@pnpm/npm-resolver'
import type { VersionSelectors } from '@pnpm/resolver-base'
import normalizePath from 'normalize-path'

export type BulkAuditReport = Record<string, BulkAuditAdvisory[]>

export interface BulkAuditAdvisory {
  id: number
  url: string
  title: string
  severity: string
  vulnerable_versions: string
  cwe: string[]
  cvss: {
    score: number
    vectorString: string | null
  }
}

const semverOpt = { loose: true, includePrerelease: true }

declare class Advisory {
  source: number
  name: string
  dependency: string
  title: string
  url: string
  severity: string
  versions: string[]
  vulnerableVersions: string[]
  cwe: string[]
  cvss: {
    score: number
    vectorString: string | null
  }

  range: string | null
  id: string

  get updated (): boolean
  get type (): 'advisory' | 'metavuln'
  get packument (): PackageMeta | null

  testVersion (version: string, spec?: string): boolean
  testSpec (spec: string): boolean
}

export type VulnFixAvailable = boolean | { name: string, version: string, isSemVerMajor?: boolean }

export class Vuln {
  readonly name: string
  readonly packument: PackageMeta | null
  readonly versions: string[]
  readonly via = new Set<Vuln>()
  readonly advisories = new Set<Advisory>()
  readonly effects = new Set<Vuln>()
  readonly nodes = new Set<BulkAuditNode>()
  readonly topNodes = new Set<BulkAuditNode>()
  #fixAvailable: VulnFixAvailable = true

  constructor (options: { name: string, advisory: Advisory }) {
    this.name = options.name
    this.addAdvisory(options.advisory)
    this.packument = options.advisory.packument
    this.versions = options.advisory.versions
  }

  get fixAvailable (): VulnFixAvailable {
    return this.#fixAvailable
  }

  set fixAvailable (f: VulnFixAvailable) {
    this.#fixAvailable = f
    // if there's a fix available for this at the top level, it means that
    // it will also fix the vulns that led to it being there.  to get there,
    // we set the vias to the most "strict" of fix available.
    // - false: no fix is available
    // - {name, version, isSemVerMajor} fix requires -f, is semver major
    // - {name, version} fix requires -f, not semver major
    // - true: fix does not require -f
    // TODO: duped entries may require different fixes but the current
    // structure does not support this, so the case were a top level fix
    // corrects a duped entry may mean you have to run fix more than once
    for (const v of this.via) {
      // don't blow up on loops
      if (v.fixAvailable === f) {
        continue
      }

      if (f === false) {
        v.fixAvailable = f
      } else if (v.fixAvailable === true) {
        v.fixAvailable = f
      } else if (typeof f === 'object' && (
        typeof v.fixAvailable !== 'object' || !v.fixAvailable.isSemVerMajor)) {
        v.fixAvailable = f
      }
    }
  }

  testSpec (spec: string): boolean {
    // TODO:
    // const specObj = npa(spec)
    // if (!specObj.registry) {
    //   return true
    // }

    // if (specObj.subSpec) {
    //   spec = specObj.subSpec.rawSpec
    // }
    for (const v of this.versions) {
      if (satisfies(v, spec) && !satisfies(v, this.range, semverOpt)) {
        return false
      }
    }
    return true
  }

  addVia (v: Vuln): void {
    this.via.add(v)
    this.effects.add(v)
    // TODO: // call the setter since we might add vias _after_ setting fixAvailable
    // this.fixAvailable = this.fixAvailable
  }

  deleteVia (v: Vuln): void {
    this.via.delete(v)
    v.effects.delete(this)
  }

  deleteAdvisory (advisory: Advisory): void {
    this.advisories.delete(advisory)
    // // make sure we have the max severity of all the vulns causing this one
    // this.severity = null
    // this.#range = null
    // this.#simpleRange = null
    // // refresh severity
    // for (const advisory of this.advisories) {
    //   this.addAdvisory(advisory)
    // }

    // remove any effects that are no longer relevant
    const vias = new Set([...this.advisories].map(a => a.dependency))
    for (const via of this.via) {
      if (!vias.has(via.name)) {
        this.deleteVia(via)
      }
    }

    // TODO: update versions
  }

  addAdvisory (advisory: Advisory): void {
    this.advisories.add(advisory)
    // const sev = severities.get(advisory.severity)
    // this.#range = null
    // this.#simpleRange = null
    // if (sev > severities.get(this.severity)) {
    //   this.severity = advisory.severity
    // }

    // TODO: update versions
  }

  get range (): string {
    return [...this.advisories].map(v => v.range).join(' || ')
  }
}

export class BulkProcessedAuditReport {
  public readonly report = new Map<string, Vuln>()
  public readonly topVulns = new Map<string, Vuln>()
}

export async function performBulkAudit (
  report: BulkAuditReport,
  auditTree: BulkAuditTree,
  lockfile: LockfileObject,
  opts: {
    lockfileDir: string
    virtualStoreDir: string
    virtualStoreDirMaxLength: number
  }
): Promise<BulkProcessedAuditReport> {
  const calculator = new Calculator()

  const promises: Array<Promise<unknown>> = []
  for (const [name, advisories] of Object.entries(report)) {
    for (const advisory of advisories) {
      promises.push(calculator.calculate(name, advisory))
    }
  }

  // now the advisories are calculated with a set of versions
  // and the packument.  turn them into our style of vuln objects
  // which also have the affected nodes, and also create entries
  // for all the metavulns that we find from dependents.
  const advisories = new Set((await Promise.all(promises)) as Advisory[])
  const auditReport = new BulkProcessedAuditReport()

  const specLoader = new PackageSpecLoader(
    lockfile,
    {
      lockfileDir: opts.lockfileDir,
      virtualStoreDir: opts.virtualStoreDir,
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    }
  )

  const seen = new Set<string>()
  for (const advisory of advisories) {
    const { name, range } = advisory
    const k = `${name}@${range}`

    let vuln = auditReport.report.get(name)
    if (vuln) {
      vuln.addAdvisory(advisory)
    } else {
      vuln = new Vuln({ name, advisory })
      auditReport.report.set(name, vuln)
    }

    // don't flag the exact same name/range more than once
    // adding multiple advisories with the same range is fine, but no
    // need to search for nodes we already would have added.
    if (!seen.has(k)) {
      const p: Array<Promise<void>> = []
      for (const node of auditTree.allNodesByPackageName.get(name) ?? []) {
        if (!shouldAudit(node)) {
          continue
        }

        // if not vulnerable by this advisory, keep searching
        if (!advisory.testVersion(node.version)) {
          continue
        }

        // we will have loaded the source already if this is a metavuln
        if (advisory.type === 'metavuln') {
          vuln.addVia(auditReport.report.get(advisory.dependency)!)
        }

        // already marked this one, no need to do it again
        if (vuln.nodes.has(node)) {
          continue
        }

        // haven't marked this one yet.  get its dependents.
        vuln.nodes.add(node)
        for (const dep of node.dependents) {
          if (dep.isImporter) {
            // Nothing to do if this is an importer
            continue
          }
          // eslint-disable-next-line no-await-in-loop
          const spec = await specLoader.getSpecForDependencyRelationship(dep, node)
          if (spec === null) {
            // TODO: spec isn't available or dep is a top level package
            continue
          }
          if (dep.isDirect && !vuln.topNodes.has(dep)) {
            vuln.fixAvailable = fixAvailable(vuln, spec)
            if (vuln.fixAvailable !== true) {
              // now we know the top node is vulnerable, and cannot be
              // upgraded out of the bad place without --force.  But, there's
              // no need to add it to the actual vulns list, because nothing
              // depends on root.
              auditReport.topVulns.set(vuln.name, vuln)
              vuln.topNodes.add(dep)
            }
          } else {
          // calculate a metavuln, if necessary
            const calc = calculator.calculate(dep.name, advisory) as Promise<Advisory>

            p.push(calc.then(meta => {
              if (meta.testVersion(dep.version, spec)) {
                advisories.add(meta)
              }
            }))
          }
        }
      }
      // eslint-disable-next-line no-await-in-loop
      await Promise.all(p)
      seen.add(k)
    }

    // make sure we actually got something.  if not, remove it
    // this can happen if you are loading from a lockfile created by
    // npm v5, since it lists the current version of all deps,
    // rather than the range that is actually depended upon,
    // or if using --omit with the older audit endpoint.
    if (auditReport.report.get(name)!.nodes.size === 0) {
      auditReport.report.delete(name)
      continue
    }

    // if the vuln is valid, but THIS advisory doesn't apply to any of
    // the nodes it references, then remove it from the advisory list.
    // happens when using omit with old audit endpoint.
    for (const advisory of vuln.advisories) {
      const relevant = [...vuln.nodes]
        .some(n => advisory.testVersion(n.version))
      if (!relevant) {
        vuln.deleteAdvisory(advisory)
      }
    }
  }

  return auditReport
}

function shouldAudit (node: BulkAuditNode): boolean {
  if (!node.version || node.isImporter) {
    return false
  }
  return true
}
// given the spec, see if there is a fix available at all, and note whether or not it's a semver major fix or not (i.e. will need --force)
function fixAvailable (vuln: Vuln, spec: string): VulnFixAvailable {
  // TODO we return true, false, OR an object here. this is probably a bad pattern.
  if (!vuln.testSpec(spec)) {
    return true
  }

  if (!vuln.packument) {
    return false
  }

  // TODO: replace registry url
  const specObj = parseBareSpecifier(spec, vuln.name, 'latest', 'https://registry.npmjs.org/')
  if (!specObj) {
    return false
  }

  // // even if we HAVE a packument, if we're looking for it somewhere other than the registry and we have something vulnerable then we're stuck with it.
  // const specObj = npa(spec)
  // if (!specObj.registry) {
  //   return false
  // }

  // if (specObj.subSpec) {
  //   spec = specObj.subSpec.rawSpec
  // }

  // TODO: From npm-pick-manifest:
  // // we don't provide fixes for top nodes other than root, but we still check to see if the node is fixable with a different version, and note if that is a semver major bump.
  // try {
  //   const {
  //     _isSemVerMajor: isSemVerMajor,
  //     version,
  //     name,
  //   } = pickManifest(vuln.packument, spec, {
  //     ...this.options,
  //     before: null,
  //     avoid: vuln.range,
  //     avoidStrict: true,
  //   })
  //   return { name, version, isSemVerMajor }
  // } catch (er) {
  //   return false
  // }
  const versionSelectors: VersionSelectors = {}
  for (const version of Object.keys(vuln.packument.versions)) {
    versionSelectors[version] = { selectorType: 'version', weight: 0 }
  }
  versionSelectors[vuln.range] = { selectorType: 'range', weight: -1 }
  const chosenPackage = pickPackageFromMeta(pickVersionByVersionRange, {
    preferredVersionSelectors: versionSelectors,
  }, specObj, vuln.packument)
  if (!chosenPackage) {
    return false
  }
  // TODO: this won't work as-is because pickPackageFromMeta will never return a package outside of the given spec
  return {
    name: chosenPackage.name,
    version: chosenPackage.version,
    isSemVerMajor: false, // TODO: assume false for now
  }
}

class PackageSpecLoader {
  manifestCache = new Map<string, PackageManifest | null>()

  constructor (
    private readonly lockfile: LockfileObject,
    private readonly opts: {
      lockfileDir: string
      virtualStoreDir: string
      virtualStoreDirMaxLength: number
    }
  ) { }

  async getPackageManifest (depPath: DepPath): Promise<PackageManifest | null> {
    if (this.manifestCache.has(depPath)) {
      return this.manifestCache.get(depPath)!
    }

    const pkgLocation = lockfileToPackageLocation(depPath, this.lockfile, this.opts)
    if (!pkgLocation) {
      return null
    }
    const manifest = await safeReadPackageJsonFromDir(pkgLocation)
    this.manifestCache.set(depPath, manifest)
    return manifest
  }

  async getSpecForDependencyRelationship (parent: BulkAuditNode, child: BulkAuditNode): Promise<string | null> {
    if (!parent.depPath) {
      return null
    }
    const manifest = await this.getPackageManifest(parent.depPath)
    if (!manifest) {
      return null
    }
    const spec = getSpecFromPackageManifest(manifest, child.name)
    if (spec === '') {
      return null
    }
    return spec
  }
}

function lockfileToPackageLocation (
  depPath: DepPath,
  lockfile: LockfileObject,
  opts: {
    lockfileDir: string
    virtualStoreDir: string
    virtualStoreDirMaxLength: number
  }
): string | undefined {
  const pkgSnapshot = lockfile.packages?.[depPath]
  if (!pkgSnapshot) {
    return undefined
  }

  const { name } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)

  // Seems like this field should always contain a relative path
  let packageLocation = normalizePath(path.join(
    opts.virtualStoreDir,
    depPathToFilename(depPath, opts.virtualStoreDirMaxLength),
    'node_modules',
    name
  ))
  if (!packageLocation.startsWith('../') && !path.isAbsolute(packageLocation)) {
    packageLocation = `./${packageLocation}`
  }
  if (!packageLocation.endsWith('/')) {
    packageLocation += '/'
  }
  return packageLocation
}
