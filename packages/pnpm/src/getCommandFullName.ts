export default function getCommandFullName (cmd: string) {
  switch (cmd) {
    case 'add':
    case 'install':
    case 'i':
      return 'install'
    case 'uninstall':
    case 'r':
    case 'rm':
    case 'un':
      return 'uninstall'
    case 'link':
    case 'ln':
      return 'link'
    case 'unlink':
    case 'dislink':
      return 'unlink'
    case 'install-test':
    case 'it':
      return 'install-test'
    case 'update':
    case 'up':
    case 'upgrade':
      return 'update'
    case 'list':
    case 'ls':
    case 'll':
    case 'la':
      return 'list'
    case 'rebuild':
    case 'rb':
      return 'rebuild'
    // some commands have no aliases: publish, prune
    default:
      return cmd
  }
}
