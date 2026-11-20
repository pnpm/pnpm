import { renderHelp } from 'render-help'

const OPTIONS = [
  {
    description: "Don't check if working tree is clean",
    name: '--no-git-checks',
  },
  {
    description: 'Sets the prerelease identifier (e.g. alpha, beta, rc)',
    name: '--preid <preid>',
  },
  {
    description: 'Sets the tag prefix. Default is "v". Set to empty string to remove the prefix.',
    name: '--tag-version-prefix <prefix>',
  },
  {
    description: 'Allow bumping to the same version',
    name: '--allow-same-version',
  },
  {
    description: 'Commit message. "%s" is replaced with the new version. Default is "%s".',
    name: '--message <message>',
  },
  {
    description: "Don't create a commit or tag for the version bump. Git commits and tags are always skipped in recursive mode.",
    name: '--no-git-tag-version',
  },
  {
    description: 'Skip running git commit hooks when committing the version bump',
    name: '--no-commit-hooks',
  },
  {
    description: 'Sign the generated git tag with GPG',
    name: '--sign-git-tag',
  },
  {
    description: 'Filter packages by name (glob pattern)',
    name: '--filter <pattern>',
  },
  {
    description: 'Show information in JSON format',
    name: '--json',
  },
  {
    description: 'Apply command to all packages in workspace. Without a version argument, consumes the pending change intents from .changeset/ and applies the resulting release plan',
    name: '--recursive',
  },
  {
    description: 'Print what the command would do without changing anything',
    name: '--dry-run',
  },
]

export function help (): string {
  return renderHelp({
    description: 'Bumps the version of a package.',
    usages: [
      'pnpm version <newversion>',
      'pnpm version <major|minor|patch|premajor|preminor|prepatch|prerelease|from-git>',
      'pnpm version -r [--dry-run]',
    ],
    descriptionLists: [
      {
        title: 'Options',
        list: OPTIONS,
      },
    ],
  })
}
