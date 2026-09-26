import { nerfDart } from '@pnpm/config.registry-auth-key'

function getMaxParts (uris: string[]) {
  return uris.reduce((max, uri) => {
    const parts = uri.split('/').length;
    return parts > max ? parts : max;
  }, 0);
}

export function pickSettingByUrl<T> (
  generic: { [key: string]: T } | undefined,
  uri: string
): T | undefined {
  if (!generic) return undefined;
  if (generic[uri]) return generic[uri];
  /* const { nerf, withoutPort } = parseUri(uri); */
  const nerf = nerfDart(uri);
  const withoutPort = removePort(new URL(uri));
  if (generic[nerf]) return generic[nerf];
  if (generic[withoutPort]) return generic[withoutPort];
  const maxParts = getMaxParts(Object.keys(generic));
  const parts = nerf.split('/');
  for (let i = Math.min(parts.length, maxParts) - 1; i >= 3; i--) {
    const key = `${parts.slice(0, i).join('/')}/`;
    if (generic[key]) {
      return generic[key];
    }
  }
  if (withoutPort !== uri) {
    return pickSettingByUrl(generic, withoutPort);
  }
  return undefined;
}

function removePort (config: URL): string {
  if (config.port === '') return config.href;
  config.port = '';
  const res = config.toString();
  return res.endsWith('/') ? res : `${res}/`;
}
