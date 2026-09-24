export const DEXTRA_WS_PROTOCOL = "codeg-events"
const DEXTRA_WS_TOKEN_PROTOCOL_PREFIX = "codeg-token."

function base64UrlEncode(value: string): string {
  const bytes = new TextEncoder().encode(value)
  let binary = ""
  for (const byte of bytes) {
    binary += String.fromCharCode(byte)
  }
  return btoa(binary)
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/g, "")
}

export function buildDextraWebSocketProtocols(token: string): string[] {
  const trimmed = token.trim()
  if (!trimmed) return [DEXTRA_WS_PROTOCOL]
  return [
    DEXTRA_WS_PROTOCOL,
    `${DEXTRA_WS_TOKEN_PROTOCOL_PREFIX}${base64UrlEncode(trimmed)}`,
  ]
}
