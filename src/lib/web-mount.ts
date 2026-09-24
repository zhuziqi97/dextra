/** 完整 Web UI 挂载在服务端的会话路径下，桌面与普通 Web 保持原路径。 */
export function getWebMountPath(): string {
  if (typeof window === "undefined") return ""
  return (
    window.location.pathname.match(/^(.*\/client-web\/[^/]+)(?:\/|$)/)?.[1] ??
    ""
  )
}

export function webPath(path: string): string {
  const base = getWebMountPath()
  if (
    !base ||
    !path.startsWith("/") ||
    path.startsWith("//") ||
    path === base ||
    path.startsWith(base + "/")
  )
    return path
  return base + path
}

export function clientStorageKey(key: string): string {
  const clientId =
    typeof window === "undefined"
      ? undefined
      : (window as Window & { __DEXTRA_CLIENT_ID__?: string })
          .__DEXTRA_CLIENT_ID__
  return clientId ? `client:${clientId}:${key}` : key
}
