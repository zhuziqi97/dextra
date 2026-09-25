# Internal Dextra desktop packages

Dextra is distributed internally as desktop clients. The
`internal-clients.yml` workflow builds unsigned packages from the candidate
branch `codex/runner-execution-platform`; after that branch is merged to `main`,
the same workflow can be started with `workflow_dispatch`. It stores GitHub
Actions artifacts only. It does not create a GitHub Release, publish a Docker
image, build standalone server artifacts, or require Apple Developer ID or
Tauri updater signing secrets.

The artifact matrix is Linux x64 `.deb`, `.rpm`, `.AppImage`; Linux arm64 `.deb`,
`.rpm`; macOS x64 and arm64 `.dmg`; and Windows x64 NSIS `.exe`. Each artifact
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

Download the macOS artifact from the private repository's Actions run and
unpack it. Open the `.dmg`, copy `Dextra.app` to Applications, and launch the
copied app. These internal packages have no Developer ID signature or Apple
notarization. A local ad-hoc code signature, when present, is not an Apple
distribution signature.

If macOS blocks the first launch, try opening `Dextra.app` once, then go to
**System Settings → Privacy & Security → Open Anyway** and confirm **Open**.
Apple documents this per-app exception in [Safely open apps on your
Mac](https://support.apple.com/en-au/102445). Do this only for the package whose
source commit and workflow run were approved internally; do not disable
Gatekeeper for the whole machine.

After installation, open Dextra, pair it from Convene's **My Clients** page,
select a local directory, and verify its execution binding and MCP authorization
separately. Test a native conversation through the desktop window and Convene's
proxied workspace before marking that particular package as accepted.
