# ADR-0031: Professional UI Replacement

## Status

Proposed (2026-11-04) — `UI-P01` (extension prompt in `analytics/prompt-stage3.md` §10). 
Acceptance gate: land of `crates/strangecoin-ui/` (design system + component library + 5 screens), full axe-core WCAG AA audit green, strangler migration closeout (`src/gui/` → pure launcher), `tests/two_clients.rs` green throughout.

## Context

Strangecoin today ships an `eframe`/`egui 0.22` immediate-mode GUI in
`src/gui/`. The current state (verified at HEAD post-Stage 2):

- **Login screen** — wallet address input + password field (regtest default
  `password`).
- **Wallet view** — single egui view: address (read-only), balance, recipient
  + amount inputs, Send button.
- **No node operator dashboard** — operator cannot see peers, sync status,
  mempool, or recent blocks without tailing logs.
- **No agent activity view** — ACP messages (ADR-0030), batched txs
  (ADR-0028), sponsor txs are invisible in the UI.
- **No transaction detail screen** — no receipts, no witness proofs (Stage 3
  S3-P11 stateless witnesses), no event logs.
- **No settings screen** — operator must edit `config.toml` directly.
- **No accessibility audit** — no WCAG compliance check; tab order, focus
  indicators, color contrast, keyboard navigation all ad-hoc.
- **No themes** — single visual style; no light/dark toggle.
- **No i18n** — English-only labels.
- **Windows `winapi` features declared explicitly** in `Cargo.toml` (eframe
  0.22 depends on `winapi` without declaring needed features — this repo
  declares them).

The forces in tension:

1. **Demo aesthetic vs. production UX.** The current UI is fine for a "two-
   node demo" (the headline `tests/two_clients.rs` test), but inadequate for
   a node operator running Strangecoin daily. ROADMAP.md Этап 4 explicitly
   requires "professional UI".
2. **Render backend choice.** Three serious options:
   - **Retain eframe/egui** — Rust-native, immediate-mode, existing team
     expertise, no new deps. But accessibility support is limited (no
     native screen reader hooks; manual tab-order).
   - **Tauri+Web (HTML/CSS)** — best accessibility (browser a11y tree), best
     design ecosystem (Tailwind, shadcn). But massive new dependency
     (webview), non-trivial IPC bridge Rust↔JS, heavier builds.
   - **iced (retained Elm-style)** — Rust-native, retained mode (better for
     complex state), accessibility via platform APIs. But smaller ecosystem,
     learning curve, less battle-tested than egui.
3. **Strangler migration risk.** Replacing `src/gui/` in one PR is too big a
   change; `tests/two_clients.rs` (the headline integration test) might
   break. Incremental migration is needed.
4. **WCAG AA compliance (V-47).** New UI must meet WCAG AA; without an
   audit gate, accessibility regressions slip in. Need a CI-enforced check.
5. **i18n scope.** Initial set must be small (en/ru/zh) but extensible.
6. **MCP / ACP toggles.** The Settings screen must expose enable/disable for
   MCP server (ADR-0029) and ACP transport (ADR-0030) — these are operator
   config surfaces.

## Decision

**We will replace the demo `eframe`/`egui` UI with a professional UI in a new
crate `crates/strangecoin-ui/`, retaining eframe/egui as the render backend
(with custom design system + component library layered on top), migrating
incrementally via the strangler pattern, with WCAG AA accessibility enforced
by an axe-core CI gate, light/dark themes, and i18n (en/ru/zh) via the
`fluent` crate.**

### Render backend: retain eframe/egui 0.22+

**Choice: retain `eframe`/`egui`.** Rationale:

- **Rust-native, no new heavyweight dep.** eframe/egui is already in the
  dependency tree (Stage 0); no webview, no IPC bridge.
- **Team expertise.** Existing `src/gui/` code is eframe/egui; the team knows
  the immediate-mode paradigm.
- **Deterministic builds unchanged.** Adding Tauri+Web would require
  reproducible-build pipeline changes (webview versioning, JS bundling).
  SLSA L3 provenance (Stage 0) would need rework.
- **Trade-off accepted: accessibility is manual.** eframe/egui does not have
  native screen reader hooks (unlike browser DOM in Tauri+Web). WCAG AA
  compliance is achieved via manual tab-order, focus indicators, color
  contrast ≥ 4.5:1, keyboard navigation, and an axe-core audit on headless
  rendered screenshots (with DOM-emulation layer for the audit).

**Upgrade path tracked in this ADR:**

- If eframe/egui adds native screen reader hooks in a future version → adopt
  (amend this ADR).
