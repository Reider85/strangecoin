# Strangecoin UI — English (source locale)
# ≥100 strings covering all UI labels (UI-P02 КГ).

## Navigation / shell
app-title = Strangecoin
nav-wallet = Wallet
nav-dashboard = Dashboard
nav-transactions = Transactions
nav-agents = Agents
nav-settings = Settings
nav-back = Back
nav-home = Home

## Wallet
wallet-title = Wallet
wallet-send-tab = Send
wallet-history-tab = History
wallet-contracts-tab = Contracts
wallet-balance = Balance
wallet-address = Address
wallet-send-recipient = Recipient address
wallet-send-amount = Amount
wallet-send-fee = Fee
wallet-send-submit = Send
wallet-send-success = Transaction submitted: { $txid }
wallet-send-error-invalid-address = Invalid bech32 address
wallet-send-error-insufficient-balance = Insufficient balance
wallet-send-form-title = Send Transaction
wallet-recent-transactions = Recent Transactions
wallet-deployed-contracts = Deployed Contracts
wallet-view-details = View Details
wallet-view-abi = View ABI
wallet-interact = Interact
wallet-empty-history = No transactions yet
wallet-empty-contracts = No deployed contracts
wallet-send-batch = Batch send
wallet-send-batch-unavailable = Batch send is available after AI-P04
wallet-history-filter-all = All
wallet-history-filter-sent = Sent
wallet-history-filter-received = Received
wallet-history-filter-failed = Failed
wallet-history-col-txid = TxID
wallet-history-col-type = Type
wallet-history-col-from = From
wallet-history-col-to = To
wallet-history-col-amount = Amount
wallet-history-col-fee = Fee
wallet-history-col-status = Status
wallet-history-col-height = Height
wallet-history-prev = Previous
wallet-history-next = Next
wallet-contracts-read = Read
wallet-contracts-write = Write

## Dashboard
dashboard-title = Node Dashboard
dashboard-peers = Peers
dashboard-sync = Sync Status
dashboard-mempool = Mempool
dashboard-blocks = Recent Blocks
dashboard-total-balance = Total Balance
dashboard-pending-transactions = Pending Transactions
dashboard-network-hashrate = Network Hashrate
dashboard-active-agents = Active Agents
dashboard-recent-transactions = Recent Transactions
dashboard-tip-height = Tip Height
dashboard-sync-percent = { $percent }% synced
dashboard-connected-peers = { $count } connected
dashboard-uptime = Uptime
dashboard-version = Version

## Transaction detail
tx-detail-title = Transaction
tx-detail-receipt = Receipt
tx-detail-witness = Witness Proof
tx-detail-events = Events
tx-detail-overview = Overview
tx-detail-status = Status
tx-detail-block = Block
tx-detail-from = From
tx-detail-to = To
tx-detail-amount = Amount
tx-detail-fee = Fee
tx-detail-gas-used = Gas Used
tx-detail-timestamp = Timestamp
tx-detail-input-data = Input Data
tx-detail-raw-data = Raw Transaction Data
tx-detail-verify = Verify
tx-detail-load = Load Transaction
tx-detail-not-found = Transaction not found

## Agent activity
agent-activity-title = Agent Activity
agent-activity-filter = Filter
agent-activity-agents = Agents
agent-activity-activities = Activities
agent-activity-total-agents = Total Agents
agent-activity-online-agents = Online
agent-activity-offline-agents = Offline
agent-activity-error-agents = Errors
agent-activity-refresh = Refresh Activities
agent-activity-export = Export Activities
agent-activity-view-details = View Agent Details
agent-activity-empty = No agent activity

## Settings
settings-title = Settings
settings-general = General
settings-network = Network
settings-security = Security
settings-appearance = Appearance
settings-about = About
settings-peers = Peers
settings-keystore = Keystore
settings-config = Config
settings-mcp = MCP Server
settings-acp = ACP Transport
settings-save = Save Settings
settings-reset = Reset to Defaults
settings-language = Language
settings-currency = Currency
settings-theme = Theme
settings-font-size = Font Size
settings-data-directory = Data Directory

## Theme
theme-light = Light
theme-dark = Dark
theme-system = System

## Buttons / actions
button-ok = OK
button-cancel = Cancel
button-save = Save
button-delete = Delete
button-add = Add
button-edit = Edit
button-close = Close
button-confirm = Confirm
button-copy = Copy
button-refresh = Refresh
button-search = Search
button-filter = Filter
button-export = Export
button-import = Import
button-login = Log In
button-register = Register
button-send = Send
button-retry = Retry
button-apply = Apply

