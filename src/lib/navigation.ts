"use client"

import { useMemo } from "react"
import {
  usePathname as useNextPathname,
  useRouter as useNextRouter,
} from "next/navigation"
import { getWebMountPath, webPath } from "./web-mount"

export { useSearchParams } from "next/navigation"

/** 挂载页面通过相同静态导出路由导航，Next 的本地路由树不接收代理前缀。 */
export function useRouter() {
  const router = useNextRouter()
  return useMemo(() => {
    if (!getWebMountPath()) return router
    return {
      ...router,
      push: (href: string) => window.location.assign(webPath(href)),
      replace: (href: string) => window.location.replace(webPath(href)),
      refresh: () => window.location.reload(),
      prefetch: () => {},
    }
  }, [router])
}

export function usePathname() {
  const pathname = useNextPathname()
  const prefix = getWebMountPath()
  return prefix && pathname.startsWith(prefix)
    ? pathname.slice(prefix.length) || "/"
    : pathname
}
