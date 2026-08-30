cargo.exe : warning: unused manifest key: wallet
строка:1 знак:1
+ & "C:\Users\user\.cargo\bin\cargo.exe" test hundred_transactions 2>&1 ...
+ ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
    + CategoryInfo          : NotSpecified: (warning: unused manifest key: wallet:String) [], RemoteException
    + FullyQualifiedErrorId : NativeCommandError
 
warning: unused import: `rand::Rng`
  --> src\main.rs:10:5
   |
10 | use rand::Rng;
   |     ^^^^^^^^^
   |
   = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

warning: unused import: `SIGNATURE_LENGTH`
 --> src\wallet.rs:5:55
  |
5 | use ed25519_dalek::{Signer, SigningKey, VerifyingKey, SIGNATURE_LENGTH};
  |                                                       ^^^^^^^^^^^^^^^^

warning: unused import: `PasswordHash`
 --> src\wallet.rs:8:29
  |
8 |         rand_core::RngCore, PasswordHash, PasswordHasher, SaltString,
  |                             ^^^^^^^^^^^^

warning: unused imports: `Read` and `Write`
  --> src\wallet.rs:15:15
   |
15 | use std::io::{Read, Write};
   |               ^^^^  ^^^^^

warning: unused import: `uuid::Uuid`
  --> src\wallet.rs:17:5
   |
17 | use uuid::Uuid;
   |     ^^^^^^^^^^

warning: use of deprecated function `base64::encode`: Use Engine::encode
    --> src\main.rs:1464:44
     |
