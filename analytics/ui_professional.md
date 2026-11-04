# Strangecoin Professional UI Spec

**Status:** Proposed (2026-11-04)
**Implementation prompts:** `analytics/prompt-stage3.md` §10 `UI-P01`..`UI-P05`
**Authoritative ADR:** [ADR-0031](../ADR/0031-professional-ui-replacement.md)
**Related specs:** [agent_integration.md](agent_integration.md) (Agent Activity screen), [acp_messages.md](acp_messages.md) (Agent Activity content), [mcp_server.md](mcp_server.md) (Settings toggles)
**Invariants enforced:** #27 (Professional UI WCAG AA)
**Threat vectors mitigated:** V-47 (UI accessibility regression)
**Spec version:** 1.0

---

## 1. Overview

The current Strangecoin UI (`src/gui/`) is a demo-grade `eframe`/`egui 0.22`
immediate-mode GUI — a login screen + a single wallet view (address,
balance, recipient, amount, Send button). It is inadequate for daily node
operation and for monitoring agent activity.

This spec defines the **professional UI replacement** (ADR-0031): a new
crate `crates/strangecoin-ui/` with a hand-rolled design system, 14
reusable components, and 5 screens, retaining `eframe`/`egui` as the
render backend, with WCAG AA accessibility enforced by an axe-core CI gate,
light/dark themes, and i18n (en/ru/zh) via the `fluent` crate.

The migration uses the **strangler pattern** (ARCHITECT.md §10.5): `src/gui/`
becomes a thin launcher delegating to `strangecoin_ui::App`. The headline
test `tests/two_clients.rs` remains green throughout the migration.

---

## 2. Design tokens

Single source of truth in `crates/strangecoin-ui/src/tokens.rs`.

### 2.1. Color palette

7 hues × 10 shades (50..950):

| Hue | 50 | 100 | 200 | 300 | 400 | 500 | 600 | 700 | 800 | 900 | 950 |
|-----|----|-----|-----|-----|-----|-----|-----|-----|-----|-----|-----|
| primary | #eff6ff | #dbeafe | #bfdbfe | #93c5fd | #60a5fa | #3b82f6 | #2563eb | #1d4ed8 | #1e40af | #1e3a8a | #172554 |
| secondary | #f8fafc | #f1f5f9 | #e2e8f0 | #cbd5e1 | #94a3b8 | #64748b | #475569 | #334155 | #1e293b | #0f172a | #020617 |
| accent | #fdf4ff | #fae8ff | #f5d0fe | #f0abfc | #e879f9 | #d946ef | #c026d3 | #a21caf | #86198f | #701a75 | #4a044e |
| success | #f0fdf4 | #dcfce7 | #bbf7d0 | #86efac | #4ade80 | #22c55e | #16a34a | #15803d | #166534 | #14532d | #052e16 |
| warning | #fffbeb | #fef3c7 | #fde68a | #fcd34d | #fbbf24 | #f59e0b | #d97706 | #b45309 | #92400e | #78350f | #451a03 |
| error | #fef2f2 | #fee2e2 | #fecaca | #fca5a5 | #f87171 | #ef4444 | #dc2626 | #b91c1c | #991b1b | #7f1d1d | #450a0a |
| neutral | #fafafa | #f4f4f5 | #e4e4e7 | #d4d4d8 | #a1a1aa | #71717a | #52525b | #3f3f46 | #27272a | #18181b | #09090b |

**WCAG AA contrast:** body text uses `neutral-900` on `neutral-50` (light
theme) or `neutral-50` on `neutral-950` (dark theme) — both ≥ 12:1 contrast
ratio, well above the 4.5:1 minimum.

### 2.2. Spacing scale

| Token | px |
|-------|-----|
| `xs` | 4 |
| `sm` | 8 |
| `md` | 12 |
| `base` | 16 |
| `lg` | 24 |
| `xl` | 32 |
| `2xl` | 48 |
| `3xl` | 64 |

