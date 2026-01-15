# RunnerBuddy — GitHub Self-Hosted Runner GUI

> A friendly desktop app that makes your computer a GitHub Actions runner in 3 clicks.
>
> ---
>
> ## The Problem
>
> Setting up a GitHub self-hosted runner today requires:
> 1. Navigate to repo/org settings → Actions → Runners → New runner
> 2. 2. Copy a time-limited token (expires in 1 hour!)
>    3. 3. Download a tarball, extract it
>       4. 4. Run `./config.sh` with the right flags
>          5. 5. Manually set up a systemd service / launchd plist / Windows service
>             6. 6. Hope you remember how to update labels or reconfigure later
>               
>                7. This is fine for DevOps engineers. It's annoying for someone who just wants their old laptop to run builds.
>               
>                8. ---
>               
>                9. ## MVP Scope
>
> ### What it does (v0.1)
> - **Register a runner** to a repo, org, or enterprise with GitHub OAuth or PAT
> - - **Install as a service** that runs on boot (one toggle)
>   - - **Show live status** — idle, running job, offline
>     - - **Start/stop** the runner
>       - - **Unregister** cleanly when you're done
>        
>         - ### What it doesn't do (yet)
>         - - Multiple runners on one machine
>           - - Runner groups management
>             - - Custom work directories
>               - - Ephemeral runner mode
>                 - - Proxy configuration
>                   - - Auto-updates of the runner binary
>                    
>                     - ---
>
> ## Tech Stack Recommendation
>
> ### Option A: Tauri (Recommended)
> ```
> Frontend: SvelteKit or React + Tailwind
> Backend:  Rust
> Size:     ~5-10 MB
> ```
> - Native performance, tiny binary
> - - Good for system-level stuff (services, file permissions)
>   - - Cross-platform from single codebase
>     - - Rust's `std::process` for spawning runner, service management
>      
>       - ### Option B: Electron
>       - ```
>         Frontend: React + Tailwind
>         Backend:  Node.js
>         Size:     ~80-150 MB
>         ```
>         - Faster to prototype if you know the JS ecosystem well
>         - - Heavier, but "it works"
>           - - Use `child_process` and platform-specific libs
>            
>             - ### Option C: Flutter Desktop
>             - ```
>               Frontend: Flutter/Dart
>               Backend:  Dart + FFI or shell-outs
>               Size:     ~20-30 MB
>               ```
>               - Good if you want iOS/Android versions later
>               - - Service management would be shell-outs
>                
>                 - **I'd go Tauri** — it's the right tool for a lightweight system utility.
>                
>                 - ---
>
> ## Architecture
>
> ```
> ┌─────────────────────────────────────────────────────────────────┐
> │                         RunnerBuddy                              │
> ├─────────────────────────────────────────────────────────────────┤
> │  ┌───────────────────────────────────────────────────────────┐  │
> │  │                      UI Layer (Svelte)                    │  │
> │  │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────────────┐  │  │
> │  │  │ Setup   │ │ Status  │ │ Config  │ │ Logs            │  │  │
> │  │  │ Wizard  │ │ Display │ │ Editor  │ │ Viewer          │  │  │
> │  │  └─────────┘ └─────────┘ └─────────┘ └─────────────────┘  │  │
> │  └───────────────────────────────────────────────────────────┘  │
> │                              │                                   │
> │                              ▼                                   │
> │  ┌───────────────────────────────────────────────────────────┐  │
> │  │                   Tauri Command Layer                     │  │
> │  │  ┌──────────────┐ ┌──────────────┐ ┌──────────────────┐   │  │
> │  │  │ github_api   │ │ runner_mgmt  │ │ service_mgmt     │   │  │
> │  │  │ - OAuth flow │ │ - download   │ │ - install svc    │   │  │
> │  │  │ - get token  │ │ - configure  │ │ - start/stop     │   │  │
> │  │  │ - list repos │ │ - run        │ │ - enable on boot │   │  │
> │  │  └──────────────┘ └──────────────┘ └──────────────────┘   │  │
> │  └───────────────────────────────────────────────────────────┘  │
> │                              │                                   │
> │                              ▼                                   │
> │  ┌───────────────────────────────────────────────────────────┐  │
> │  │                 Platform Abstraction Layer                │  │
> │  │  ┌─────────────┐ ┌─────────────┐ ┌─────────────────────┐  │  │
> │  │  │   Linux     │ │   macOS     │ │   Windows           │  │  │
> │  │  │  systemd    │ │  launchd    │ │  SC / Task Sched    │  │  │
> │  │  └─────────────┘ └─────────────┘ └─────────────────────┘  │  │
> │  └───────────────────────────────────────────────────────────┘  │
> └─────────────────────────────────────────────────────────────────┘
>                                │
>                                ▼
> ┌─────────────────────────────────────────────────────────────────┐
> │                    External Components                           │
> │  ┌─────────────────┐    ┌─────────────────────────────────────┐ │
> │  │  GitHub API     │    │  actions/runner binary              │ │
> │  │  (REST + OAuth) │    │  (downloaded & managed separately)  │ │
> │  └─────────────────┘    └─────────────────────────────────────┘ │
> └─────────────────────────────────────────────────────────────────┘
> ```
>
> ---
>
> ## Data Model
>
> ### Local Config (`~/.config/runnerbuddy/config.json`)
> ```json
> {
>   "runner": {
>     "name": "zach-macmini",
>     "labels": ["self-hosted", "macOS", "ARM64", "gpu"],
>     "work_dir": "~/.runnerbuddy/work",
>     "scope": {
>       "type": "repo",
>       "owner": "zachs-company",
>       "repo": "backend-api"
>     }
>   },
>   "service": {
>     "installed": true,
>     "run_on_boot": true
>   },
>   "github": {
>     "auth_method": "oauth",
>     "token_expires": null
>   },
>   "runner_version": "2.321.0",
>   "install_path": "~/.runnerbuddy/runner"
> }
> ```
>
> ### Runtime State (in-memory)
> ```typescript
> interface RunnerState {
>   status: 'offline' | 'idle' | 'running';
>   current_job?: {
>     workflow: string;
>     run_id: number;
>     started_at: Date;
>   };
>   pid?: number;
>   last_heartbeat: Date;
> }
> ```
>
> ---
>
> ## MVP Development Phases
>
> ### Phase 1: Core Loop (Week 1)
> - [ ] Tauri + Svelte scaffolding
> - [ ] - [ ] GitHub OAuth flow (or PAT input)
> - [ ] - [ ] Download runner binary
> - [ ] - [ ] Run `config.sh` from app
> - [ ] - [ ] Start runner process (foreground, no service yet)
> - [ ] - [ ] Basic status display (running/stopped)
>
> - [ ] ### Phase 2: Service Integration (Week 2)
> - [ ] - [ ] Linux systemd service install/uninstall
> - [ ] - [ ] macOS launchd plist management
> - [ ] - [ ] Windows service (use runner's built-in svc.cmd)
> - [ ] - [ ] "Run on boot" toggle
> - [ ] - [ ] Proper start/stop/restart controls
>
> - [ ] ### Phase 3: Polish (Week 3)
> - [ ] - [ ] System tray icon with status
> - [ ] - [ ] Job history (parse logs or poll API)
> - [ ] - [ ] Label editing (requires re-registration)
> - [ ] - [ ] Log viewer
> - [ ] - [ ] Unregister & cleanup flow
> - [ ] - [ ] Auto-update runner binary
>
> - [ ] ### Phase 4: Nice-to-Haves (Future)
> - [ ] - [ ] Multiple runners per machine
> - [ ] - [ ] Runner groups support
> - [ ] - [ ] Ephemeral mode
> - [ ] - [ ] Notifications (job started/completed)
> - [ ] - [ ] Resource usage graphs
> - [ ] - [ ] Dark/light mode
>
> - [ ] ---
>
> - [ ] ## Security Considerations
>
> - [ ] 1. **Token storage**: Use OS keychain (macOS Keychain, Windows Credential Manager, Linux Secret Service) — Tauri has plugins for this.
>
> - [ ] 2. **PAT scope**: Minimum required is `repo` for repo-level, `admin:org` for org-level.
>
> - [ ] 3. **OAuth scopes**: Same — request minimum necessary.
>
> - [ ] 4. **Service user**: On Linux, consider creating a dedicated user rather than running as root.
>
> - [ ] 5. **Registration token**: These expire in 1 hour, so we request on-demand during setup, not stored.
>
> - [ ] 6. **Runner token**: The runner itself stores a token in `.credentials` — this is managed by the runner binary, not us.
>
> - [ ] ---
>
> - [ ] ## Get Started
>
> - [ ] ```bash
> - [ ] # Prerequisites
> - [ ] brew install rust node  # or equivalent
>
> - [ ] # Create project
> - [ ] npm create tauri-app@latest runnerbuddy -- --template svelte-ts
> - [ ] cd runnerbuddy
>
> - [ ] # Add dependencies
> - [ ] cargo add reqwest tokio serde serde_json dirs
> - [ ] npm install -D tailwindcss @tailwindcss/forms
>
> - [ ] # Run dev
> - [ ] npm run tauri dev
> - [ ] ```
>
> - [ ] ---
>
> - [ ] *This doc is a starting point. Adjust based on what you actually want to build vs. what sounds cool on paper.*