1464 | ...                   if base64::encode(wallet.public_key.to_bytes()) == self.wallet_address {
     |                                  ^^^^^^
     |
     = note: `#[warn(deprecated)]` on by default

warning: use of deprecated function `base64::encode`: Use Engine::encode
    --> src\main.rs:1510:67
     |
1510 | ...                   self.wallet_address = base64::encode(wallet.public_key.to_bytes());
     |                                                     ^^^^^^

warning: use of deprecated function `base64::encode`: Use Engine::encode
    --> src\main.rs:1949:25
     |
1949 |                 base64::encode(sk.verifying_key().to_bytes())
     |                         ^^^^^^

warning: use of deprecated associated function `pbkdf2::password_hash::SaltString::new`: use `from_b64` instead
   --> src\wallet.rs:149:32
    |
149 |         let salt = SaltString::new(&keystore.salt)
    |                                ^^^

warning: value assigned to `chain_opt` is never read
   --> src\main.rs:217:49
    |
217 |         let mut chain_opt: Option<Vec<Block>> = None;
    |                                                 ^^^^ this value is reassigned later and never used
...
225 |             chain_opt = db_guard.get(b"chain").and_then(|v| serde_json::from_slice::<Vec<Block>>(&v).ok());
    |             --------- `chain_opt` is overwritten here before the previous value is read
    |
    = note: `#[warn(unused_assignments)]` (part of `#[warn(unused)]`) on by default

warning: value assigned to `balances_opt` is never read
   --> src\main.rs:218:62
    |
218 |         let mut balances_opt: Option<HashMap<String, u64>> = None;
    |                                                              ^^^^ this value is reassigned later and never used
...
228 |             balances_opt = db_guard.get(b"balances").and_then(|v| serde_json::from_slice::<HashMap<String, u64>>(
&v).ok());
    |             ------------ `balances_opt` is overwritten here before the previous value is read

warning: value assigned to `difficulty_opt` is never read
   --> src\main.rs:219:47
    |
219 |         let mut difficulty_opt: Option<u32> = None;
    |                                               ^^^^ this value is reassigned later and never used
...
231 |             difficulty_opt = db_guard.get(b"difficulty").and_then(|v| serde_json::from_slice::<u32>(&v).ok());
    |             ------------------------------------------------------------------------------------------------- `di
fficulty_opt` is overwritten here before the previous value is read

warning: variable does not need to be mutable
   --> src\main.rs:469:21
    |
469 |         if let Some(mut block) = block {
    |                     ----^^^^^
    |                     |
    |                     help: remove this `mut`
    |
    = note: `#[warn(unused_mut)]` (part of `#[warn(unused)]`) on by default

warning: unused variable: `total_duration`
   --> src\main.rs:575:21
    |
575 |                 let total_duration = SystemTime::now()
    |                     ^^^^^^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_total_duration`
    |
    = note: `#[warn(unused_variables)]` (part of `#[warn(unused)]`) on by default

warning: variable does not need to be mutable
    --> src\main.rs:1083:41
     |
1083 | ...                   let mut temp_blockchain: BlockchainDeserialize = match serde_json::from_str(blockchain_dat
a) {
     |                           ----^^^^^^^^^^^^^^^
     |                           |
     |                           help: remove this `mut`

warning: unused variable: `current_hash`
    --> src\main.rs:1186:13
     |
1186 |         let current_hash = blockchain.chain.last().map(|b| b.hash.clone()).unwrap_or_default();
     |             ^^^^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_current_hash`

warning: unused variable: `current_timestamp`
    --> src\main.rs:1188:13
     |
1188 |         let current_timestamp = blockchain.chain.last().map(|b| b.timestamp).unwrap_or(0);
     |             ^^^^^^^^^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_current_timestamp`

warning: unused variable: `current_balances`
    --> src\main.rs:1191:13
     |
1191 |         let current_balances = blockchain.balances.clone();
     |             ^^^^^^^^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_current_balances`

warning: unused variable: `wallet_address`
    --> src\main.rs:1192:13
     |
1192 |         let wallet_address = self.address.clone();
     |             ^^^^^^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_wallet_address`

warning: unused variable: `received_hash`
    --> src\main.rs:1265:37
     |
1265 | ...                   let received_hash = temp_blockchain.chain.last().map(|b| b.hash.clone()).unwrap_or_default
();
     |                           ^^^^^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_received_h
ash`

warning: unused variable: `received_timestamp`
    --> src\main.rs:1266:37
     |
1266 | ...                   let received_timestamp = temp_blockchain.chain.last().map(|b| b.timestamp).unwrap_or(0);
     |                           ^^^^^^^^^^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_recei
ved_timestamp`

warning: unused variable: `mining_progress`
    --> src\main.rs:1615:29
     |
1615 |                         let mining_progress = Arc::clone(&self.mining_progress);
     |                             ^^^^^^^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_mining
_progress`

warning: unused variable: `current_hash`
    --> src\main.rs:1391:17
     |
1391 |             let current_hash = blockchain.chain.last().map(|b| b.hash.clone()).unwrap_or_default();
     |                 ^^^^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_current_hash`

warning: unused variable: `received_hash`
    --> src\main.rs:1392:17
     |
1392 |             let received_hash = received_blockchain.chain.last().map(|b| b.hash.clone()).unwrap_or_default();
     |                 ^^^^^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_received_hash`

warning: fields `mining_thread` and `last_sync` are never read
   --> src\main.rs:144:5
    |
129 | struct WalletApp {
    |        --------- fields in this struct
...
144 |     mining_thread: Option<JoinHandle<()>>,
    |     ^^^^^^^^^^^^^
145 |     last_repaint: f64,
146 |     last_sync: f64,
    |     ^^^^^^^^^
    |
    = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: field `0` is never read
   --> src\main.rs:155:15
    |
155 |     Completed(Option<Block>),
    |     --------- ^^^^^^^^^^^^^
    |     |
    |     field in this variant
    |
    = note: `MiningStatus` has derived impls for the traits `Debug` and `Clone`, but these are intentionally ignored du
ring dead code analysis
help: consider changing the field to be of unit type to suppress this warning while preserving the field numbering, or 
remove the field
    |
155 -     Completed(Option<Block>),
155 +     Completed(()),
    |

warning: field `0` is never read
   --> src\main.rs:156:12
    |
156 |     Failed(String),
    |     ------ ^^^^^^
    |     |
    |     field in this variant
    |
    = note: `MiningStatus` has derived impls for the traits `Debug` and `Clone`, but these are intentionally ignored du
ring dead code analysis
help: consider changing the field to be of unit type to suppress this warning while preserving the field numbering, or 
remove the field
    |
156 -     Failed(String),
156 +     Failed(()),
    |

warning: field `private_key` is never read
  --> src\wallet.rs:32:9
   |
30 | pub struct Wallet {
   |            ------ field in this struct
31 |     pub public_key: VerifyingKey,
32 |     pub private_key: Option<SigningKey>,
   |         ^^^^^^^^^^^

warning: method `sign_transaction` is never used
   --> src\wallet.rs:191:12
    |
 35 | impl Wallet {
    | ----------- method in this implementation
...
191 |     pub fn sign_transaction(&self, transaction: &super::Transaction) -> Result<String, String> {
    |            ^^^^^^^^^^^^^^^^

warning: `strangecoin` (bin "strangecoin" test) generated 29 warnings (run `cargo fix --bin "strangecoin" -p strangecoi
n --tests` to apply 17 suggestions)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.75s
     Running unittests src\main.rs (target\debug\deps\strangecoin-20923a3268f5bd02.exe)

running 1 test
test tests::hundred_transactions_five_wallets ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.18s