### 2.3. Typography scale

| Token | size (px) | weight | line-height | font family |
|-------|-----------|--------|-------------|--------------|
| `xs` | 12 | 400 | 1.4 | Inter, system-ui, sans-serif |
| `sm` | 14 | 400 | 1.4 | Inter, system-ui, sans-serif |
| `base` | 16 | 400 | 1.5 | Inter, system-ui, sans-serif |
| `lg` | 18 | 500 | 1.4 | Inter, system-ui, sans-serif |
| `xl` | 20 | 600 | 1.3 | Inter, system-ui, sans-serif |
| `2xl` | 24 | 700 | 1.2 | Inter, system-ui, sans-serif |
| `display` | 32 | 700 | 1.1 | Inter, system-ui, sans-serif |

Mono: `JetBrains Mono`, `Consolas`, monospace (for code blocks, addresses,
hashes).

### 2.4. Radius

| Token | px |
|-------|-----|
| `none` | 0 |
| `sm` | 4 |
| `md` | 8 |
| `lg` | 16 |
| `full` | 9999 |

### 2.5. Shadow

| Token | value |
|-------|-------|
| `none` | none |
| `sm` | `0 1px 2px 0 rgba(0,0,0,0.05)` |
| `md` | `0 4px 6px -1px rgba(0,0,0,0.1), 0 2px 4px -2px rgba(0,0,0,0.1)` |
| `lg` | `0 10px 15px -3px rgba(0,0,0,0.1), 0 4px 6px -4px rgba(0,0,0,0.1)` |

---

## 3. Themes

`Theme` enum in `crates/strangecoin-ui/src/theme.rs`:

```rust
enum Theme {
    Light,
    Dark,
    System,  // resolves via eframe::Visuals::dark() detection
}
```

`ThemeManager`:

- Reads `ui.theme` from `config.toml` (default: `system`).
- Resolves `System` to `Light` or `Dark` via `eframe::Visuals::dark()`
  detection.
- Persists user choice to `config.toml` on change.
- Applies theme to all components via design tokens (color values swap
  between light/dark palettes).

**Config:**

```toml
[ui]
theme = "system"  # "light" | "dark" | "system"
locale = "en"     # "en" | "ru" | "zh" | "system"
```

---

## 4. i18n

`Localization` struct in `crates/strangecoin-ui/src/i18n.rs`, using the
`fluent` crate (L20n-style).

### 4.1. Locale files

`crates/strangecoin-ui/locales/{en,ru,zh}/main.ftl` — Fluent translation
files. Initial set: ≥100 strings covering all UI labels.

Example (`locales/en/main.ftl`):

```ftl
wallet-title = Wallet
wallet-send-tab = Send
wallet-history-tab = History
wallet-contracts-tab = Contracts
wallet-send-recipient = Recipient address
wallet-send-amount = Amount
wallet-send-fee = Fee
wallet-send-submit = Send
wallet-send-success = Transaction submitted: { $txid }
wallet-send-error-invalid-address = Invalid bech32 address
wallet-send-error-insufficient-balance = Insufficient balance
dashboard-title = Node Dashboard
dashboard-peers = Peers
dashboard-sync = Sync Status
dashboard-mempool = Mempool
dashboard-blocks = Recent Blocks
tx-detail-title = Transaction
tx-detail-receipt = Receipt
tx-detail-witness = Witness Proof
tx-detail-events = Events
agent-activity-title = Agent Activity
agent-activity-filter = Filter
settings-title = Settings
settings-network = Network
settings-peers = Peers
settings-keystore = Keystore
settings-config = Config
settings-mcp = MCP Server
settings-acp = ACP Transport
theme-light = Light
theme-dark = Dark
theme-system = System
```

### 4.2. Translation workflow

