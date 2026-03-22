// This file contains type definitions that aren't just `export = any`

declare module 'cli-columns' {
  function cliColumns (values: string[], opts?: { newline?: string, width?: number }): string
  export = cliColumns
}

declare module 'normalize-registry-url' {
  function normalizeRegistryUrl (registry: string): string
  export = normalizeRegistryUrl
}

declare module 'normalize-newline' {
  function normalizeNewline (text: string): string
  export = normalizeNewline
}

declare module 'path-name' {
  const pathname: string
  export = pathname
}

declare module 'right-pad' {
  function rightPad (txt: string, size: number): string
  export = rightPad
}

declare module 'semver-utils' {
  export function parseRange (range: string): Array<{
    semver?: string
    operator: string
    major?: string
    minor?: string
    patch?: string
  }>
}

declare module 'split-cmd' {
  export function split (cmd: string): string[]
  export function splitToObject (cmd: string): { command: string, args: string[] }
}

declare module 'strip-comments-strings' {
  export interface CodeItem {
    // What feature of the code has been found:
    type: string
    // The indices of the feature in the original code string:
    index: number
    indexEnd: number
    // The test of the feature
    content: string
  }
  export interface CodeAttributes {
    // The remaining code text after all features have been stripped:
    text: string
    // The items found:
    comments: CodeItem[]
    regexes: CodeItem[]
    strings: CodeItem[]
  }
  export function parseString (str: string): CodeAttributes
  export type CodeItemReplacer = (item: CodeItem) => string
  export function stripComments (
    str: string, replacer?: CodeItemReplacer): string
}

declare module 'bin-links/lib/fix-bin.js' {
  function fixBin (path: string, execMode: number): Promise<void>
  export = fixBin
}

declare namespace NodeJS.Module {
  function _nodeModulePaths (from: string): string[]
}

