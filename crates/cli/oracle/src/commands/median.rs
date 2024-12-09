use std::str::FromStr;
use miden_client::transactions::{TransactionKernel, TransactionRequest, TransactionScript};
use miden_client::{accounts::AccountId, crypto::FeltRng};
use miden_client::{Client, ZERO,Felt};
use pm_accounts::publisher::PUBLISHER_COMPONENT_LIBRARY;
use pm_accounts::utils::word_to_masm;
use pm_types::Pair;
use pm_utils_cli::{JsonStorage, ORACLE_ACCOUNT_COLUMN, PRAGMA_ACCOUNTS_STORAGE_FILE};

#[derive(clap::Parser, Debug, Clone)]
#[clap(about = "Creates a new Oracle Account")]
pub struct MedianCmd {
    // Input pair (format example: "BTC/USD")
    pair: String,
}

impl MedianCmd {
    pub async fn call(&self, client: &mut Client<impl FeltRng>) -> anyhow::Result<()> {
        let pragma_storage = JsonStorage::new(PRAGMA_ACCOUNTS_STORAGE_FILE)?;
        let oracle_id = pragma_storage.get_key(ORACLE_ACCOUNT_COLUMN).unwrap();

        let oracle_id = AccountId::from_hex(oracle_id).unwrap();

        let (_, _) = client.get_account(oracle_id).await.unwrap();
        let pair: Pair = Pair::from_str(&self.pair).unwrap();

        let pair_id_felt: Felt = pair.try_into().unwrap();

        let tx_script_code = format!(
            "
            use.oracle_component::oracle_module
            use.std::sys
    
            begin
                push.{pair}
                call.oracle_module::get_median
                debug.stack
            end
            ",
            pair = word_to_masm([pair_id_felt, ZERO, ZERO, ZERO]),
        );

        // TODO: Can we pipe stdout to a variable so we can see the stack??

        let median_script = TransactionScript::compile(
            tx_script_code,
            [],
            TransactionKernel::testing_assembler()
                .with_library(PUBLISHER_COMPONENT_LIBRARY.as_ref())
                .map_err(|e| {
                    anyhow::anyhow!("Error while setting up the component library: {e:?}")
                })?
                .clone(),
        )
        .map_err(|e| anyhow::anyhow!("Error while compiling the script: {e:?}"))?;

        // TODO: Do we need an advice ?

        let transaction_request = TransactionRequest::new()
            .with_custom_script(median_script)
            .map_err(|e| anyhow::anyhow!("Error while building transaction request: {e:?}"))?;

        let tx_result = client
            .new_transaction(oracle_id, transaction_request)
            .await
            .map_err(|e| anyhow::anyhow!("Error while creating a transaction: {e:?}"))?;

        client
            .submit_transaction(tx_result.clone())
            .await
            .map_err(|e| anyhow::anyhow!("Error while submitting a transaction: {e:?}"))?;
        
        Ok(())
    }
}
