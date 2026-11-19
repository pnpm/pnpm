declare module '@zkochan/js-yaml' {
  export function dump (value: object, options: import('js-yaml').DumpOptions & { blankLines?: boolean }): string
}