declare module '@pnpm/npm-conf/lib/types' {
  interface npmType {
    types: {
      access: Array<string | null>
      'allow-same-version': BooleanConstructor
      'always-auth': BooleanConstructor
      also: Array<string | null>
      audit: BooleanConstructor
      'auth-type': string[]
      'bin-links': BooleanConstructor
      browser: Array<StringConstructor | null>
      ca: Array<StringConstructor | ArrayConstructor | null>
      // eslint-disable-next-line @typescript-eslint/consistent-type-imports
      cafile: import('path').PlatformPath
      // eslint-disable-next-line @typescript-eslint/consistent-type-imports
      cache: import('path').PlatformPath
      'cache-lock-stale': NumberConstructor
      'cache-lock-retries': NumberConstructor
      'cache-lock-wait': NumberConstructor
      'cache-max': NumberConstructor
      'cache-min': NumberConstructor
      cert: Array<StringConstructor | null>
      cidr: Array<StringConstructor | ArrayConstructor | null>
      color: Array<string | BooleanConstructor>
      depth: NumberConstructor
      description: BooleanConstructor
      dev: BooleanConstructor
      'dry-run': BooleanConstructor
      editor: StringConstructor
      'engine-strict': BooleanConstructor
      force: BooleanConstructor
      'fetch-retries': NumberConstructor
      'fetch-retry-factor': NumberConstructor
      'fetch-retry-mintimeout': NumberConstructor
      'fetch-retry-maxtimeout': NumberConstructor
      git: StringConstructor
      'git-tag-version': BooleanConstructor
      'commit-hooks': BooleanConstructor
      global: BooleanConstructor
      // eslint-disable-next-line @typescript-eslint/consistent-type-imports
      globalconfig: import('path').PlatformPath
      'global-style': BooleanConstructor
      group: Array<StringConstructor | NumberConstructor>
      // eslint-disable-next-line @typescript-eslint/consistent-type-imports
      'https-proxy': Array<typeof import('url') | null>
      'user-agent': StringConstructor
      'ham-it-up': BooleanConstructor
      heading: StringConstructor
      'if-present': BooleanConstructor
      'ignore-prepublish': BooleanConstructor
      'ignore-scripts': BooleanConstructor
      // eslint-disable-next-line @typescript-eslint/consistent-type-imports
      'init-module': import('path').PlatformPath
      'init-author-name': StringConstructor
      'init-author-email': StringConstructor
      // eslint-disable-next-line @typescript-eslint/consistent-type-imports
      'init-author-url': Array<string | typeof import('url')>
      'init-license': StringConstructor
      'init-version': () => void
      json: BooleanConstructor
      key: Array<StringConstructor | null>
      'legacy-bundling': BooleanConstructor
      link: BooleanConstructor
      'local-address': never[]
      loglevel: string[]
      // eslint-disable-next-line @typescript-eslint/consistent-type-imports
      logstream: typeof import('stream').Stream
      'logs-max': NumberConstructor
      long: BooleanConstructor
      maxsockets: NumberConstructor
      message: StringConstructor
      'metrics-registry': Array<StringConstructor | null>
      'node-options': Array<StringConstructor | null>
      'node-version': Array<(() => void) | null>
      'no-proxy': Array<StringConstructor | ArrayConstructor | null>
      offline: BooleanConstructor
      'onload-script': Array<StringConstructor | null>
      only: Array<string | null>
      optional: BooleanConstructor
      'package-lock': BooleanConstructor
      otp: Array<StringConstructor | null>
      'package-lock-only': BooleanConstructor
      parseable: BooleanConstructor
      'prefer-offline': BooleanConstructor
      'prefer-online': BooleanConstructor
      // eslint-disable-next-line @typescript-eslint/consistent-type-imports
      prefix: import('path').PlatformPath
      production: BooleanConstructor
      progress: BooleanConstructor
      provenance: BooleanConstructor
      // eslint-disable-next-line @typescript-eslint/consistent-type-imports
      proxy: Array<boolean | typeof import('url') | null>
      'read-only': BooleanConstructor
      'rebuild-bundle': BooleanConstructor
      // eslint-disable-next-line @typescript-eslint/consistent-type-imports
      registry: Array<typeof import('url') | null>
      rollback: BooleanConstructor
      save: BooleanConstructor
      'save-bundle': BooleanConstructor
      'save-dev': BooleanConstructor
      'save-exact': BooleanConstructor
      'save-optional': BooleanConstructor
      'save-prefix': StringConstructor
      'save-prod': BooleanConstructor
      scope: StringConstructor
      'script-shell': Array<StringConstructor | null>
      'scripts-prepend-node-path': Array<string | boolean>
      searchopts: StringConstructor
      searchexclude: Array<StringConstructor | null>
      searchlimit: NumberConstructor
      searchstaleness: NumberConstructor
      'send-metrics': BooleanConstructor
      shell: StringConstructor
      shrinkwrap: BooleanConstructor
      'sign-git-tag': BooleanConstructor
      'sso-poll-frequency': NumberConstructor
      'sso-type': Array<string | null>
      'strict-ssl': BooleanConstructor
      tag: StringConstructor
      timing: BooleanConstructor
      // eslint-disable-next-line @typescript-eslint/consistent-type-imports
      tmp: import('path').PlatformPath
      unicode: BooleanConstructor
      'unsafe-perm': BooleanConstructor
      usage: BooleanConstructor
      user: Array<StringConstructor | NumberConstructor>
      // eslint-disable-next-line @typescript-eslint/consistent-type-imports
      userconfig: import('path').PlatformPath
      umask: () => void
      version: BooleanConstructor
      'tag-version-prefix': StringConstructor
      versions: BooleanConstructor
      viewer: StringConstructor
      _exit: BooleanConstructor
    }
  }
  const npmTypes: npmType
  export = npmTypes
}

declare module 'npm-packlist' {
  interface PacklistTree {
    path: string
    package: Record<string, unknown>
    isProjectRoot?: boolean
    edgesOut?: Map<string, unknown>
    workspaces?: Map<string, string>
  }
  function npmPacklist (tree: PacklistTree, options?: Record<string, unknown>): Promise<string[]>
  export = npmPacklist
}
