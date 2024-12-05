use miden_crypto::{hash::rpo::RpoDigest, Felt, Word, ZERO};
use miden_lib::transaction::TransactionKernel;
use miden_objects::{
    accounts::{Account, AccountId, StorageMap, StorageSlot},
    transaction::TransactionScript,
    vm::AdviceInputs,
    Digest,
};

use miden_tx::{testing::MockChain, TransactionExecutor};
use pm_accounts::{
    oracle::{OracleAccountBuilder, ORACLE_COMPONENT_LIBRARY},
    publisher::PublisherAccountBuilder,
    utils::{new_pk_and_authenticator, word_to_masm},
};
use pm_types::{Currency, Entry, Pair};
use std::sync::Arc;

#[test]
fn test_oracle_get_entry() {
    let entry = mock_entry();
    let entry_as_word: Word = entry.try_into().unwrap();
    let pair: Felt = entry_as_word[0];
    let pair_word: Word = [pair, ZERO, ZERO, ZERO];

    let (publisher_pub_key, _) = new_pk_and_authenticator([0_u8; 32]);
    let publisher_id = 12345_u64;
    let publisher_id_word = [Felt::new(publisher_id), ZERO, ZERO, ZERO];
    let publisher_account_id = AccountId::try_from(publisher_id).unwrap();
    let publisher_account = PublisherAccountBuilder::new(publisher_pub_key, publisher_account_id)
        .with_storage_slots(vec![
            // TODO: We need a leading empty map else indexing goes wrong.
            StorageSlot::empty_map(),
            // Entries map
            StorageSlot::Map(
                StorageMap::with_entries(vec![(
                    // The key is the pair id
                    RpoDigest::from(pair_word),
                    // The value is the entry
                    entry_as_word,
                )])
                .unwrap(),
            ),
        ])
        .build();

    let (oracle_pub_key, oracle_auth) = new_pk_and_authenticator([1_u8; 32]);
    let oracle_id = 98765_u64;
    let oracle_account_id = AccountId::try_from(oracle_id).unwrap();
    let oracle_account = OracleAccountBuilder::new(oracle_pub_key, oracle_account_id)
        .with_storage_slots(vec![
            // TODO: For some reasons, we have to add this map at index 0.
            StorageSlot::empty_map(),
            // Next publisher slot. Starts from idx 4 for our test since 3 is already populated.
            StorageSlot::Value([Felt::new(4), ZERO, ZERO, ZERO]),
            // Publisher registry
            StorageSlot::Map(
                StorageMap::with_entries(vec![(
                    RpoDigest::new(publisher_id_word),
                    [Felt::new(3), ZERO, ZERO, ZERO],
                )])
                .unwrap(),
            ),
            StorageSlot::Value(publisher_id_word),
        ])
        .build();

    let mut mock_chain =
        MockChain::with_accounts(&[publisher_account.clone(), oracle_account.clone()]);
    mock_chain.seal_block(None);

    let tx_script_code = format!(
        "
        use.oracle_component::oracle_module
        use.std::sys

        begin
            push.{pair}
            push.{publisher_id}

            call.oracle_module::get_entry

            # push.{entry} assert_eqw

            exec.sys::truncate_stack
        end
        ",
        pair = word_to_masm(pair_word),
        publisher_id = publisher_account.id(),
        entry = word_to_masm(entry_as_word),
    );

    let tx_script = TransactionScript::compile(
        tx_script_code,
        [],
        TransactionKernel::testing_assembler()
            .with_library(ORACLE_COMPONENT_LIBRARY.as_ref())
            .expect("adding oracle library should not fail")
            .with_debug_mode(true)
            .clone(),
    )
    .unwrap();

    let advice_inputs = FpiAdviceBuilder::new(&mock_chain)
        .with_account(&publisher_account)
        .build();

    let tx_context = mock_chain
        .build_tx_context(oracle_account.id(), &[], &[])
        .advice_inputs(advice_inputs)
        .tx_script(tx_script)
        .build();

    let mut executor =
        TransactionExecutor::new(Arc::new(tx_context.clone()), Some(oracle_auth.clone()))
            .with_debug_mode(true)
            .with_tracing();

    // load the foreign account's code into the transaction executor
    executor.load_account_code(publisher_account.code());

    // Show the next publisher slot
    println!(
        "==== ORACLE ====\n0: {:?}\n1: {:?}\n2: {:?}",
        // TODO: Item (0) is populated with something else? We expected a map?
        oracle_account.storage().get_item(0),
        // TODO: Item (1) is populated with something else?
        oracle_account.storage().get_item(1),
        // TODO: We have to use "2" even though it's supposed to be index 1.
        oracle_account.storage().get_item(2),
    );

    // Show the registered publisher
    println!(
        "{:?}",
        // TODO: We have to use "3" even though it's supposed to be index 2.
        oracle_account.storage().get_map_item(3, publisher_id_word)
    );

    // Show the expected entry
    println!(
        "==== PUBLISHER ====\n0: {:?}\n1: {:?}\nPublisher Pair slot: {:?}",
        publisher_account.storage().get_item(0),
        // TODO: This looks to be the leading empty map.. but why "1"?
        publisher_account.storage().get_map_item(1, pair_word),
        // TODO: We have to use "2" even though it's supposed to be index 1?
        publisher_account.storage().get_map_item(2, pair_word)
    );

    // execute the tx. The test assertion is made in the masm script.
    let _ = executor
        .execute_transaction(
            oracle_account.id(),
            tx_context.tx_inputs().block_header().block_num(),
            &[],
            tx_context.tx_args().clone(),
        )
        .unwrap();
}

