import expandTilde from './fs/expand_tilde'

export default (globalPath: string): string => expandTilde(globalPath || '~/.pnpm')
