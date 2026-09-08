"use client"

import { useEffect } from "react"
import { useRouter } from "@/lib/navigation"

export default function SettingsPage() {
  const router = useRouter()

  useEffect(() => {
    router.replace("/settings/appearance")
  }, [router])

  return null
}
