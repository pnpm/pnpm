import expandTilde from './fs/expandTilde'

export default (globalPath: string): string => expandTilde(globalPath || '~/.pnpm')
