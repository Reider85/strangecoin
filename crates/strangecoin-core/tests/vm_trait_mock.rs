use std::collections::HashMap;

use strangecoin_core::types::{Account, BlockHeader};
use strangecoin_core::{
    BlockContext, ExecutionContext, ExecutionResult, GasCounter, Log, Opcode, Receipt, TxStatus,
    VmError, VmExecutor,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct MockError(VmError);

impl From<MockError> for VmError {
    fn from(err: MockError) -> Self {
        err.0
    }
}

struct MockVmExecutor {
    result: Result<ExecutionResult, MockError>,
}

impl VmExecutor for MockVmExecutor {
    type Error = MockError;

    fn execute(&self, _ctx: ExecutionContext) -> Result<ExecutionResult, Self::Error> {
        self.result.clone()
    }

    fn gas_cost(&self, opcode: Opcode) -> u64 {
        match opcode {
            Opcode::Instruction => 1,
            Opcode::HostCall => 100,
            Opcode::MemoryGrow => 3,
            Opcode::Other => 5,
        }
    }

    fn register_code(&mut self, _address: &str, _wasm: Vec<u8>) -> Result<(), VmError> {
        Ok(())
    }
}

fn sample_header() -> BlockHeader {
    BlockHeader {
        index: 7,
        timestamp: 1_700_000_000,
        previous_hash: "prev".to_string(),
        hash: "hash".to_string(),
        nonce: 42,
        target: "00ff".to_string(),
        consensus_version: 1,
        state_root: [1u8; 32],
        tx_root: [2u8; 32],
        receipts_root: [0u8; 32],
    }
}

fn sample_context() -> ExecutionContext {
    let mut accounts = HashMap::new();
    accounts.insert("sc1alice".to_string(), Account::eoa(100, 1));
    accounts.insert("sc1contract".to_string(), Account::default());
    let mut storage_overlay = HashMap::new();
    storage_overlay.insert("sc1contract".to_string(), {
        let mut slots = HashMap::new();
        slots.insert([9u8; 32], vec![1, 2, 3]);
        slots
    });
    ExecutionContext {
        block: sample_header(),
        accounts,
        caller: "sc1alice".to_string(),
        callee: "sc1contract".to_string(),
        value: 5,
        gas_limit: 100_000,
        gas_used: 1_500,
        depth: 0,
        reentrant: false,
        read_only: false,
        block_context: BlockContext {
            height: 7,
            timestamp: 1_700_000_000,
            coinbase: "sc1miner".to_string(),
            block_gas_limit: 1_000_000,
        },
        storage: Default::default(),
        storage_overlay,
        storage_address: "sc1contract".to_string(),
        input: Vec::new(),
    }
}

#[test]
fn mock_vm_success_result() {
    let log = Log {
        address: "sc1contract".to_string(),
        topics: vec![[3u8; 32]],
        data: vec![0xde, 0xad],
    };
    let vm = MockVmExecutor {
        result: Ok(ExecutionResult {
            gas_used: 2_300,
            logs: vec![log.clone()],
            status: TxStatus::Success,
            return_data: vec![42],
            storage_overlay: HashMap::new(),
            value_transfers: Vec::new(),
        }),
    };
    let ctx = sample_context();
    let res = vm.execute(ctx).expect("mock succeeds");
    assert_eq!(res.gas_used, 2_300);
    assert_eq!(res.status, TxStatus::Success);
    assert_eq!(res.return_data, vec![42]);
    assert_eq!(res.logs, vec![log]);
}

#[test]
fn mock_vm_error_converts_into_vm_error() {
    let vm = MockVmExecutor {
        result: Err(MockError(VmError::OutOfGas)),
    };
    let err: VmError = vm.execute(sample_context()).expect_err("mock fails").into();
    assert_eq!(err, VmError::OutOfGas);
}

#[test]
fn mock_vm_sees_full_execution_context() {
    let vm = MockVmExecutor {
        result: Ok(ExecutionResult {
            gas_used: 0,
            logs: vec![],
            status: TxStatus::Success,
            return_data: vec![],
            storage_overlay: HashMap::new(),
            value_transfers: Vec::new(),
        }),
    };
    let ctx = sample_context();
    assert_eq!(ctx.block.index, 7);
    assert_eq!(ctx.accounts.len(), 2);
    assert_eq!(ctx.accounts["sc1alice"].balance(), 100);
    assert_eq!(ctx.caller, "sc1alice");
    assert_eq!(ctx.callee, "sc1contract");
    assert_eq!(ctx.value, 5);
    assert_eq!(ctx.gas_limit, 100_000);
    assert_eq!(ctx.gas_used, 1_500);
    assert_eq!(ctx.depth, 0);
    assert!(!ctx.reentrant);
    assert_eq!(ctx.block_context.block_gas_limit, 1_000_000);
    assert_eq!(
        ctx.storage_overlay["sc1contract"][&[9u8; 32]],
        vec![1, 2, 3]
    );
    let res = vm.execute(ctx).expect("ok");
    assert_eq!(res.gas_used, 0);
}

#[test]
fn gas_cost_per_opcode() {
    let vm = MockVmExecutor {
        result: Ok(ExecutionResult {
            gas_used: 0,
            logs: vec![],
            status: TxStatus::Success,
            return_data: vec![],
            storage_overlay: HashMap::new(),
            value_transfers: Vec::new(),
        }),
    };
    assert_eq!(vm.gas_cost(Opcode::Instruction), 1);
    assert_eq!(vm.gas_cost(Opcode::HostCall), 100);
    assert_eq!(vm.gas_cost(Opcode::MemoryGrow), 3);
    assert_eq!(vm.gas_cost(Opcode::Other), 5);
}

#[test]
fn receipt_carries_gas_logs_status() {
    let receipt = Receipt {
        txid: [7u8; 32],
        gas_used: 5_000,
        logs: vec![Log {
            address: "sc1contract".to_string(),
            topics: vec![],
            data: vec![],
        }],
        status: TxStatus::Reverted,
        block_height: 3,
        tx_index: 1,
        value_transfers: Vec::new(),
    };
    assert_eq!(receipt.gas_used, 5_000);
    assert_eq!(receipt.status, TxStatus::Reverted);
    assert_eq!(receipt.logs.len(), 1);
    assert_eq!(receipt.block_height, 3);
    assert_eq!(receipt.tx_index, 1);
}

struct FixedGasCounter {
    limit: u64,
    used: u64,
}

impl GasCounter for FixedGasCounter {
    fn spend(&mut self, gas: u64) -> Result<(), VmError> {
        if self.limit - self.used < gas {
            return Err(VmError::OutOfGas);
        }
        self.used += gas;
        Ok(())
    }

    fn gas_left(&self) -> u64 {
        self.limit - self.used
    }

    fn gas_used(&self) -> u64 {
        self.used
    }
}

#[test]
fn gas_counter_spend_and_out_of_gas() {
    let mut counter = FixedGasCounter {
        limit: 100,
        used: 0,
    };
    counter.spend(60).expect("first spend ok");
    assert_eq!(counter.gas_left(), 40);
    assert_eq!(counter.gas_used(), 60);
    assert_eq!(counter.spend(60), Err(VmError::OutOfGas));
    counter.spend(40).expect("exact remainder ok");
    assert_eq!(counter.gas_left(), 0);
}
