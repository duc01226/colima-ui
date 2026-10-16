<p align="center">
  <img src="src-tauri/icons/128x128@2x.png" width="128" height="128" alt="ColimaUI" />
</p>

<h1 align="center">ColimaUI</h1>

<p align="center">
  A modern desktop & web GUI for <a href="https://github.com/abiosoft/colima">Colima</a> — manage Docker containers, Kubernetes clusters, Linux VMs, and AI models from a beautiful dark-themed interface.
</p>

<p align="center">
  <a href="https://github.com/vnknowledge2014/colima-ui/releases/latest"><img src="https://img.shields.io/github/v/release/vnknowledge2014/colima-ui?style=flat-square&color=blue" alt="Latest Release" /></a>
  <a href="https://github.com/vnknowledge2014/colima-ui/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/vnknowledge2014/colima-ui/ci.yml?branch=main&style=flat-square&label=CI" alt="CI" /></a>
  <img src="https://img.shields.io/badge/Tauri-v2-orange?style=flat-square" alt="Tauri v2" />
  <img src="https://img.shields.io/badge/Svelte-5-ff3e00?style=flat-square" alt="Svelte 5" />
  <img src="https://img.shields.io/badge/Rust-2021-dea584?style=flat-square" alt="Rust" />
  <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux-lightgrey?style=flat-square" alt="macOS & Linux" />
  <a href="https://github.com/vnknowledge2014/colima-ui/blob/main/LICENSE"><img src="https://img.shields.io/github/license/vnknowledge2014/colima-ui?style=flat-square" alt="License" /></a>
</p>

---

## 📥 Download

