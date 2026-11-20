# Strangecoin Agent API

A JSON-RPC 2.0 interface for agent operations in Strangecoin, implementing the agent_* methods specified in AI-P04.

## Features

- **Session Management**: Agent session lifecycle and authentication
- **Transaction Batching**: Batch transaction submission and processing
- **Agent Operations**: Balance queries, nonce management, activity tracking
- **Sponsorship**: Transaction sponsorship and management
- **Security**: Session-based authentication with rate limiting
- **Integration**: Seamless integration with existing Strangecoin components

## Quick Start

### Server Setup

```rust
use strangecoin_api::{AgentApiServer, ServerConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ServerConfig::default();
    let server = AgentApiServer::new(config).await?;
    server.serve().await?;
    
    Ok(())
}
```

### Basic Usage

```rust
use strangecoin_api::{AgentApiServer, ServerConfig};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ServerConfig {
        listen_addr: "127.0.0.1:9002".parse().unwrap(),
        session_timeout: 3600,
        max_sessions_per_agent: 10,
        enable_cors: true,
        enable_rate_limit: true,
        rate_limit_per_minute: 60,
    };
    
    let server = AgentApiServer::new(config).await?;
    server.serve().await?;
    
    Ok(())
}
```

## API Endpoints

### JSON-RPC Endpoint

- **URL**: `POST /rpc`
- **Content-Type**: `application/json`
- **Authentication**: `Authorization: Bearer <session_token>`

### Health Check

- **URL**: `GET /health`
- **Response**: JSON with server status and version

## Available Methods

### Session Management

#### `agent_getSession`

Create or retrieve an agent session.

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "agent_getSession",
  "params": {}
}
```

#### `agent_getSessionInfo`

Get detailed session information.

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "agent_getSessionInfo",
  "params": {}
}
```

#### `agent_closeSession`

Close an active session.

```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "agent_closeSession",
  "params": {}
}
```

### Transaction Batching

#### `agent_signAndSubmitBatch`

Sign and submit a batch of transactions.

```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "method": "agent_signAndSubmitBatch",
  "params": {
    "transactions": [
      {
        "transfer": {
          "from": "0x1234567890abcdef",
          "to": "0xabcdef1234567890",
          "amount": "1000000"
        }
      }
    ]
  }
}
```

#### `agent_getBatchStatus`

Get batch processing status.

```json
{
  "jsonrpc": "2.0",
  "id": 5,
  "method": "agent_getBatchStatus",
  "params": {
    "batch_id": "batch_123"
  }
}
```

#### `agent_getBatchReceipts`

Get batch execution receipts.

```json
{
  "jsonrpc": "2.0",
  "id": 6,
  "method": "agent_getBatchReceipts",
  "params": {
    "batch_id": "batch_123"
  }
}
```

### Agent Management

#### `agent_getActivity`

Get agent activity feed.

```json
{
  "jsonrpc": "2.0",
  "id": 7,
  "method": "agent_getActivity",
  "params": {
    "limit": 10,
    "offset": 0
  }
}
```

#### `agent_getBalance`

Get agent balance.

```json
{
  "jsonrpc": "2.0",
  "id": 8,
  "method": "agent_getBalance",
  "params": {}
}
```

#### `agent_getNonce`

Get agent nonce for transaction signing.

```json
{
  "jsonrpc": "2.0",
  "id": 9,
  "method": "agent_getNonce",
  "params": {}
}
```

### Sponsorship

#### `agent_sponsorTransaction`

Sponsor a transaction from agent account.

```json
{
  "jsonrpc": "2.0",
  "id": 10,
  "method": "agent_sponsorTransaction",
  "params": {
    "beneficiary": "0x1234567890abcdef",
    "amount": "1000000",
    "max_fee": "10000"
  }
}
```

#### `agent_getSponsorships`

Get active sponsorships.

```json
{
  "jsonrpc": "2.0",
  "id": 11,
  "method": "agent_getSponsorships",
  "params": {}
}
```

#### `agent_revokeSponsorship`

Cancel an active sponsorship.

```json
{
  "jsonrpc": "2.0",
  "id": 12,
  "method": "agent_revokeSponsorship",
  "params": {
    "sponsorship_id": "spon_123"
  }
}
```

## Configuration

### Server Configuration

```rust
let config = ServerConfig {
    listen_addr: "127.0.0.1:9002".parse().unwrap(),
    session_timeout: 3600, // seconds
    max_sessions_per_agent: 10,
    enable_cors: true,
    enable_rate_limit: true,
    rate_limit_per_minute: 60,
};
```

### Session Permissions

```rust
use strangecoin_api::session::SessionPermission;

let permissions = vec![
    SessionPermission::ReadAccount,
    SessionPermission::SubmitTransaction,
    SessionPermission::ManageBatches,
    SessionPermission::SponsorTransactions,
];
```

## Security

### Authentication

The API uses session-based authentication with Bearer tokens:

1. Create a session through the authentication system
2. Use the session token in the `Authorization` header
3. Token includes digital signature for integrity

### Rate Limiting

- Configurable rate limits per session
- Request counting with time windows
- Automatic rejection when limits exceeded

### CORS Support

- Configurable CORS headers
- Support for cross-origin requests
- Secure by default

## Integration

### With Existing Strangecoin Components

The agent API integrates seamlessly with existing Strangecoin components:

- **BlockchainFacade**: All blockchain operations
- **ACP**: Agent communication protocol
- **Event System**: Real-time updates
- **Configuration**: System-wide settings

### External Integration

The JSON-RPC interface can be used by:

- Web applications
- Mobile applications
- External services
- Development tools

## Testing

Run the test suite:

```bash
cargo test
```

Run specific test modules:

```bash
cargo test session_tests
cargo test auth_tests
cargo test method_tests
cargo test integration
```

## Examples

See the `examples/` directory for complete usage examples:

- `basic_usage.rs`: Basic server setup and usage
- `session_management.rs`: Advanced session management
- `batch_operations.rs`: Transaction batching examples

## Error Handling

All methods return JSON-RPC 2.0 responses with proper error handling:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32001,
    "message": "Session not found",
    "data": null
  },
  "result": null
}
```

Common error codes:
- `-32001`: Session not found
- `-32002`: Session expired
- `-32003`: Invalid session token
- `-32004`: Authentication failed
- `-32005`: Permission denied
- `-32006`: Invalid parameters
- `-32007`: Transaction validation failed
- `-32008`: Batch processing failed
- `-32009`: Blockchain operation failed
- `-32010`: Internal server error
- `-32011`: Rate limit exceeded
- `-32012`: Method not found
- `-32013`: Session limit exceeded
- `-32014`: Sponsorship not found
- `-32015`: Invalid signature
- `-32016`: Agent not found

## License

Apache 2.0