- Source strings authored in `en/main.ftl`.
- `ru/main.ftl` and `zh/main.ftl` translated by contributors.
- Missing translations fall back to `en` (documented in `Localization::tr`
  behavior; logs warning on first miss).
- PRs welcome; translations reviewed by maintainers.

---

## 5. Components (14)

Each component in `crates/strangecoin-ui/src/components/`:

| Component | File | Description |
|-----------|------|-------------|
| Button | `button.rs` | Variants: primary, secondary, ghost, danger; sizes: sm, md, lg |
| Input | `input.rs` | Text input with label, validation, error state; bech32 address input variant |
| Card | `card.rs` | Container with optional header/footer; shadow variants |
| Table | `table.rs` | Sortable columns, pagination, row click handler |
| Modal | `modal.rs` | Centered overlay; backdrop click + Esc to close |
| Drawer | `drawer.rs` | Side panel (left/right); used for filters |
| Toast | `toast.rs` | Success/error/warning/info notifications; auto-dismiss |
| Tabs | `tabs.rs` | Top tab bar; keyboard navigable (Left/Right arrows) |
| Breadcrumb | `breadcrumb.rs` | Navigation trail; click to navigate |
| Badge | `badge.rs` | Small status indicator (color-coded + text label) |
| Avatar | `avatar.rs` | Address identicon (generated from pubkey hash) |
| Stat | `stat.rs` | Large number display with label (e.g., "1,234 SC") |
| Chart | `chart.rs` | Sparkline / bar chart for balance history, block times |
| CodeBlock | `codeblock.rs` | Monospace block with copy button; for addresses, hashes, ABI JSON |

**All components:**

- Light/dark variants (auto-resolved via `ThemeManager`).
- Focus indicators (WCAG 2.4.7) — 2px `primary-500` outline on focus.
- Min touch target 44×44 px (WCAG 2.5.5).
- ARIA-like labels via `egui::Widget::id()` + accessibility metadata.
- i18n-aware (labels via `Localization::tr`).

---

## 6. Screens (5)

### 6.1. Wallet screen

**Tabs:** Send / History / Contracts

**Send tab:**

- Recipient input (bech32 validation, `Input` component).
- Amount input (SC + atomic units).
- Fee input (default = estimate from `agent_estimateTaskCost`, AI-P04).
- Submit button (calls `eth_sendTransaction` or `agent_signAndSubmitBatch`
  for batch sends).
- Success/error toast.

**History tab:**

- Table of recent txs (columns: txid, type, from, to, amount, fee, status,
  block height).
- Filter (sent/received/failed).
- Pagination (50 per page).
- Click row → navigates to Transaction Detail screen (UI-P04).

**Contracts tab:**

- List of deployed contracts (address, name, ABI).
- Click → call interface:
  - Method selector dropdown.
  - Args inputs (typed per ABI).
  - "Read" button (calls `eth_call`, displays result).
  - "Write" button (calls `eth_sendTransaction`).
- ABI viewer (CodeBlock component, JSON).

### 6.2. Node Dashboard screen

**Panels:** Peers / Sync / Mempool / Blocks

**Peers panel:**

- Table (peer_id, addr, latency EWMA, score, direction, capabilities).
- Color-coded score: green (>0.5), yellow (0.2-0.5), red (<0.2) — Stage 2
  peer scoring bands (ADR-0022).
- **WCAG 1.4.1:** color is not the only indicator — text label
  ("healthy" / "warning" / "banned") accompanies the color.
- Live updates via `NodeEvent` bus (Stage 1, ADR-0009).

**Sync panel:**

- Current height, tip height, sync %.
- Headers queue depth, blocks queue depth.
- Sync engine state (idle / headers-first / blocks / catch-up).

**Mempool panel:**

- Tx count, total fee (sum).
- Top-10-by-fee table (txid, sender, receiver, amount, fee, gas).
- RBF conflicts highlighted (badge).

**Blocks panel:**

