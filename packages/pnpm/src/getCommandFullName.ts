export default function getCommandFullName (cmd: string) {
  switch (cmd) {
    case 'install':
    case 'i':
      return 'install'
    case 'r':
    case 'remove':
    case 'rm':
    case 'un':
    case 'uninstall':
      return 'remove'
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
    case 'test':
    case 't':
    case 'tst':
      return 'test'
    case 'run':
    case 'run-script':
      return 'run'
    case 'recursive':
    case 'multi':
    case 'm':
      return 'recursive'
    // some commands have no aliases: publish, prune, add, why
    default:
      return cmd
  }
}