Grab the latest release from [**GitHub Releases**](https://github.com/vnknowledge2014/colima-ui/releases/latest):

| Platform | File | Architecture |
|----------|------|--------------| 
| **macOS** (Apple Silicon) | `ColimaUI_*_aarch64.dmg` | M1 / M2 / M3 / M4 |
| **macOS** (Intel) | `ColimaUI_*_x64.dmg` | x86_64 |
| **Linux** (Debian/Ubuntu) | `ColimaUI_*_amd64.deb` | x86_64 |
| **Linux** (Universal) | `ColimaUI_*_amd64.AppImage` | x86_64 |

> **macOS Gatekeeper:** Pre-built DMGs are currently unsigned. If macOS shows
> *"ColimaUI.app is damaged and can't be opened"*, open **System Settings →
> Privacy & Security** and click **Open Anyway**, or run
> `xattr -cr /Applications/ColimaUI.app` after dragging the app to Applications.
> This removes the download quarantine flag and is only needed for the
> downloaded build, not for locally compiled apps.

> **Browser development:** `npm run dev` serves the frontend at `http://localhost:1420`. The packaged localhost API binds an available port in 11420–11429 and does not serve frontend assets. Browser credentials are handed off out of band; there is no HTTP token endpoint (`src-tauri/src/api_server.rs:66`, `vite.config.ts:43`).

Windows 11 x64 support is in preparation for existing Docker Desktop and Rancher Desktop Moby engines with Linux containers. Candidate/signing tools are authored; native qualification and a Windows release remain pending. See the [Windows qualification procedure](docs/windows-qualification.md). Windows does not install/provision an engine or offer VM/Kubernetes/runtime management (`src-tauri/src/host_features.rs:16`).

---

## ✨ Features

### 🐳 Docker Management
- **Containers** — List, start, stop, restart, pause, unpause, remove, rename, inspect, exec, view real-time stats and logs
- **Images** — Pull, remove, prune, inspect, tag with batch operations
- **Volumes** — Create, inspect, remove, prune
- **Networks** — Create (bridge/overlay/macvlan/host), inspect, remove, prune
- **Docker Compose** — List projects, view services, restart/stop projects, view logs

### ☸️ Kubernetes
- **Multi-resource browser** — Pods, Deployments, Services, ConfigMaps, Secrets, StatefulSets, DaemonSets, Jobs, CronJobs, Ingresses, PVCs, Namespaces, Nodes, Events
- **CRD discovery** — Dynamically browse custom resources in the sidebar
- **Resource actions** — Describe, view/edit YAML, view logs, delete, restart, scale
- **Real-time log streaming** — Follow pod logs via SSE with auto-scroll
- **HTTP Benchmark** — Benchmark K8s services with configurable concurrency and latency statistics (p50/p95/p99)
- **Port forwarding** — Create and manage port forwards to services and pods
- **Exec** — Shell into pod containers
- **Cluster health** — Node status, resource pressure, component health
- **Context switching** — Switch between multiple kubeconfig contexts
- **Kind clusters** — Create and manage [Kind](https://kind.sigs.k8s.io/) clusters

### 🖥️ Linux VMs (Lima)
- Create VMs from templates with custom CPU, memory, and disk settings
- Start, stop, delete VMs; shell into VMs directly

### 🤖 AI Diagnostic Agent
- **Self-learning AI** — Expert-level troubleshooting for Colima/Lima/Docker/Kubernetes errors
- **5-tool agent loop** — Web search, page fetch, diagnostic log collection, safe command execution, user-approved commands
- **Knowledge Bank** — SQLite-backed memory (Windows native LocalAppData/`ColimaUI/knowledge.db`; macOS/Linux `~/.colima-ui/knowledge.db`) with 22+ builtin solutions, user feedback via 👍/👎
- **3-tier command sandbox** — Safe (auto-run), Approve (user click), Banned (Rust-level block). Prevents destructive commands even if AI hallucinates
- **Multi-provider AI Chat** — Anthropic, OpenAI, Google, Ollama, OpenRouter, Groq, Together AI, Mistral, DeepSeek
- **AI Models** — Pull, delete, and serve Ollama models directly

### 📊 Dashboard & UI
- System overview with running/stopped counts, CPU/memory allocation
- Integrated xterm.js terminal with SSH into instances on supported hosts and native Docker ContainerExec sessions
- Setup Wizard & Getting Started Tour for new users
- Dark theme with glassmorphism effects and micro-animations
- Context menus, keyboard shortcuts, global toast notifications
- i18n support (English, Japanese, Vietnamese, Chinese)

---

## 🚀 Getting Started

### Prerequisites

- **Node.js 24.21.0, npm 11.19.0 and Rust 1.99.0** — contributor pins in `.node-version`, `package.json` and `rust-toolchain.toml`; the package retains Node ≥22.22.2 as its minimum engine constraint.
- **Docker CLI and an existing engine** — Windows preparation targets Docker Desktop or Rancher Desktop Moby with Linux containers. Other hosts retain their existing Docker/Colima prerequisites.
- **Colima** — for Colima VM management on supported hosts.
- **Native Tauri prerequisites** — host C++/platform libraries; Windows uses the declared Visual Studio 2022/MSVC/SDK policy and WebView2 in the [qualification guide](docs/windows-qualification.md).
- **Git for Windows** — supplies the shell for the existing POSIX Git hooks on Windows.
- **kubectl, Kind and Ollama** *(optional on supported hosts)* — their feature prerequisites do not authorize unsupported Windows operations.

### Development

```bash
git clone https://github.com/vnknowledge2014/colima-ui.git
cd colima-ui
npm ci

# Desktop app (Tauri native window + Vite dev server)
npm run tauri dev

# Web-only (frontend at http://localhost:1420)
npm run dev
```

### Lint & Quality Gates

Use the pinned contributor toolchain and run the repository gates before committing or pushing:

```bash
npm run lint
npm run typecheck
npm run check
npm run check:tokens
npm run check:i18n
npm test
npm run test:tooling
npm run check:windows
npm run lint:rust
```

The npm pre-push hook runs these gates; its explicit skip branch means hook installation alone is not evidence of execution. CI additionally runs locked native Rust library tests on macOS/Linux/hosted Windows and dependency-health checks. Live ignored Docker/Trivy suites need their stated prerequisites and deliberate selection; see [Boundary testing](docs/integration-test-reference.md). Hosted Windows Server checks do not qualify Windows 11 installed behavior.

`git push --no-verify` / `SKIP_LINT_GATES=1` are emergency-only and need explicit approval.

### Production Build

```bash
# Desktop app → src-tauri/target/release/bundle/
npm run tauri build

# Web-only → dist/
npm run build
```

For Windows per-user NSIS candidates, real signature checks, dedicated provider fixtures, installed metrics/account/update controls and evidence limits, follow [Windows qualification](docs/windows-qualification.md). A successful compile or candidate receipt is preparation evidence.

---

## 🛠️ Tech Stack

| Layer | Technology | Version |
|-------|-----------|---------|
| **Frontend** | Svelte 5 (Runes) | 5.56 |
| **Language** | TypeScript | ~6.0.3 |
| **Bundler** | Vite | 8.x |
| **Desktop** | Tauri | 2.x |
| **Backend** | Rust (Edition 2021) | — |
| **HTTP Server** | Axum | 0.8 |
| **Docker Client** | Bollard | 0.21 |
| **Terminal** | xterm.js | 6.0 |
| **Database** | SQLite (rusqlite) | — |
| **Styling** | Vanilla CSS (dark theme) | — |

---

## 📁 Project Structure

```
colima-ui/
├── src/                          # Frontend (Svelte 5 + TypeScript)
│   ├── App.svelte                # Main shell, sidebar, event listeners
│   ├── main.ts                   # Entry point
│   ├── pages/                    # 16 page components
│   ├── components/               # 8 shared components
│   ├── store/                    # Svelte 5 rune-based state
│   ├── lib/                      # API layer, AI bus, i18n, utilities
│   ├── styles/                   # CSS design system (tokens, reset, layout, ...)
│   └── locales/                  # i18n (en, ja, vi, zh)
├── src-tauri/                    # Backend (Rust + Tauri)
│   ├── src/
│   │   ├── lib.rs                # App setup & command registration
│   │   ├── api_server.rs         # Axum localhost API (11420–11429)
│   │   ├── commands/             # 14 command modules (business logic)
│   │   ├── routes/               # 14 HTTP route modules (thin delegation)
│   │   ├── adapters/             # Unified DevOps adapter traits
│   │   ├── services/             # High-level service layer
│   │   ├── auth.rs               # API token & auth middleware
│   │   ├── sse.rs                # SSE broadcast & event watchers
│   │   ├── validation.rs         # Security validation (shell injection, etc.)
│   │   ├── helpers.rs            # Response wrappers, CLI runner, caching
│   │   ├── platform.rs           # OS/arch/package manager detection
│   │   ├── docker_state.rs       # Bollard event stream for push updates
│   │   ├── instance_reader.rs    # Colima config YAML parser
│   │   ├── terminal_session.rs   # PTY session management
│   │   └── poller.rs             # Background instance poller
│   └── tauri.conf.json           # Tauri configuration
├── scripts/release.sh            # Automated version bump + tag + push
├── docs/                         # Technical documentation
└── external_skill/               # AI orchestration skill for external agents
```

> See [`docs/`](docs/) for detailed technical documentation.

---

## 🔄 CI/CD & Releases

- **CI** (`.github/workflows/ci.yml`) — frontend/tooling/foundation checks, locked all-target/all-feature Rust check/Clippy on macOS/Linux/hosted Windows, native library tests and dependency health.
- **Release** (`.github/workflows/release.yml`) — builds DMG/deb/AppImage on version tags using the portable semantic-version writer. Windows candidate/signing and native qualification use separate dispatch workflows and do not publish a Windows release.

```bash
# Bump version, commit, tag, and push
./scripts/release.sh patch   # 0.1.1 → 0.1.2
./scripts/release.sh minor   # 0.1.1 → 0.2.0
./scripts/release.sh major   # 0.1.1 → 1.0.0
./scripts/release.sh 2.0.0   # explicit version
```

---

## 🤝 Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes following [Conventional Commits](https://www.conventionalcommits.org/)
4. Push and open a Pull Request

---

## 📄 License

MIT License. Part of the [Colima](https://github.com/abiosoft/colima) ecosystem.

<p align="center">
  Built with ❤️ using <a href="https://tauri.app">Tauri</a>, <a href="https://svelte.dev">Svelte</a>, and <a href="https://www.rust-lang.org">Rust</a>
</p>
