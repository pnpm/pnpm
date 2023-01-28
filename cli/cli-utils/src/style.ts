import chalk from 'chalk'

export const TABLE_OPTIONS = {
  border: {
    topBody: '─',
    topJoin: '┬',
    topLeft: '┌',
    topRight: '┐',

    bottomBody: '─',
    bottomJoin: '┴',
    bottomLeft: '└',
    bottomRight: '┘',

    bodyJoin: '│',
    bodyLeft: '│',
    bodyRight: '│',

    joinBody: '─',
    joinJoin: '┼',
    joinLeft: '├',
    joinRight: '┤',
  },
  columns: {},
}

for (const [key, value] of Object.entries(TABLE_OPTIONS.border)) {
  // @ts-expect-error
  TABLE_OPTIONS.border[key] = chalk.grey(value)
}
