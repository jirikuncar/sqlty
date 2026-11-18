// Released engines, from the @sqlty/wasm tarballs on the npm registry, checked against the
// integrity the registry lists. The playground runs its own build otherwise: that of main.
const REGISTRY = "https://registry.npmjs.org/@sqlty/wasm";
const RELEASE = /^\d+\.\d+\.\d+$/;

// The released versions, newest first.
export async function versions() {
  const found = await json(REGISTRY);
  return Object.values(found.versions)
    .filter(({ version, deprecated }) => RELEASE.test(version) && !deprecated)
    .map(({ version }) => version)
    .sort((a, b) => b.localeCompare(a, "en", { numeric: true }));
}

// The bytes of `sqlty_wasm.wasm` of the release `version`.
export async function released(version) {
  if (!RELEASE.test(version)) throw new Error(`${version} is not a release`);
  const { dist } = await json(`${REGISTRY}/${version}`);
  const response = await fetch(dist.tarball);
  if (!response.ok) throw new Error(`${dist.tarball}: ${response.status}`);
  const tarball = await response.arrayBuffer();
  const digest = await crypto.subtle.digest("SHA-512", tarball);
  if (`sha512-${base64(digest)}` !== dist.integrity) throw new Error(`@sqlty/wasm@${version} is not the published tarball`);
  const unzipped = new Blob([tarball]).stream().pipeThrough(new DecompressionStream("gzip"));
  return entry(new Uint8Array(await new Response(unzipped).arrayBuffer()), "package/sqlty_wasm.wasm");
}

// The file `name` of a tar archive.
export function entry(tar, name) {
  const field = (header, start, length) => new TextDecoder().decode(header.subarray(start, start + length)).replace(/\0.*$/s, "");
  for (let at = 0; at + 512 <= tar.length; ) {
    const header = tar.subarray(at, at + 512);
    const prefix = field(header, 345, 155);
    const path = (prefix ? `${prefix}/` : "") + field(header, 0, 100);
    const size = parseInt(field(header, 124, 12), 8);
    if (path === name) return tar.slice(at + 512, at + 512 + size);
    if (!path) break;
    at += 512 + Math.ceil(size / 512) * 512;
  }
  throw new Error(`${name} is not in the package`);
}

async function json(url) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url}: ${response.status}`);
  return response.json();
}

function base64(buffer) {
  return btoa(String.fromCharCode(...new Uint8Array(buffer)));
}
