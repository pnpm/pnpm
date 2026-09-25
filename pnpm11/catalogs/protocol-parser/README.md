# @pnpm/catalogs.protocol-parser

> Parse catalog protocol specifiers and return the catalog name.

Catalog protocol specifiers start with `catalog:`. The parser reads the value after that prefix as the catalog name, and returns `default` when no name is provided.

## Examples

- `catalog:foo` -> `"foo"`
- `catalog:` -> `"default"`