- If eframe/egui accessibility story does not mature by Stage 5+ → consider
  Tauri+Web migration as a separate ADR (ADR-0036 candidate).

### New crate: `crates/strangecoin-ui/`

A headless design system + component library, render-backend-agnostic core
(trait `UiBackend` with `EframeEguiBackend` impl). Structure:

```
crates/strangecoin-ui/
├── Cargo.toml           # strangecoin-ui = "0.1.0", features = ["eframe_egui", "a11y-audit"]
├── src/
│   ├── lib.rs           # UiBackend trait, EframeEguiBackend impl, App struct (top-level)
│   ├── tokens.rs        # Design tokens: ColorPalette, SpacingScale, TypographyScale, Radius, Shadow
│   ├── theme.rs         # Theme enum (Light/Dark/System), ThemeManager
│   ├── i18n.rs          # Localization via fluent-bundle
│   ├── components/      # 14 reusable components
│   │   ├── button.rs
│   │   ├── input.rs
│   │   ├── card.rs
│   │   ├── table.rs
│   │   ├── modal.rs
│   │   ├── drawer.rs
│   │   ├── toast.rs
│   │   ├── tabs.rs
│   │   ├── breadcrumb.rs
│   │   ├── badge.rs
│   │   ├── avatar.rs
│   │   ├── stat.rs
│   │   ├── chart.rs
│   │   └── codeblock.rs
│   └── screens/         # 5 screens
│       ├── wallet/      # Send / History / Contracts tabs
│       ├── dashboard/   # Peers / Sync / Mempool / Blocks panels
│       ├── tx_detail/   # Overview / Receipt / Witness / Events panels
│       ├── agent_activity/  # Feed + Filter sidebar
│       └── settings/    # Network / Peers / Keystore / Config / MCP / ACP sub-screens
├── locales/
│   ├── en/main.ftl
│   ├── ru/main.ftl
│   └── zh/main.ftl
├── tests/
│   ├── a11y_audit.rs            # axe-core audit harness (all components + screens)
│   ├── component_snapshot.rs    # snapshot tests (light/dark)
│   └── screens/
│       ├── wallet_a11y.rs
│       ├── dashboard_a11y.rs
│       ├── tx_detail_a11y.rs
│       ├── agent_activity_a11y.rs
│       └── settings_a11y.rs
└── examples/
    └── gallery.rs       # design system gallery (renders all components in light/dark, all locales)
```

### Design system

- **Design tokens** (single source of truth in `tokens.rs`):
  - `ColorPalette` — 7 hues (primary, secondary, accent, success, warning,
    error, neutral) × 10 shades each (50..950).
  - `SpacingScale` — 4 / 8 / 12 / 16 / 24 / 32 / 48 / 64 px.
  - `TypographyScale` — xs / sm / base / lg / xl / 2xl, with font family,
    weight, line-height.
  - `Radius` — none / sm (4) / md (8) / lg (16) / full.
  - `Shadow` — none / sm / md / lg.
- **Themes** (`theme.rs`): `Theme` enum (Light/Dark/System). `ThemeManager`
  resolves `System` via `eframe::Visuals::dark()` detection. Persisted to
  config (`ui.theme = "light" | "dark" | "system"`).
- **i18n** (`i18n.rs`): `fluent` crate (L20n-style). Locale files in
  `locales/{en,ru,zh}/main.ftl`. `Localization` struct loads bundles, exposes
  `tr(key, args)` method. Config: `ui.locale = "en" | "ru" | "zh" | "system"`.

### Screens (5)

| Screen | Tabs/Panels | Data sources |
|--------|-------------|--------------|
| **Wallet** | Send / History / Contracts | `eth_getBalance`, `eth_getTransactionByHash`, `eth_call`/`eth_sendTransaction`, `agent_signAndSubmitBatch` (AI-P04) |
| **Node Dashboard** | Peers / Sync / Mempool / Blocks | `NodeEvent` bus (Stage 1), `StorageEngine::get_tip`, mempool iterator |
| **Transaction Detail** | Overview / Receipt / Witness / Events | `StorageEngine::get_tx` + `get_receipt` (Stage 3 S3-P09 indices) + `StateWitness` wire (Stage 3 S3-P11) + ABI decoder (indexer) |
| **Agent Activity** | Feed + Filter sidebar | `agent_getActivity` (AI-P04, delegates to indexer); ACP messages + batched txs + sponsor txs (stub) + transfers |
| **Settings** | Network / Peers / Keystore / Config / MCP / ACP | `config.toml` (read/write with validation), `OperatorKeystore` vs `AgentKeystore` (ADR-0028 — separate tabs), MCP toggles (ADR-0029), ACP toggles (ADR-0030) |