## Status / generic
loading = Loading…
error = Error
success = Success
warning = Warning
info = Info
status-online = Online
status-offline = Offline
status-pending = Pending
status-confirmed = Confirmed
status-failed = Failed
empty-state = Nothing to show
required-field = This field is required
invalid-format = Invalid format
unknown-error = An unknown error occurred
confirm-title = Are you sure?
unsaved-changes = You have unsaved changes
search-placeholder = Search…
filter-all = All
theme-switched = Theme switched to { $theme }
language-switched = Language switched to { $language }

## UI-P04: node dashboard
dashboard-current-height = Current Height
dashboard-sync-state = Sync State
dashboard-headers-queue = Headers Queue
dashboard-blocks-queue = Blocks Queue
dashboard-score-healthy = healthy
dashboard-score-warning = warning
dashboard-score-banned = banned
dashboard-no-peers = No peers connected
dashboard-peer-id = Peer ID
dashboard-peer-addr = Address
dashboard-peer-latency = Latency
dashboard-peer-score = Score
dashboard-peer-direction = Direction
dashboard-peer-capabilities = Capabilities
dashboard-total-fee = Total Fee
dashboard-no-mempool = Mempool is empty
dashboard-mempool-txid = TxID
dashboard-mempool-from = From
dashboard-mempool-to = To
dashboard-mempool-amount = Amount
dashboard-mempool-fee = Fee
dashboard-mempool-gas = Gas
dashboard-mempool-rbf = RBF conflict
dashboard-no-blocks = No blocks yet
dashboard-block-height = Height
dashboard-block-hash = Hash
dashboard-block-txs = Txs
dashboard-block-gas = Gas used
dashboard-block-miner = Miner
dashboard-block-timestamp = Timestamp
dashboard-block-tx-list = Transactions
dashboard-block-detail = Block Detail

## UI-P04: transaction detail
tx-detail-txid = Transaction ID
tx-detail-kind = Type
tx-detail-status-pending = Pending
tx-detail-copy = Copy
tx-detail-gas-limit = Gas Limit
tx-detail-revert = Revert Reason
tx-detail-events-empty = No events
tx-detail-receipt-unavailable = Receipt unavailable (only the tip block is retained)
tx-detail-verified = verified
tx-detail-verify-failed = verification failed
tx-detail-witness-unavailable = Press Verify to load the proof
tx-detail-witness-account = Account proof
tx-detail-witness-storage = Storage proof
tx-detail-witness-raw = Witness wire proof
tx-detail-event-topics = Topics
tx-detail-event-data = Data
tx-detail-no-selection = Select a transaction
tx-detail-loading = Loading transaction details…

## UI-P05: agent activity
agent-activity-feed-title = Activity Feed
agent-activity-filter-title = Filters
agent-activity-filter-agent = Agent address
agent-activity-filter-types = Event types
agent-activity-filter-from = From timestamp
agent-activity-filter-to = To timestamp
agent-activity-filter-clear = Clear filters
agent-activity-col-time = Time
agent-activity-col-agent = Agent
agent-activity-col-type = Type
agent-activity-col-details = Details
agent-activity-stub = stub - live source unavailable
agent-activity-open-tx = Open transaction
agent-activity-open-acp = View ACP message
agent-activity-acp-detail = ACP message detail
agent-activity-no-match = No matching activity
agent-event-acp-negotiate = ACP negotiate
agent-event-acp-task = ACP task
agent-event-acp-settle = ACP settle
agent-event-acp-revoke = ACP revoke
agent-event-batched-tx = Batched transaction
agent-event-sponsor-tx = Sponsored transaction
agent-event-transfer = Transfer

## UI-P05: settings
settings-network-listen = Listen address
settings-network-seeds = Seed nodes
settings-network-max-peers = Max peers
settings-network-id = Network ID
settings-read-only = read-only after start
settings-peers-ban = Ban
settings-peers-unban = Unban
settings-peers-add-seed = Add seed
settings-peers-banned = banned
settings-keystore-operator = Operator keystore
settings-keystore-agent = Agent keystore
settings-keystore-import = Import
settings-keystore-export = Export
settings-keystore-change-password = Change password
settings-keystore-derive = Derive new key
settings-keystore-unavailable = keystore isolation is not available in this build
settings-config-raw = config.toml
settings-config-validate = Validate
settings-config-save-restart = Save and Restart
settings-config-invalid = invalid config
settings-mcp-enable = Enable MCP server
settings-mcp-transport = Transport
settings-mcp-listen = HTTP listen address
settings-mcp-clients = Connected clients
settings-acp-enable = Enable ACP transport
settings-acp-sessions = Active sessions
settings-acp-ban-agent = Ban agent
settings-status-saved = saved
settings-status-error = error
