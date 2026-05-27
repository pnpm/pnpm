import { FILTERING } from '@pnpm/cli.common-cli-options-help'
import { docsUrl } from '@pnpm/cli.utils'
import { renderHelp } from 'render-help'

export function help (): string {
  return renderHelp({
    description: 'Stage packages for publishing, deferring proof-of-presence (2FA) to a later point in time.',
    descriptionLists: [
      {
        title: 'Subcommands',
        list: [
          {
            description: 'Stage a package for publishing.',
            name: 'publish',
          },
          {
            description: 'List all staged package versions.',
            name: 'list',
          },
          {
            description: 'View details of a specific staged package.',
            name: 'view',
          },
          {
            description: 'Approve a staged package, publishing it to the npm registry.',
            name: 'approve',
          },
          {
            description: 'Reject a staged package, removing it from the registry.',
            name: 'reject',
          },
          {
            description: 'Download the tarball of a staged package for inspection.',
            name: 'download',
          },
        ],
      },
      {
        title: 'Options',
        list: [
          {
            description: 'The base URL of the npm registry.',
            name: '--registry <url>',
          },
          {
            description: 'Show information in JSON format for list, view, publish, and download.',
            name: '--json',
          },
          {
            description: 'Registers the staged package with the given tag. By default, the "latest" tag is used.',
            name: '--tag <tag>',
          },
          {
            description: 'Tells the registry whether the staged package should be public or restricted.',
            name: '--access <public|restricted>',
          },
          {
            description: 'Does everything stage publish would do except uploading to the registry.',
            name: '--dry-run',
          },
          {
            description: 'One-time password for approve and reject.',
            name: '--otp',
          },
          {
            description: 'Stage all publishable packages from the workspace.',
            name: '--recursive',
            shortAlias: '-r',
          },
        ],
      },
      FILTERING,
    ],
    url: docsUrl('stage'),
    usages: [
      'pnpm stage publish [<tarball>|<dir>] [--tag <tag>] [--access <public|restricted>] [options]',
      'pnpm stage list [<package-spec>]',
      'pnpm stage view <stage-id>',
      'pnpm stage approve <stage-id>',
      'pnpm stage reject <stage-id>',
      'pnpm stage download <stage-id>',
    ],
  })
}