### Strangler migration

Per ARCHITECT.md §10.5 (strangler pattern). `src/gui/` becomes a thin
launcher:

- `src/gui/mod.rs` — `egui::App` impl delegates to `strangecoin_ui::App`.
- Old `src/gui/wallet_view.rs` and similar demo files are removed
  incrementally as the corresponding new screens land in `strangecoin-ui`.
- `tests/two_clients.rs` (the headline integration test) remains green
  throughout the migration — it does not depend on the specific UI code,
  only on the underlying `BlockchainFacade`. If the test breaks due to UI
  changes, the test is fixed to use the new launcher, but coverage is not
  reduced.

Migration order (per `prompt-stage3.md` §10):

1. **UI-P02** — design system + components + tokens + themes + i18n + a11y
   audit harness.
2. **UI-P03** — wallet screen migration (Send/History/Contracts) + `src/gui/`
   → launcher.
3. **UI-P04** — node dashboard + transaction detail (receipts, witness
   proofs).
4. **UI-P05** — agent activity + settings + full a11y audit + strangler
   closeout + Changelog 4.0.0.

### WCAG AA accessibility (invariant #27)

**Approach:** axe-core audit on headless-rendered screenshots with a DOM-
emulation layer.

- eframe/egui does not produce a DOM; axe-core expects a DOM. The audit
  harness (`tests/a11y_audit.rs`) renders each component/screen headlessly,
  builds a synthetic DOM from the egui widget tree (with ARIA-like metadata
  attached via `egui::Widget::id()` + accessibility metadata), and runs
  axe-core rules against the synthetic DOM.
- WCAG AA criteria checked per screen: color contrast ≥ 4.5:1, focus
  indicators (WCAG 2.4.7), min touch target 44×44 px (WCAG 2.5.5),
  keyboard navigation (Tab/Enter/Esc), no color-only information (WCAG
  1.4.1 — peer scoring color band has text label too).
- **CI gate:** `cargo test -p strangecoin-ui --features a11y-audit` must
  pass on every PR. Failures block merge.
- Manual review items (e.g., dynamic content live-regions) documented in
  `docs/spec/ui_professional.md`.

### i18n

- **Initial locales:** `en`, `ru`, `zh` — cover the maintainer's known
  audience. Extensible via `fluent` — add a new `locales/{locale}/main.ftl`
  file and update the `Localization` bundle loader.
- **Translation workflow:** source strings authored in `en/main.ftl`;
  `ru/main.ftl` and `zh/main.ftl` translated by contributors. PRs welcome.
- **Config:** `ui.locale = "en" | "ru" | "zh" | "system"` in `config.toml`.
  `system` resolves via `unic-langid` OS locale detection.

### Pin policy

- `eframe` / `egui` — pinned `=0.22.x` (Stage 2 precedent; upgrade requires
  ADR amendment to this ADR).
- `fluent` / `fluent-bundle` — pinned `=x.y.z`.
- `axe-core` (via `cargo test` headless audit) — dev-dep only, not in
  runtime Cargo.toml.

## Consequences

### Positive

- **No new heavyweight dep.** Retaining eframe/egui keeps the dependency tree
  lean; deterministic builds (SLSA L3, Stage 0) unchanged.
- **Rust-native story preserved.** No JS/TS in the build; no webview runtime
  dependency.
- **Team expertise reuse.** Existing eframe/egui knowledge applies.
- **Design system reusable for future UIs.** The `crates/strangecoin-ui/`
  component library can be reused for Stage 5+ explorer / faucet web UIs
  (with a different render backend impl if needed — the `UiBackend` trait is
  render-backend-agnostic).
- **Incremental migration.** Strangler pattern means `tests/two_clients.rs`
  stays green throughout; rollback is possible per-screen.

### Negative

- **Accessibility effort is manual.** Without native screen reader hooks,
  WCAG AA compliance requires manual implementation and synthetic-DOM audit
  harness. Higher engineering cost than Tauri+Web (where the browser a11y
  tree comes for free).
- **axe-core requires DOM-emulation layer.** Building the synthetic DOM from
  egui widget tree is non-trivial; some axe-core rules may not apply cleanly.
- **i18n adds translation workflow.** Translations must be maintained
  per-locale; missing translations fall back to `en` (documented in spec).
