export type VersioningBumpType = 'patch' | 'minor' | 'major'

export type VersioningChangelogStorage = 'registry' | 'repository'

export interface VersioningChangelogSettings {
  format?: string
  /**
   * Where release changelogs live. Defaults to `registry`: no CHANGELOG.md is
   * committed; each release's section is composed at publish time and packed
   * into the published tarball, on top of the previously published version's
   * changelog. `repository` keeps a committed CHANGELOG.md in every package.
   */
  storage?: VersioningChangelogStorage
}

/**
 * An epic ties a group of member packages to a lead package, constraining
 * every member's major version to the band derived from the lead's major:
 * while the lead is on major `M`, members live in `M×100 … M×100+99`. Members
 * move independently inside the band; when a release plan takes the lead to a
 * new stable major, every member re-bases to the band floor in the same plan.
 */
export interface VersioningEpic {
  /**
   * The package whose major version defines the band, referenced by name or
   * by `./`-prefixed workspace directory (e.g. `pnpm`).
   */
  lead: string
  /**
   * Selectors matching the member packages: name globs, `./`-prefixed
   * directory globs, and `!`-prefixed negations, following pnpm's selector
   * conventions (e.g. `["./pnpm11/**", "!./pnpm11/private/**"]`).
   */
  packages: string[]
}

/**
 * Settings for native workspace release management, declared under the
 * `versioning` key of pnpm-workspace.yaml.
 */
export interface VersioningSettings {
  /**
   * Groups of packages that always release together at one shared version.
   */
  fixed?: string[][]
  /**
   * Epics that band member packages' majors to a lead package's major.
   */
  epics?: VersioningEpic[]
  /**
   * Packages permanently excluded from versioning and dependent propagation.
   */
  ignore?: string[]
  /**
   * Caps the bump a release from the current checkout may apply. Enforced on
   * the final assembled release plan, after dependent propagation and
   * fixed-group resolution.
   */
  maxBump?: VersioningBumpType
  /**
   * Per-package release lanes: maps a package name to the lane it is on
   * (e.g. `"@example/cli": "alpha"`). A lane is a parallel release track that
   * emits `X.Y.Z-tag.N` prereleases; every unlisted package is on the
   * reserved default lane, `main`, and releases stable versions.
   */
  lanes?: Record<string, string>
  changelog?: VersioningChangelogSettings
}
