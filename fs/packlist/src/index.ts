import Arborist from '@npmcli/arborist'
import npmPacklist from 'npm-packlist'

export async function packlist (pkgDir: string) {
  const arborist = new Arborist(({ path: pkgDir }))
  const tree = await arborist.loadActual()
  return npmPacklist(tree)
}