#[test]
fn test_oracle_register_publisher() {
    let (oracle_pub_key, oracle_auth) = new_pk_and_authenticator([1_u8; 32]);
    let oracle_id = 98765_u64;
    let oracle_account_id = AccountId::try_from(oracle_id).unwrap();
    let mut oracle_account = OracleAccountBuilder::new(oracle_pub_key, oracle_account_id).build();

    let mut mock_chain = MockChain::with_accounts(&[oracle_account.clone()]);
    mock_chain.seal_block(None);

    let publisher_id = 12345_u64;
    let publisher_id_word = [ZERO, ZERO, ZERO, Felt::new(publisher_id)];
    let publisher_account_id = AccountId::try_from(publisher_id).unwrap();

    let tx_script_code = format!(
        "
        use.oracle_component::oracle_module
        use.std::sys

        begin
            push.{publisher_account_id}
            call.oracle_module::register_publisher
            exec.sys::truncate_stack
        end
        ",
    );

    let tx_script = TransactionScript::compile(
        tx_script_code,
        [],
        TransactionKernel::testing_assembler()
            .with_library(ORACLE_COMPONENT_LIBRARY.as_ref())
            .expect("adding oracle library should not fail")
            .with_debug_mode(true)
            .clone(),
    )
    .unwrap();

    let tx_context = mock_chain
        .build_tx_context(oracle_account.id(), &[], &[])
        .tx_script(tx_script)
        .build();

    let executor =
        TransactionExecutor::new(Arc::new(tx_context.clone()), Some(oracle_auth.clone()))
            .with_debug_mode(true)
            .with_tracing();

    // execute the tx. The test assertion is made in the masm script.
    let executed_transaction = executor
        .execute_transaction(
            oracle_account.id(),
            tx_context.tx_inputs().block_header().block_num(),
            &[],
            tx_context.tx_args().clone(),
        )
        .unwrap();

    oracle_account
        .apply_delta(executed_transaction.account_delta())
        .unwrap();

    assert_eq!(
        oracle_account
            .storage()
            .get_map_item(3, publisher_id_word)
            .unwrap(),
        [Felt::new(3), ZERO, ZERO, ZERO]
    );
    assert_eq!(
        oracle_account.storage().get_item(4).unwrap(),
        RpoDigest::new(publisher_id_word)
    );
    assert_eq!(
        oracle_account.storage().get_item(2).unwrap(),
        RpoDigest::new([Felt::new(4), ZERO, ZERO, ZERO])
    );
}

// HELPER FUNCTIONS
// ================================================================================================

/// Mocks [Entry] representing price feeds for use in tests.
fn mock_entry() -> Entry {
    Entry {
        pair: Pair::new(Currency::new("BTC").unwrap(), Currency::new("USD").unwrap()),
        price: 50000,
        decimals: 2,
        timestamp: 1732710094,
    }
}

/// Builder for constructing FPI (Foreign Procedure Invocation) advice inputs
pub struct FpiAdviceBuilder<'a> {
    chain: &'a MockChain,
    accounts: Vec<&'a Account>,
}

impl<'a> FpiAdviceBuilder<'a> {
    pub fn new(chain: &'a MockChain) -> Self {
        Self {
            chain,
            accounts: Vec::new(),
        }
    }

    /// Adds an account to the builder
    pub fn with_account(&mut self, account: &'a Account) -> &mut Self {
        self.accounts.push(account);
        self
    }

    /// Builds the AdviceInputs with all the configured accounts
    pub fn build(&self) -> AdviceInputs {
        let mut advice_map = Vec::new();
        let mut inputs = AdviceInputs::default().with_merkle_store(self.chain.accounts().into());

        for account in &self.accounts {
            let foreign_id_root = Digest::from([account.id().into(), ZERO, ZERO, ZERO]);
            let foreign_id_and_nonce = [account.id().into(), ZERO, ZERO, account.nonce()];
            let foreign_vault_root = account.vault().commitment();
            let foreign_storage_root = account.storage().commitment();
            let foreign_code_root = account.code().commitment();

            // Add account information to advice map
            advice_map.push((
                foreign_id_root,
                [
                    &foreign_id_and_nonce,
                    foreign_vault_root.as_elements(),
                    foreign_storage_root.as_elements(),
                    foreign_code_root.as_elements(),
                ]
                .concat(),
            ));

            // Add storage and code roots
            advice_map.push((foreign_storage_root, account.storage().as_elements()));
            advice_map.push((foreign_code_root, account.code().as_elements()));

            // Process storage slots
            for slot in account.storage().slots() {
                if let StorageSlot::Map(map) = slot {
                    // extend the merkle store and map with the storage maps
                    inputs.extend_merkle_store(map.inner_nodes());
                    // populate advice map with Sparse Merkle Tree leaf nodes
                    inputs.extend_map(
                        map.leaves()
                            .map(|(_, leaf)| (leaf.hash(), leaf.to_elements())),
                    );
                }
            }
        }

        // Add all collected advice map entries
        inputs.with_map(advice_map)
    }
}
