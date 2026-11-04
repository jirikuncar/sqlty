// A playground session in the URL hash: its JSON, deflated, as base64url. No DOM, so
// tests/playground.test.mjs runs it too.

// Most browsers and chat apps keep links up to this length.
export const MAX_LINK = 8000;

export async function encode(session) {
  const bytes = new TextEncoder().encode(JSON.stringify(session));
  const deflated = await new Response(
    new Blob([bytes]).stream().pipeThrough(new CompressionStream("deflate")),
  ).arrayBuffer();
  const binary = Array.from(new Uint8Array(deflated), (byte) => String.fromCharCode(byte)).join("");
  return btoa(binary).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/, "");
}

// The session of a hash `encode` made, or null for any other.
export async function decode(hash) {
  try {
    const binary = atob(hash.replaceAll("-", "+").replaceAll("_", "/"));
    const bytes = Uint8Array.from(binary, (char) => char.charCodeAt(0));
    const inflated = new Blob([bytes]).stream().pipeThrough(new DecompressionStream("deflate"));
    const session = JSON.parse(await new Response(inflated).text());
    return session && typeof session === "object" ? session : null;
  } catch {
    return null;
  }
}
