import {stripIndent} from 'common-tags'
import getCommandFullName from '../getCommandFullName'

export default function (input: string[]) {
  const cmdName = getCommandFullName(input[0])
  console.log(getHelpText(cmdName))
}

function getHelpText(command: string) {
  switch (getCommandFullName(command)) {
    case 'install':
      return stripIndent`
        pnpm install (with no args, in package dir)
        pnpm install [<@scope>/]<name>
        pnpm install [<@scope>/]<name>@<tag>
        pnpm install [<@scope>/]<name>@<version>
        pnpm install [<@scope>/]<name>@<version range>
        pnpm install <git-host>:<git-user>/<repo-name>
        pnpm install <git repo url>
        pnpm install <tarball file>
        pnpm install <tarball url>
        pnpm install <folder>

        Aliases: i, install

        Options:

          -P, --save-prod                 save package to your \`dependencies\`
          -D, --save-dev                  save package to your \`devDependencies\`
          -O, --save-optional             save package to your \`optionalDependencies\`
          -E, --save-exact                install exact version
          -g, --global                    install as a global package
          --store                         the location where all the packages are saved on the disk.
          --offline                       trigger an error if any required dependencies are not available in local store
          --network-concurrency <number>  maximum number of concurrent network requests
          --child-concurrency <number>    controls the number of child processes run parallelly to build node modules
          --independent-leaves            symlinks leaf dependencies directly from the global store
          --[no-]verify-store-integrity   if false, doesn't check whether packages in the store were mutated
          --production                    packages in \`devDependencies\` won't be installed
          --[no-]lock
      `

    case 'uninstall':
      return stripIndent`
        pnpm uninstall [<@scope>/]<pkg>[@<version>]...

        Aliases: remove, rm, r, un, unlink

        Removes packages from \`node_modules\` and from the project's \`packages.json\`
      `

    case 'link':
      return stripIndent`
        pnpm link (in package dir)
        pnpm link [<@scope>/]<pkg>
        pnpm link <folder>

        Aliases: ln
      `

    case 'dislink':
      return stripIndent`
        pnpm dislink (in package dir)
        pnpm dislink [<@scope>/]<pkg>...

        Removes the link created by \`pnpm link\` and reinstalls package if it is saved in \`package.json\`
      `

    case 'update':
      return stripIndent`
        pnpm update [-g] [<pkg>...]

        Aliases: up, upgrade

        Options:

          -g, --global                    update globally installed packages
          --depth                         how deep should levels of dependencies be inspected
                                          0 is default, which means top-level dependencies
      `

    case 'list':
      return stripIndent`
        pnpm ls [[<@scope>/]<pkg> ...]

        Aliases: list, la, ll

        When run as ll or la, it shows extended information by default.

        Options:

          --long                          show extended information
          --parseable                     show parseable output instead of tree view
          -g, --global                    list packages in the global install prefix instead of in the current project
          --depth                         max display depth of the dependency tree
          --prod, --production            display only the dependency tree for packages in \`dependencies\`.
          --dev                           display only the dependency tree for packages in \`devDependencies\`.
      `

    case 'prune':
      return stripIndent`
        pnpm prune [--production]

        Options:

          --prod, --production            remove the packages specified in \`devDependencies\`

        Removes extraneous packages
      `

    case 'install-test':
      return stripIndent`
        This command runs an \`npm install\` followed immediately by an \`npm test\`.
        It takes exactly the same arguments as \`npm install\`.
      `

    default:
      return stripIndent`
        Usage: pnpm [command] [flags]

        Commands:

          - install
          - update
          - uninstall
          - link
          - dislink
          - list
          - prune
          - install-test

        Other commands are passed through to npm
      `
  }
}
