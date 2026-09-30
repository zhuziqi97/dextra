# Internal Dextra desktop packages

Dextra is distributed internally as desktop clients. The
`internal-clients.yml` workflow builds internal packages from the candidate
branches `codex/runner-execution-platform` and
`codex/dextra-stream-flow-control`; the workflow can also be started on a
selected branch with `workflow_dispatch`. It stores GitHub
Actions artifacts only. It does not create a GitHub Release, publish a Docker
image, build standalone server artifacts, or require Apple Developer ID or
Tauri updater signing secrets.

macOS bundles use Tauri's ad-hoc signing identity (`-`). The workflow verifies
the complete mounted app signature and every packaged executable with
`codesign --verify --strict`; this is separate from checking DMG bytes and
bundle contents. Windows and Linux packages retain their existing signing policy.

The GitHub repository is public, so its Actions logs and downloadable build
artifacts are visible to people with repository read access. Internal use here
describes the intended installation audience, not artifact confidentiality.

The artifact matrix is Linux x64 `.deb` and `.AppImage`; Linux arm64 `.deb`;
macOS arm64 `.dmg`; and Windows x64 NSIS `.exe`. Each artifact
contains its package files and `build-info.json`, which records the exact source
commit, version, target triple, workflow run ID, and package filenames. Use
only packages from the same accepted source commit when assembling a Convene
download image.

The workflow checks the actual package contents for the Dextra application
identity, architecture, web resources, and the `dextra-mcp` and
`dextra-cerebro-mcp-bridge` companion binaries. A successful build or package
check alone does not certify desktop behavior. Record installation and runtime
acceptance separately for each platform.

## Download and install on macOS

Download the macOS artifact from the repository's Actions run and
unpack it. Open the `.dmg`, copy `Dextra.app` to Applications, and launch the
copied app. These internal packages have a local ad-hoc code signature, but no
Developer ID signature or Apple notarization. An ad-hoc signature is not an
Apple distribution signature and does not automatically satisfy Gatekeeper.

If macOS blocks the first launch, try opening `Dextra.app` once, then go to
**System Settings → Privacy & Security → Open Anyway** and confirm **Open**.
Apple documents this per-app exception in [Safely open apps on your
Mac](https://support.apple.com/en-au/102445). Do this only for the package whose
source commit and workflow run were approved internally; do not disable
Gatekeeper for the whole machine.

The `0.32.4-dextra.2` macOS package was built without a complete app-bundle
signature. Its DMG can match the published SHA-256 and still be reported as
damaged by macOS. For this already downloaded internal package, copy the app
to Applications and repair its local signature, verify it, and clear only this
app's download quarantine:

```sh
codesign --force --deep --sign - "/Applications/Dextra.app"
codesign --verify --deep --strict "/Applications/Dextra.app" &&
  xattr -dr com.apple.quarantine "/Applications/Dextra.app" &&
  open "/Applications/Dextra.app"
```

Do this only after matching the package's published SHA-256 and confirming the
approved source commit and run. For new ad-hoc signed packages, verify the
signature first; if the per-app Privacy & Security exception still does not
allow launch, clear this same app's quarantine and open it. A normal download
that opens without this exception requires Developer ID signing and Apple
notarization; those credentials are not configured in this internal pipeline.

When accepting a package on a macOS machine, open Dextra, pair it from Convene's
**My Clients** page, select a local directory, and verify its execution binding
and MCP authorization separately. Test a native conversation through the desktop
window and Convene's proxied workspace before marking that particular package
as runtime accepted. D10 checks the macOS packages and their contents only;
their runtime acceptance remains the D7 result for the earlier version.
