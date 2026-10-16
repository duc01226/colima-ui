# Windows qualification procedure

Read this when building a Windows candidate or collecting Windows evidence. The canonical requirements and all 38 cases remain in `docs/specs/windows-docker-ui.md`; the six-phase delivery plan and G1–G25 remain in `plans/261014-1121-windows-docker-ui/plan.md`. This guide describes the authored tools, not a completed Windows qualification. Native Windows execution, signatures, installed behavior and both providers are pending until their actual evidence is reviewed.

## Prerequisites and boundaries

The supported target being prepared is Windows 11 x64 with Linux containers on existing Docker Desktop and Rancher Desktop in Moby mode. Neither the app nor these runners install, start provisioning, or replace a container engine. Use a dedicated, empty qualification engine and disposable ordinary Windows accounts. Prepare both providers separately and retain their distinct daemon IDs and provider/version provenance. Never point a destructive fixture at a personal or shared engine.

The platform-neutral commands below run through Node, npm and Cargo on Windows/macOS/Linux; their native checks explicitly reject an unsupported host. Use an x64 Visual Studio 2022 developer environment on Windows with MSVC toolset family 14.44 and Windows SDK 10.0.26100.0. Read `scripts/windows-toolchain.json` for the servicing policy. Node 24.21.0, npm 11.19.0 and Rust 1.99.0 are pinned. MSVC servicing patches within that family are allowed; build receipts record the actual developer environment and compiler/linker hashes. This is not an exact vendor-patch pin.

Install the existing project dependencies with `npm ci`. That runs the repository's hook setup; Git for Windows supplies the shell used by the hooks. Native Tauri prerequisites include the C++ development tools and WebView2. Hosted Windows Server CI can prove its own compilation/tests/packaging, but does not replace Windows 11 workstation qualification. An isolated selected-module cross-check on macOS does not prove a complete Windows application build.

Read `docs/integration-test-reference.md` before selecting live tests. Existing ignored Unix Docker suites are separate evidence and may require Unix tools. The Windows runners below select exact tests, reject missing/current-run receipts, and never count zero selected tests or a prerequisite skip as qualification.

## Build and package

Run from the repository root in the prepared Windows developer environment:

```text
npm run build:windows
```

Candidate mode builds a per-user NSIS installer and deliberately removes ambient updater private-key inputs. It does not produce signed updater proof. The tool writes `tmp/reports/windows-build/build.json` only after a successful fresh build and records source, app version, exact artifact hashes, tools and pending qualification. It invokes the installed Tauri CLI with literal arguments, `--ci` and locked Cargo resolution.

