use crate::SuzukaFullNode;
use m1_da_light_node_client::{
	blob_response, BatchWriteRequest, BlobWrite, LightNodeServiceClient,
	StreamReadFromHeightRequest,
};
use mcr_settlement_client::{McrSettlementClient, McrSettlementClientOperations};
use mcr_settlement_manager::{
	CommitmentEventStream, McrSettlementManager, McrSettlementManagerOperations,
};
use movement_types::{Block, BlockCommitmentEvent};
use suzuka_executor::{
	v1::SuzukaExecutorV1, ExecutableBlock, ExecutableTransactions, FinalityMode, HashValue,
	SignatureVerifiedTransaction, SignedTransaction, SuzukaExecutor, Transaction,
};

use anyhow::Context;
use async_channel::{Receiver, Sender};
use sha2::Digest;
use tokio::sync::RwLock;
use tokio_stream::StreamExt;

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

pub struct SuzukaPartialNode<T> {
	executor: T,
	transaction_sender: Sender<SignedTransaction>,
	pub transaction_receiver: Receiver<SignedTransaction>,
	light_node_client: Arc<RwLock<LightNodeServiceClient<tonic::transport::Channel>>>,
	settlement_manager: McrSettlementManager,
}

impl<T> SuzukaPartialNode<T>
where
	T: SuzukaExecutor + Send + Sync,
{
	pub fn new<C>(
		executor: T,
		light_node_client: LightNodeServiceClient<tonic::transport::Channel>,
		settlement_client: C,
	) -> (Self, impl Future<Output = Result<(), anyhow::Error>> + Send)
	where
		C: McrSettlementClientOperations + Send + 'static,
	{
		let (settlement_manager, commitment_events) = McrSettlementManager::new(settlement_client);
		let (transaction_sender, transaction_receiver) = async_channel::unbounded();
		(
			Self {
				executor,
				transaction_sender,
				transaction_receiver,
				light_node_client: Arc::new(RwLock::new(light_node_client)),
				settlement_manager,
			},
			read_commitment_events(commitment_events),
		)
	}

	fn bind_transaction_channel(&mut self) {
		self.executor.set_tx_channel(self.transaction_sender.clone());
	}

	pub fn bound<C>(
		executor: T,
		light_node_client: LightNodeServiceClient<tonic::transport::Channel>,
		settlement_client: C,
	) -> Result<(Self, impl Future<Output = Result<(), anyhow::Error>> + Send), anyhow::Error>
	where
		C: McrSettlementClientOperations + Send + 'static,
	{
		let (mut node, background_task) = Self::new(executor, light_node_client, settlement_client);
		node.bind_transaction_channel();
		Ok((node, background_task))
	}

    pub async fn tick_write_transactions_to_da(&self) -> Result<(), anyhow::Error> {
        
        // limit the total time batching transactions
        let start_time = std::time::Instant::now();
        let end_time = start_time + std::time::Duration::from_millis(100);
        
        let mut transactions = Vec::new();


        while let Ok(transaction_result) = tokio::time::timeout(Duration::from_millis(100), self.transaction_receiver.recv()).await {

            match transaction_result {
                Ok(transaction) => {
                    println!("Got transaction: {:?}", transaction);
                    let serialized_transaction = serde_json::to_vec(&transaction)?;
                    transactions.push(BlobWrite {
                        data: serialized_transaction
                    });
                },
                Err(_) => {
                    break;
                }
            }

            if std::time::Instant::now() > end_time {
                break;
            }
        }

        if transactions.len() > 0 {
            let client_ptr = self.light_node_client.clone();
            let mut light_node_client = client_ptr.write().await;
            light_node_client.batch_write(
                BatchWriteRequest {
                    blobs: transactions
                }
            ).await?;
            println!("Wrote transactions to DA");
        }

        Ok(())


    }

    pub async fn write_transactions_to_da(&self) -> Result<(), anyhow::Error> {
        
        loop {
            self.tick_write_transactions_to_da().await?;
        }

    }

    // receive transactions from the transaction channel and send them to be executed
    // ! This assumes the m1 da light node is running sequencer mode
    pub async fn read_blocks_from_da(&self) -> Result<(), anyhow::Error> {

        let block_head_height = self.executor.get_block_head_height().await?;

        let mut stream = {
            let client_ptr = self.light_node_client.clone();
            let mut light_node_client =  client_ptr.write().await;
            light_node_client.stream_read_from_height(
                StreamReadFromHeightRequest {
                    height: block_head_height,
                }
            ).await?
        }.into_inner();

		while let Some(blob) = stream.next().await {
			println!("Stream hot!");
			// get the block
			let block_bytes = match blob?
				.blob
				.ok_or(anyhow::anyhow!("No blob in response"))?
				.blob_type
				.ok_or(anyhow::anyhow!("No blob type in response"))?
			{
				blob_response::BlobType::SequencedBlobBlock(blob) => blob.data,
				_ => {
					anyhow::bail!("Invalid blob type in response")
				},
			};

			// get the block
			let block: Block = serde_json::from_slice(&block_bytes)?;
			println!("Received block: {:?}", block);

			// get the transactions
			let mut block_transactions = Vec::new();
			for transaction in block.transactions {
				let signed_transaction: SignedTransaction = serde_json::from_slice(&transaction.0)?;
				let signature_verified_transaction = SignatureVerifiedTransaction::Valid(
					Transaction::UserTransaction(signed_transaction),
				);
				block_transactions.push(signature_verified_transaction);
			}

			// form the executable transactions vec
			let block = ExecutableTransactions::Unsharded(block_transactions);

			// hash the block bytes
			let mut hasher = sha2::Sha256::new();
			hasher.update(&block_bytes);
			let slice = hasher.finalize();
			let block_hash = HashValue::from_slice(slice.as_slice())?;

			// form the executable block and execute it
			let executable_block = ExecutableBlock::new(block_hash, block);
			let block_id = executable_block.block_id;
			let commitment =
				self.executor.execute_block(FinalityMode::Opt, executable_block).await?;

			println!("Executed block: {:?}", block_id);

			self.settlement_manager.post_block_commitment(commitment).await?;
		}

		Ok(())
	}
}