- Recent 10 blocks (height, hash, tx count, gas used, miner, timestamp).
- Click → block detail modal (header + tx list).

### 6.3. Transaction Detail screen

**Panels:** Overview / Receipt / Witness / Events

Takes `txid` parameter (navigated from Wallet History row click).

**Overview panel:**

- txid (CodeBlock, copy button).
- Type (transfer / call / create).
- From, to, amount, fee, gas used.
- Status (success / fail).
- Block height, timestamp.

**Receipt panel:**

- Status (success / fail with revert reason).
- Gas used, gas limit.
- Logs (decoded ABI events).

**Witness panel:**

- `StateWitness` wire proof (Stage 3 S3-P11 deliverable).
- Account proof: Merkle path to `state_root`.
- Contract storage proof: Merkle path to `contract_storage_root`.
- "Verify" button — calls `verify_block_stateless` (Stage 1 + Stage 3 wire)
  client-side; displays "verified ✓" or "verification failed ✗".

**Events panel:**

- Decoded event logs (using ABI from indexer).

### 6.4. Agent Activity screen

**Panels:** Feed + Filter sidebar

**Feed:**

- Chronological list of (timestamp, agent_address, event_type, details).
- Event types:
  - `ACP_NEGOTIATE` (incoming/outgoing)
  - `ACP_TASK` (assigned/received)
  - `ACP_SETTLE` (released/received)
  - `ACP_REVOKE` (refunded/refunded-by)
  - `batched_tx` (submitted, with N txids)
  - `sponsor_tx` (stub status)
  - `transfer` (sent/received)
- Click → tx detail (UI-P04) or ACP message detail modal (negotiate/task/
  settle/revoke with all fields).

**Filter sidebar:**

- Filter by agent_address.
- Filter by event_type (multi-select).
- Filter by time range (from/to).

**Data source:** `agent_getActivity` JSON-RPC method (AI-P04), which
delegates to Stage 4 indexer.

### 6.5. Settings screen

**Sub-screens:** Network / Peers / Keystore / Config / MCP / ACP

**Network sub-screen:**

- listen_addr (read-only after start).
- seeds list (add/remove).
- max_peers (read-only after start).
- network_id (read-only).

**Peers sub-screen:**

- Peer list (same as Dashboard Peers panel).
- Ban/unban buttons.
- Add seed button.

**Keystore sub-screen:**

