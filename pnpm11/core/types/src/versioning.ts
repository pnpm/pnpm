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
 * Settings for native workspace release management, declared under the
 * `versioning` key of pnpm-workspace.yaml.
 */
export interface VersioningSettings {
  /**
   * Groups of packages that always release together at one shared version.
   */
  fixed?: string[][]
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