async fn read_commitment_events(mut stream: CommitmentEventStream) -> anyhow::Result<()> {
	while let Some(res) = stream.next().await {
		let event = res?;
		match event {
			BlockCommitmentEvent::Accepted(commitment) => {
				println!("Commitment accepted: {:?}", commitment);
			},
			BlockCommitmentEvent::Rejected { height, reason } => {
				println!("Commitment at height {height} rejected: {reason:?}");
			},
		}
	}
	Ok(())
}

impl<T> SuzukaFullNode for SuzukaPartialNode<T>
where
	T: SuzukaExecutor + Send + Sync,
{
	/// Runs the services until crash or shutdown.
	async fn run_services(&self) -> Result<(), anyhow::Error> {
		self.executor.run_service().await?;

		Ok(())
	}

	/// Runs the background tasks until crash or shutdown.
	async fn run_background_tasks(&self) -> Result<(), anyhow::Error> {
		self.executor.run_background_tasks().await?;

		Ok(())
	}

	// ! Currently this only implements opt.
	/// Runs the executor until crash or shutdown.
	async fn run_executor(&self) -> Result<(), anyhow::Error> {
		// wait for both tasks to finish
		tokio::try_join!(self.write_transactions_to_da(), self.read_blocks_from_da())?;

		Ok(())
	}
}

impl SuzukaPartialNode<SuzukaExecutorV1> {
	pub async fn try_from_env(
	) -> Result<(Self, impl Future<Output = Result<(), anyhow::Error>> + Send), anyhow::Error> {
		let (tx, _) = async_channel::unbounded();
		let light_node_client = LightNodeServiceClient::connect("http://[::1]:30730").await?;
		let executor = SuzukaExecutorV1::try_from_env(tx)
			.await
			.context("Failed to get executor from environment")?;
		let settlement_client = McrSettlementClient::new();
		Self::bound(executor, light_node_client, settlement_client)
	}
}