For signed qualification use an already-arranged Authenticode certificate in the Windows certificate store and the existing updater key corresponding to the repository public key. Supply `WINDOWS_CERTIFICATE_THUMBPRINT`, `WINDOWS_TIMESTAMP_URL`, `TAURI_SIGNING_PRIVATE_KEY` and, if needed, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` through the operator's protected environment. Never paste their values into a report, command transcript or repository. Then run:

```text
npm run build:windows:signed
```

This verifies timestamped Authenticode on the application and installer and retains the updater `.sig` artifact. It creates no certificate, account or key pair and publishes nothing. A signed build receipt is necessary but insufficient: actual updater verification and installed-version read-back are separate tests. `.github/workflows/windows-candidate.yml` has explicit candidate/signed dispatch; signing requires the provisioned runner/environment. The workflow cannot supply missing credentials or native proof by inference.

## Dedicated provider boundary tier

Prepare a fixture image explicitly before the test. `src-tauri/tests/fixtures/provider/Dockerfile` requires an already-local immutable base, `/bin/sh`, `sleep`, `tar`, `sha256sum`, `stty` and ordinary shell utilities used by the assertions. Use the declared fixture label and retain its exact local content ID. The contract run does not pull or implicitly build images. Do not remove foreign images to satisfy a prerequisite.

Inspect the chosen context and daemon through literal context selectors, without changing the global context. Substitute the actual values in this command; placeholders are not runnable evidence:

```text
node scripts/windows-qualify.mjs --provider docker-desktop --context desktop-linux --engine-id <actual-id> --fixture-image <already-local-tag> --fixture-id sha256:<actual-64-hex> --dedicated-engine <same-actual-id> --profile disposable-account
```

Repeat for `--provider rancher-moby` and that provider's context/daemon/image. The runner requires native Windows 11, enforces the explicit dedicated-engine contract, captures source/tool identity, and selects `windows_provider_contract::native::native_provider_core_contract`. The shared fixture checks the daemon before operations, allocates unique run-owned resources and removes only its own resources. Inspect failed cleanup before another run; never use prune or an engine reset as routine cleanup.

The actual app-library assertions cover Docker resources, Unicode Compose, hash/TAR copy, image export/import, ConPTY text/resize/interrupt/exit, terminal ownership and metrics attribution.

Image-import restoration requires the pre-existing fixture content to retain at least one original tag. The contract exports a unique run-owned tag, removes only that tag without force, proves its absence while the original tags/content and running fixture container remain, then imports and proves that the missing tag/content is restored. A no-op import cannot satisfy this arrangement (`src-tauri/tests/native/windows_provider_contract.rs:90`, `src-tauri/tests/native/windows_provider_contract.rs:262`). Its receipt is under `tmp/reports/windows-native/<run-id>/boundary.json`. This tier calls application owners directly. It does not prove installed Tauri IPC, WebView2 rendering, keyboard/NVDA behavior, installer/update lifecycle, second-account denial or every canonical case variant.

## Installed UI and preservation cases

Execute every applicable arrangement, action, failure and boundary variant in the canonical cases on both providers. Record the installed executable hash/version, exact context and original engine ID. Keep an external resource/data sentinel with an independent before/after digest. A successful empty list cannot establish connection readiness or preservation.

Exercise setup with missing CLI, stopped engine, incompatible target, incomplete SDK data and available/empty resources. Verify active connection, checked candidate and saved next-launch preference remain distinct. Saving must not retarget requests, events, jobs or terminals. Restart must refuse unfinished transfers unless they finish or are explicitly cancelled and cleaned up; close terminals before exit. Verify reconnect only to the original daemon and reject stale reads/events after loss/recovery. Keep external host CLI/context changes active as hostile controls.

Walk container/Compose/resource actions, overwrite/no-overwrite transfers, Unicode/space/drive/UNC paths, cancellation/publication races, SBOM destination conflict and cleanup. Trivy is optional; if testing SBOM, arrange its native executable/database separately and record that prerequisite. For interactive terminals use the installed UI to verify input, UTF-8, resize, Ctrl-C, Escape/tab ownership, exit, close, late acquisition and no orphan process. Verify Windows unsupported VM/Kubernetes/runtime-install operations are refused through raw IPC/HTTP, AI tools and automation as well as hidden in navigation.

Verify keyboard focus and return, required zoom/reflow, NVDA labels/status and all shipped locales against the accepted design/canonical cases. DOM fixtures, screenshots and source sensors have separate limits; none substitutes for these native observations. Preserve macOS and Linux flows under TC-WINUI-020, including their existing runtime management behavior.

For installer/lifecycle cases, use an ordinary account and actual NSIS installer. Cover a fresh WebView2 prerequisite, offline/unavailable bootstrap recovery, paths containing spaces/Unicode, launch, ordinary uninstall, reinstall and upgrade. Verify the exact running installed version and persistence of settings/history. Inspect the actual generated installer behavior; do not infer preservation from a development template. Do not select an explicit delete-app-data option when proving ordinary uninstall preservation. Before and after each operation check external engines, resources, credentials and data sentinels. Install/update operations are performed only in the arranged disposable profile.

## Private reference and second-account controls

Use the actual private-reference filename and exact API port for the running instance; never search neighboring health ports or copy the token into evidence. Obtain the owner's nonsecret SID through Windows account tooling. The script reads credentials only in memory and writes nonsecret observations. Run the following from Windows PowerShell as the ordinary owner, then use a distinct ordinary non-administrator outsider account with the same reference path, owner SID, port and run UUID:

```powershell
powershell.exe -NoProfile -NonInteractive -File scripts/windows-account-check.ps1 -PrivateReference <actual-path> -ExpectedOwnerSid <owner-sid> -Role owner -ApiPort <exact-port> -RunId <same-uuid> -OutFile tmp/account-owner.json
powershell.exe -NoProfile -NonInteractive -File scripts/windows-account-check.ps1 -PrivateReference <same-path> -ExpectedOwnerSid <same-owner-sid> -Role outsider -ApiPort <same-port> -RunId <same-uuid> -OutFile tmp/account-outsider.json
```

Use an arranged writable receipt location inside the checkout's `tmp/` for each account; do not grant outsider access to the private app directory to make the test run. The outsider must observe `UnauthorizedAccess`, not an absent file, plus public health 200 and unauthenticated protected API 401; the owner must read the reference and receive protected API 200. Also execute the canonical privacy-creation failure, malformed/stale-reference, exact-instance and in-app-access controls. The two-account helper alone does not prove all ACL/reparse/hard-link cases.

## Installed monitoring cost and recovery

Run independent 20- and 200-container experiments for each provider, with the installed release executable matching the current build receipt. Close existing app instances before starting. Use the same provider options as above and add:

```text
node scripts/windows-metrics.mjs --app <actual-installed-colima-ui.exe> --build-receipt tmp/reports/windows-build/build.json --count 20 --provider docker-desktop --context desktop-linux --engine-id <actual-id> --fixture-image <local-tag> --fixture-id sha256:<actual-64-hex> --dedicated-engine <same-actual-id> --profile disposable-account
```

Repeat with count 200 and with Rancher Moby (four independent runs). Follow the interactive prompts: allow the empty closed-monitoring baseline, open the installed Activity view, stop the external original engine, restore that same engine, and close the view. No replacement engine is allowed. The observer reads authenticated collector status without opening a subscription. The app and all owned helper descendants are measured through their Windows Job; fixture/observer CLI work stays outside that Job. Capture CPU including exited descendants and live resident memory continuously at one-second cadence, starting before fleet creation.

Each run requires at least 60 seconds of empty ready baseline, a full 60-second normal window and a full 60-second recovered window with complete fleet samples and consecutive completed ticks. Defaults remain 2,000 ms; p95 tick duration must be below 2,000 ms. App-plus-child CPU must be strictly below 2% of one logical core at 20 and 5% at 200. At 200, continuous loaded peak minus the same-run pre-fleet baseline maximum must be strictly below 268,435,456 bytes. Raw samples, missing-process/cadence errors and cleanup outcomes remain in the run evidence; missing samples do not become zeros. Closing the view must stop collection. `metrics.json` validation checks arithmetic and exact app/source identity; it cannot turn missing measurements into PASS.

## Seven controlled updater variants

TC-WINUI-023 requires independent newer, same, older, malformed, tampered, wrong-signature and interrupted cases. Use the existing updater key/public key throughout. The helper prepares source copies changing only application version and the controlled feed endpoint; the main checkout remains untouched:

```text
node scripts/windows-update-fixtures.mjs tmp/update-fixtures https://127.0.0.1:17420/update
```

It creates source workspaces for 0.9.0, 1.0.0 and 1.1.0 and local Git metadata only inside those task-created copies, without staging, committing or adding remotes. In each workspace run `npm ci`, then the native signed build above. Retain its original `tmp/reports/windows-build/build.json` and declared artifact paths. Do not hand-edit version carriers or substitute a different updater public key. The regular source hash stays strict; an additional fixture-family hash normalizes only application versions and the feed endpoint for comparing these copies.

Arrange a certificate/key already trusted for TLS loopback by the Windows test profile. No helper creates a signing pair or disables TLS validation. Start one feed variant from the main checkout:

```text
node scripts/windows-update-feed.mjs --variant newer --newer tmp/update-fixtures/1.1.0/workspace/tmp/reports/windows-build/build.json --baseline tmp/update-fixtures/1.0.0/workspace/tmp/reports/windows-build/build.json --older tmp/update-fixtures/0.9.0/workspace/tmp/reports/windows-build/build.json --tls-cert <provided-loopback-cert> --tls-key <provided-loopback-key> --port 17420
```

Reset the disposable app installation to the known 1.0.0 baseline before **each** variant, preserving and hashing arranged app state and external sentinels. Stop the previous feed before choosing another `--variant`. Newer must install/read back 1.1.0; same and older must offer no update and read back 1.0.0; malformed, tampered, wrong-signature and interrupted must fail and leave usable 1.0.0 after relaunch. The helper changes the offered version, response bytes/signature or transport only for the selected defect. Request logs prove delivery only. Record actual installed version, usable relaunch and state/engine/data preservation independently; HTTP success is never updater acceptance.

## Evidence bundle and review

Generate a fresh pending worksheet from the exact signed primary build:

```text
node scripts/windows-evidence.mjs init tmp/windows-evidence tmp/reports/windows-build/build.json
```

The worksheet derives all 38 identities from the canonical owner: 37 native cases per provider, shared preservation case on macOS and Linux, and G1–G25. Fill actual arrangements/actions/observations, reviewer and dates. Copy nonsecret artifacts inside this bundle and reference exact SHA256 values; raw private references, tokens and certificate/key containers are refused. Include both provider provenances, boundary receipts, each provider's two metrics runs, paired account receipts and separate installed read-back evidence for all seven update variants. Cases require positive and boundary controls, not just a completion screenshot.

The gate validator requires PASS with bound evidence for G1–G21 and G24; G24 covers generated project context/mirror verification. G22, G23 and G25 retain the plan’s explicit non-applicability conditions. Structural gate validation does not establish that mirror verification or native qualification ran (`scripts/windows-evidence.mjs:52`, `plans/261014-1121-windows-docker-ui/plan.md:120`).

After the actual observations and native signatures exist, run on Windows 11:

```text
node scripts/windows-evidence.mjs verify tmp/windows-evidence
```

This checks source/artifact/case bindings, required receipt structure and current Authenticode; its result explicitly remains **structural validation**. A reviewer must inspect every canonical variant and artifact before accepting behavior. A JSON `PASS` field alone is not proof. Publication/release approval is separate and is not granted by these tools or this implementation task.

## Local quality and fault sensitivity

The current fail-on-new policy records exact inherited GTK/Unicode maintenance/soundness debt and existing `html2md`/`elkjs` license expressions in `scripts/windows-quality-baseline.json`. These entries do not fix the debt or authorize distribution. Maintainers must resolve applicable distribution/notice obligations before publication, separately from native qualification. Patchable or newly exposed dependency defects must be repaired rather than added to this unrelated-legacy baseline. Keep fresh metadata and the pinned tool; the Cargo-dispatched and direct cargo-audit banners have distinct exact forms (`scripts/check-dependencies.mjs:9`, `.cargo/audit.toml:16`).

After all implementation and the whole-change static review, the final verifier runs the full affected suites, standard frontend checks, locked Rust library tests/Clippy, `npm run check:windows` and `npm run test:tooling`. Foundation checks cover owned Rust formatting, native/shared boundary direction, accidental credential patterns, pinned/version/config/CI drift. They scan test files too; `scripts/windows-quality-baseline.json` has explicit reviewed exceptions, not a wildcard suppression. `npm run check:dependencies` requires cargo-audit 0.22.2 and evaluates current npm/Cargo advisories and dependency metadata. Network/prerequisite failure remains a failed required check. Any necessary legacy exception names the exact finding, rationale, owner and review date; never auto-accept the dependency report.

`npm run measure:build` records warmed and unchanged frontend/Rust library builds with literal locked commands and per-command logs. Cargo no-op rebuilding is detected from compiler artifacts; the frontend build is recorded as full bundling rather than claimed cached. It does not measure an installed Windows build on another host.

The final mutation owner must reconcile every changed core rule with an intent-sensitive test, then create a final-green baseline receipt with `result: PASS` and the exact `sourceIdentity` hash. Execute the reviewed portable inventory and, separately on Windows, the native inventory:

```text
node scripts/fault-seed.mjs tests/windows-mutations.json tmp/reports/<actual-final-green-baseline>.json
node scripts/fault-seed.mjs tests/windows-native-mutations.json tmp/reports/<actual-final-green-baseline>.json
```

The declared inventory contains 100 portable and 6 native seeds. Listed seeds are not an exhaustive coverage claim. The runner saves exact source bytes, applies one declared defect, requires an actual failed test (compiler errors/timeouts are invalid evidence), and restores/compares the bytes before advancing. A survivor requires investigation and a killing test, followed by the owning review/verify loop. If interrupted, inspect `tmp/fault-seed/active.json` and its backup/state first; `node scripts/fault-seed.mjs --recover` refuses a live owner or conflicting source. Never use Git reset/checkout/restore/stash to restore a mutant. Re-run the affected green baseline after recovery.

Keep all raw results under `tmp/`. Read the canonical cases and plan before accepting evidence. Authored tooling, a successful compile, a hosted Server lane and a structurally valid bundle each have explicit limits; native Windows 11, both providers, installed lifecycle and preservation remain required.

Source owners: toolchain/pins `scripts/toolchain.mjs:6`, `scripts/windows-toolchain.json`; package/signature `scripts/windows-build.mjs:9`, `scripts/windows-signatures.ps1:1`; provider `scripts/windows-qualify.mjs:20`, `src-tauri/tests/support/docker_fixture.rs:159`; installed cost `scripts/windows-metrics.mjs`, `src-tauri/tests/native/windows_metrics_cost.rs:76`, `src-tauri/tests/support/metrics_budget.rs:4`; account/update/evidence `scripts/windows-account-check.ps1:1`, `scripts/windows-update-fixtures.mjs:10`, `scripts/windows-update-feed.mjs:36`, `scripts/windows-evidence.mjs:63`; quality/recovery `scripts/check-windows-foundation.mjs:59`, `scripts/check-dependencies.mjs:36`, `scripts/measure-build.mjs:10`, `scripts/fault-seed.mjs:59`.
