export type VersioningBumpType = 'patch' | 'minor' | 'major'

export type VersioningChangelogStorage = 'registry' | 'repository'

export interface VersioningChangelogSettings {
  format?: string
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
   * Per-package prerelease lines: maps a package name to the prerelease tag
   * of the line it is on (e.g. `"@example/cli": "alpha"`).
   */
  prereleases?: Record<string, string>
  changelog?: VersioningChangelogSettings
}