- **Stage 4 scope grows.** Adding professional UI (5 screens + design system
  + a11y audit + i18n) to Stage 4 is significant work. Mitigation: extension
  prompts `UI-P01`..`UI-P05` are gated (§8.3) and can slip to Stage 4.x if
  needed without blocking Stage 4 core.
- **eframe/egui upgrade path is locked.** Pinning `=0.22.x` means upgrades
  require an ADR amendment. If eframe/egui 0.23+ adds native a11y support,
  upgrading requires re-auditing all screens.

### Neutral

- **Design system is hand-rolled.** No external design system dependency
  (no shadcn/MUI equivalent for egui). Single source of truth in `tokens.rs`.
- **Render-backend-agnostic core is aspirational.** The `UiBackend` trait is
  designed for future render-backend swaps (e.g., to Tauri+Web if Stage 5+
  decides to migrate), but only `EframeEguiBackend` is implemented in
  Stage 4. Other backends are Stage 5+ research.

## Alternatives

### Alternative 1: Tauri+Web (HTML/CSS via webview)

- **Pros:** Best accessibility (browser a11y tree); best design ecosystem
  (Tailwind, shadcn, Radix); web-ready (can serve same UI in browser for
  explorer/faucet).
- **Cons:** Massive new dependency (webview runtime per platform); non-
  trivial IPC bridge Rust↔JS; heavier builds; reproducible-build pipeline
  (SLSA L3) needs rework for webview versioning + JS bundling; team learns
  new paradigm (Elm-style state sync).
- **Why not chosen:** Stage 4 prioritizes lean dependency tree and
  deterministic builds. Tauri+Web is a Stage 5+ candidate if eframe/egui
  accessibility story does not mature.

### Alternative 2: iced (retained Elm-style, Rust-native)

- **Pros:** Retained mode (better for complex state like dashboards);
  accessibility via platform APIs (better than egui); pure Rust.
- **Cons:** Smaller ecosystem than egui; learning curve; less battle-tested;
  would require rewriting all existing `src/gui/` code (no incremental
  migration path).
- **Why not chosen:** Stage 4 retains eframe/egui for incremental migration.
  iced is a Stage 5+ candidate if retained-mode proves necessary.

### Alternative 3: slint (declarative Rust-native)

- **Pros:** Declarative DSL; performant; cross-platform.
- **Cons:** Commercial license required for some features; smaller ecosystem;
  would require rewriting `src/gui/`.
- **Why not chosen:** License risk; ecosystem risk.

### Alternative 4: Skip design system, write screens ad-hoc

- **Pros:** Less upfront work; ship screens faster.
- **Cons:** Divergence risk (each screen looks slightly different); no theme
  consistency; accessibility per-screen ad-hoc (V-47 risk); no reuse for
  Stage 5+ explorer/faucet UIs.
- **Why not chosen:** Design system is the foundation; skipping it creates
  technical debt.

## Related

- [ADR-0009](0009-events-bus.md) — NodeEvent bus (Stage 1; reused for dashboard live updates)
- [ADR-0024](0024-storage-schema-and-engine.md) — Storage schema (Stage 3; provides indices for tx detail screen)
- [ADR-0028](0028-strangecoin-as-cryptocurrency-for-ai-agents.md) — AI agent vision (Agent Activity screen surfaces agent data; Settings reflects key isolation)
- [ADR-0029](0029-mcp-server.md) — MCP server (Settings screen has MCP toggles)
- [ADR-0030](0030-acp-agent-communication-protocol.md) — ACP transport (Settings screen has ACP toggles; Agent Activity screen shows ACP messages)
- [ARCHITECT.md §19.3/§19.5/§19.6](../../analytics/ARCHITECT.md) — Professional UI subsystem, invariant #27, threat vector V-47
- [ROADMAP.md §«Этап 4»](../../analytics/ROADMAP.md) — Stage 4 (extended with professional UI tasks)
- [docs/spec/ui_professional.md](../spec/ui_professional.md) — UI spec (design tokens, component list, screen wireframes, accessibility checklist, i18n workflow, theme switcher semantics)
- [prompt-stage3.md §10 UI-P01..UI-P05](../../analytics/prompt-stage3.md) — Implementation prompts
- [BACKLOG.md](../../analytics/BACKLOG.md) — Work item tracking
- [WCAG 2.1 AA](https://www.w3.org/TR/WCAG21/) — accessibility standard
- [axe-core](https://github.com/dequelabs/axe-core) — accessibility audit engine
- [fluent](https://projectfluent.org/) — i18n system
- [egui](https://github.com/emilk/egui) — render backend (retained)