- **Two tabs (key isolation, ADR-0028 invariant #26):**
  - **Operator tab** — operator wallet (import/export, password change).
  - **Agent tab** — agent wallets (list, import, HD derive new, session
    key management).
- Wallet list (address, balance, type).
- Import/export (encrypted JSON, AES-256-GCM + PBKDF2).
- Password change.

**Config sub-screen:**

- Raw `config.toml` editor (CodeBlock, editable).
- Validation on save (parse + semantic checks).
- "Save & Restart" button (warns that node will restart).

**MCP sub-screen:**

- Enable/disable toggle (ADR-0029).
- Transport select (stdio / HTTP+SSE / both).
- HTTP listen addr (editable if HTTP+SSE enabled).
- Connected MCP clients list (live, via `NodeEvent` bus).

**ACP sub-screen:**

- Enable/disable toggle (ADR-0030).
- Active ACP sessions list (agent_id_a, agent_id_b, escrow_txid, status).
- Ban agent button.

---

## 7. Accessibility (invariant #27)

### 7.1. WCAG AA criteria enforced

| Criterion | How enforced |
|-----------|--------------|
| 1.4.1 Use of Color | Color is never the only indicator (e.g., peer score has text label) |
| 1.4.3 Contrast (Minimum) | Body text: ≥ 12:1 ratio (well above 4.5:1 minimum) |
| 2.1.1 Keyboard | All functionality operable via keyboard (Tab/Enter/Esc/arrows) |
| 2.1.2 No Keyboard Trap | Focus can always leave a component (Esc closes modals) |
| 2.4.3 Focus Order | Tab order follows visual order (left-to-right, top-to-bottom) |
| 2.4.7 Focus Visible | 2px `primary-500` outline on focused element |
| 2.5.5 Target Size | Min touch target 44×44 px (touchscreen support) |
| 3.3.1 Error Identification | Form errors identified in text (not just color) |
| 3.3.2 Labels or Instructions | All inputs have associated labels |
| 4.1.2 Name, Role, Value | ARIA-like metadata via `egui::Widget::id()` + accessibility layer |

### 7.2. Audit harness

`crates/strangecoin-ui/tests/a11y_audit.rs`:

- Headless render of each component + each screen.
- Synthetic DOM built from egui widget tree (with ARIA-like metadata).
- axe-core ruleset run against synthetic DOM.
- CI gate: `cargo test -p strangecoin-ui --features a11y-audit` must pass
  on every PR. Failures block merge.

### 7.3. Per-screen audit tests

`crates/strangecoin-ui/tests/screens/`:

- `wallet_a11y.rs` — Send/History/Contracts tabs.
- `dashboard_a11y.rs` — Peers/Sync/Mempool/Blocks panels.
- `tx_detail_a11y.rs` — Overview/Receipt/Witness/Events panels.
- `agent_activity_a11y.rs` — Feed + Filter sidebar.
- `settings_a11y.rs` — Network/Peers/Keystore/Config/MCP/ACP sub-screens.

### 7.4. Manual review items

Some WCAG AA criteria require manual review (cannot be fully automated):

- **Dynamic content live-regions** (e.g., toasts, peer score updates) —
  manual review ensures screen readers announce changes.
- **Heading hierarchy** — manual review ensures logical heading levels (h1
  → h2 → h3, no skips).
- **Link purpose** — manual review ensures link text is descriptive (not
  "click here").

These are documented in `docs/spec/ui_professional.md` (this file) per
screen, and reviewed in PR review.

---

## 8. Strangler migration

### 8.1. Order

Per `prompt-stage3.md` §10:

1. **UI-P02** — design system + components + tokens + themes + i18n + a11y
   audit harness. `crates/strangecoin-ui/` created as workspace member.
2. **UI-P03** — wallet screen migration (Send/History/Contracts).
   `src/gui/` becomes thin launcher. Old `src/gui/wallet_view.rs` removed.
3. **UI-P04** — node dashboard + transaction detail (receipts, witness
   proofs).
4. **UI-P05** — agent activity + settings + full a11y audit + strangler
   closeout + Changelog 4.0.0.

### 8.2. `src/gui/` post-migration

After UI-P05 closeout, `src/gui/` contains only:

```rust
// src/gui/mod.rs (final form, ~30 lines)

use eframe::egui;
use strangecoin_ui::App as UiApp;

pub struct GuiApp {
    ui_app: UiApp,
}

impl egui::App for GuiApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.ui_app.update(ctx, frame);
    }
}

pub fn run_gui(node: Arc<BlockchainFacade>) -> eframe::Result<()> {
    let ui_app = UiApp::new(node);
    eframe::run_native(
        "Strangecoin",
        eframe::NativeOptions::default(),
        Box::new(move |_cc| Box::new(GuiApp { ui_app })),
    )
}
```

All business logic — wallet rendering, dashboard, agent activity, settings
— lives in `crates/strangecoin-ui/`.

### 8.3. `tests/two_clients.rs` invariant

The headline integration test `tests/two_clients.rs` (verifies 100 signed
transfers across 5 wallets settle to `[0,0,0,0,10000]`) must remain green
throughout the migration. The test does not depend on specific UI code —
it drives `BlockchainFacade` directly. If the test breaks due to UI
changes, the test is fixed to use the new launcher (`GuiApp`), but test
coverage is not reduced.

---

## 9. Crate structure

```
crates/strangecoin-ui/
├── Cargo.toml           # strangecoin-ui = "0.1.0", features = ["eframe_egui", "a11y-audit"]
├── src/
│   ├── lib.rs           # UiBackend trait, EframeEguiBackend impl, App struct
│   ├── tokens.rs        # Design tokens (single source of truth)
│   ├── theme.rs         # Theme enum, ThemeManager
│   ├── i18n.rs          # Localization via fluent-bundle
│   ├── components/      # 14 components
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
│   └── screens/
│       ├── wallet/         # mod.rs, send.rs, history.rs, contracts.rs
│       ├── dashboard/      # mod.rs, peers.rs, sync.rs, mempool.rs, blocks.rs
│       ├── tx_detail/      # mod.rs, overview.rs, receipt.rs, witness.rs, events.rs
│       ├── agent_activity/ # mod.rs, feed.rs, filter.rs
│       └── settings/       # mod.rs, network.rs, peers.rs, keystore.rs, config.rs, mcp.rs, acp.rs
├── locales/
│   ├── en/main.ftl
│   ├── ru/main.ftl
│   └── zh/main.ftl
├── tests/
│   ├── a11y_audit.rs            # axe-core audit harness
│   ├── component_snapshot.rs    # snapshot tests (light/dark)
│   └── screens/
│       ├── wallet_a11y.rs
│       ├── dashboard_a11y.rs
│       ├── tx_detail_a11y.rs
│       ├── agent_activity_a11y.rs
│       └── settings_a11y.rs
└── examples/
    └── gallery.rs       # design system gallery (renders all components)
```

**Dependencies:**

- `eframe = "=0.22.x"` (render backend; pinned per ADR-0031)
- `egui = "=0.22.x"`
- `fluent = "=x.y.z"` (i18n)
- `fluent-bundle`, `unic-langid`
- `strangecoin-core = { path = "../strangecoin-core" }` (pure types)
- `secp256k1` (for identicon address hashing in Avatar component)
- `blake3` (same)

---

## 10. Anti-goals

- **No new render backend in Stage 4.** Retain `eframe`/`egui`; Tauri+Web /
  iced / slint are alternatives considered and deferred (ADR-0031).
- **No mobile UI (iOS/Android).** Stage 5+ candidate (ADR-0033 future).
- **No custom design system framework.** Hand-rolled; no external egui
  design system dependency (none exists).
- **No live Theming editor.** Theme is config-driven (`ui.theme`); a UI
  editor for theme tokens is Stage 5+ nice-to-have.
- **No A/B testing framework.** Out of scope for Stage 4.

---

## 11. References

- [ADR-0031](../ADR/0031-professional-ui-replacement.md)
- [ADR-0009](../ADR/0009-events-bus.md) — NodeEvent bus (reused for dashboard live updates)
- [ADR-0024](../ADR/0024-storage-schema-and-engine.md) — Storage (indices for tx detail screen)
- [ADR-0028](../ADR/0028-strangecoin-as-cryptocurrency-for-ai-agents.md) — Agent vision (Agent Activity screen, key isolation in Settings/Keystore)
- [ADR-0029](../ADR/0029-mcp-server.md) — MCP server (Settings/MCP sub-screen toggles)
- [ADR-0030](../ADR/0030-acp-agent-communication-protocol.md) — ACP (Settings/ACP sub-screen toggles, Agent Activity content)
- [agent_integration.md](agent_integration.md)
- [acp_messages.md](acp_messages.md)
- [mcp_server.md](mcp_server.md)
- [ARCHITECT.md §19.3/§19.5/§19.6](../../analytics/ARCHITECT.md)
- [prompt-stage3.md §10 UI-P01..UI-P05](../../analytics/prompt-stage3.md)
- [WCAG 2.1 AA](https://www.w3.org/TR/WCAG21/)
- [axe-core](https://github.com/dequelabs/axe-core)
- [fluent](https://projectfluent.org/)
- [egui](https://github.com/emilk/egui)
