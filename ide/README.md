# Altius IDE (fork scaffold)

Altius IDE is a from-source, rebranded build of [VS Code
OSS](https://github.com/microsoft/vscode) with an Altius-owned extension
bundled in by default, turning it into an agentic Solana development
environment: fleet dispatch, guarded security scanning, and TxGuard-guided
deploys, all inside the editor rather than a separate terminal/PWA.

**Status: scaffold, not a shipped binary.** Building a full custom VS Code
distribution — Electron packaging, installers, code-signing, an update
server — is a substantial, ongoing engineering effort in its own right, well
beyond what one change can produce. What lives here is the real, reusable
part of that effort:

- a branding overlay (`product.json`) and a merge script that applies it to
  an upstream checkout, following the same pattern
  [VSCodium](https://github.com/VSCodium/vscodium) uses to de-Microsoft and
  rebrand VS Code OSS;
- a working, compilable extension (`extensions/altius-agent`) that *is* the
  actual product surface — it runs today in stock VS Code, unmodified, and
  is the thing `bootstrap.sh` bundles as built-in once you do run the fork
  build;
- a bootstrap script that clones the pinned upstream tag and wires the two
  together.

## Why an extension instead of patching VS Code's source

The agentic Solana workflows (dispatch a fleet run, review scan findings,
walk a guarded deploy) don't need forked editor internals — they need a
sidebar, a webview, a diagnostics collection, and an output channel, all of
which are stable extension APIs. Keeping that logic in
`extensions/altius-agent` means:

- it's useful immediately, in any VS Code install, without anyone building
  the fork;
- it's what the fork build bundles as a built-in (see below), so the fork
  itself only needs branding + packaging changes, not a maintained source
  patch set against upstream.

If the fork later needs true editor-level changes (a custom activity bar
default, a different welcome page), those become entries under `patches/`
applied by `bootstrap.sh` — none exist yet because nothing so far requires
touching VS Code's own source.

## Layout

```
ide/
  product.json                  Altius IDE branding overlay (name, app id, Open VSX gallery, telemetry off)
  build/
    bootstrap.sh                 clones microsoft/vscode @ pinned tag, merges product.json, bundles the extension
    merge-product-json.js        deep-merge helper used by bootstrap.sh
  patches/                       reserved for upstream source patches (empty until one is needed)
  extensions/
    altius-agent/                the bundled extension — see extensions/altius-agent/README.md
  vscode/                        gitignored; created by bootstrap.sh, not checked in
```

## Building the fork

Requires Node 20+, and enough disk/bandwidth for a shallow VS Code clone
(~500MB) plus its own toolchain.

```sh
./ide/build/bootstrap.sh
cd ide/extensions/altius-agent && npm install && npm run compile && cd -
cd ide/vscode
corepack enable && yarn install
yarn compile
./scripts/code.sh   # or scripts\code.bat on Windows
```

`ALTIUS_VSCODE_REF` overrides the pinned upstream tag (default set in
`bootstrap.sh`). Bump it deliberately, not automatically — VS Code forks
that auto-track upstream `main` inherit unreviewed churn.

## Trying the extension without the fork

The extension is the part worth using today. Open
`ide/extensions/altius-agent` in stock VS Code, `npm install`, press F5 to
launch an Extension Development Host, and the same "Altius" activity-bar
view, findings tree, and guarded deploy commands are available — see
`extensions/altius-agent/README.md`.

## Distribution (not yet built)

Turning a local `code.sh` build into something installable (per-OS
packaging, an update feed, code signing) is deliberately out of scope here;
it's a packaging/ops project, not an editor-feature one, and shouldn't block
the extension from being useful on its own.
