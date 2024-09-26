use crate::utils::MovementAddress;
use anyhow::Result;
use aptos_api::accounts::Account;
use aptos_api_types::{Address, EntryFunctionId, MoveModuleId, ViewFunction, ViewRequest};
use aptos_sdk::{
	move_types::{
		identifier::Identifier,
		language_storage::{ModuleId, TypeTag},
	},
	rest_client::{Client, FaucetClient, Response},
	types::LocalAccount,
};
use aptos_types::account_address::AccountAddress;
use bridge_shared::{
	bridge_contracts::{
		BridgeContractCounterparty, BridgeContractCounterpartyError,
		BridgeContractCounterpartyResult, BridgeContractInitiator, BridgeContractInitiatorError,
		BridgeContractInitiatorResult,
	},
	types::{
		Amount, AssetType, BridgeTransferDetails, BridgeTransferId, HashLock, HashLockPreImage,
		InitiatorAddress, RecipientAddress, TimeLock,
	},
};
use hex::{decode, FromHex};
use rand::prelude::*;
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::str::FromStr;
use std::sync::{Arc, RwLock};
use std::{
	env, fs,
	io::{Read, Write},
	path::{Path, PathBuf},
	process::{Command, Stdio},
};
use tokio::{
	io::{AsyncBufReadExt, BufReader},
	process::Command as TokioCommand,
	sync::oneshot,
	task,
};
use tracing::{debug, info};

use url::Url;

mod types;
pub mod utils;

const DUMMY_ADDRESS: AccountAddress = AccountAddress::new([0; 32]);
const COUNTERPARTY_MODULE_NAME: &str = "atomic_bridge_counterparty";

#[allow(dead_code)]
enum Call {
	Lock,
	Complete,
	Abort,
	GetDetails,
}

pub struct Config {
	pub rpc_url: Option<String>,
	pub ws_url: Option<String>,
	pub chain_id: String,
	pub signer_private_key: Arc<RwLock<LocalAccount>>,
	pub initiator_contract: Option<MovementAddress>,
	pub gas_limit: u64,
}

impl Config {
	pub fn build_for_test() -> Self {
		let seed = [3u8; 32];
		let mut rng = rand::rngs::StdRng::from_seed(seed);

		Config {
			rpc_url: Some("http://localhost:8080".parse().unwrap()),
			ws_url: Some("ws://localhost:8080".parse().unwrap()),
			chain_id: 4.to_string(),
			signer_private_key: Arc::new(RwLock::new(LocalAccount::generate(&mut rng))),
			initiator_contract: None,
			gas_limit: 10_000_000_000,
		}
	}
}

#[allow(dead_code)]
#[derive(Clone)]
pub struct MovementClient {
	///Address of the counterparty moduke
	pub counterparty_address: AccountAddress,
	///Address of the initiator module
	initiator_address: Vec<u8>,
	///The Apotos Rest Client
	pub rest_client: Client,
	///The Apotos Rest Client
	pub faucet_client: Option<Arc<RwLock<FaucetClient>>>,
	///The signer account
	signer: Arc<LocalAccount>,
}

impl MovementClient {
	pub async fn new(_config: Config) -> Result<Self, anyhow::Error> {
		let node_connection_url = "http://127.0.0.1:8080".to_string();
		let node_connection_url = Url::from_str(node_connection_url.as_str())
			.map_err(|_| BridgeContractCounterpartyError::SerializationError)?;

		let rest_client = Client::new(node_connection_url.clone());

		let seed = [3u8; 32];
		let mut rng = rand::rngs::StdRng::from_seed(seed);
		let signer = LocalAccount::generate(&mut rng);

		let mut address_bytes = [0u8; AccountAddress::LENGTH];
		address_bytes[0..2].copy_from_slice(&[0xca, 0xfe]);
		let counterparty_address = AccountAddress::new(address_bytes);
		Ok(MovementClient {
			counterparty_address,
			initiator_address: Vec::new(), //dummy for now
			rest_client,
			faucet_client: None,
			signer: Arc::new(signer),
		})
	}

	pub async fn new_for_test(
		_config: Config,
	) -> Result<(Self, tokio::process::Child), anyhow::Error> {
		let kill_cmd = TokioCommand::new("sh")
			.arg("-c")
			.arg("PID=$(ps aux | grep 'movement node run-local-testnet' | grep -v grep | awk '{print $2}' | head -n 1); if [ -n \"$PID\" ]; then kill -9 $PID; fi")
			.output()
			.await?;

		if !kill_cmd.status.success() {
			println!("Failed to kill running movement process: {:?}", kill_cmd.stderr);
		} else {
			println!("Movement process killed if it was running.");
		}

		let delete_dir_cmd = TokioCommand::new("sh")
			.arg("-c")
			.arg("if [ -d '.movement' ]; then rm -rf .movement; fi")
			.output()
			.await?;

		if !delete_dir_cmd.status.success() {
			println!("Failed to delete .movement directory: {:?}", delete_dir_cmd.stderr);
		} else {
			println!(".movement directory deleted if it was present.");
		}

		let (setup_complete_tx, mut setup_complete_rx) = oneshot::channel();
		let mut child = TokioCommand::new("movement")
			.args(&["node", "run-local-testnet", "--force-restart", "--assume-yes"])
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.spawn()?;

		let stdout = child.stdout.take().expect("Failed to capture stdout");
		let stderr = child.stderr.take().expect("Failed to capture stderr");

		task::spawn(async move {
			let mut stdout_reader = BufReader::new(stdout).lines();
			let mut stderr_reader = BufReader::new(stderr).lines();

			loop {
				tokio::select! {
					line = stdout_reader.next_line() => {
						match line {
							Ok(Some(line)) => {
								println!("STDOUT: {}", line);
								if line.contains("Setup is complete") {
									println!("Testnet is up and running!");
									let _ = setup_complete_tx.send(());
																	return Ok(());
								}
							},
							Ok(None) => {
								return Err(anyhow::anyhow!("Unexpected end of stdout stream"));
							},
							Err(e) => {
								return Err(anyhow::anyhow!("Error reading stdout: {}", e));
							}
						}
					},
					line = stderr_reader.next_line() => {
						match line {
							Ok(Some(line)) => {
								println!("STDERR: {}", line);
								if line.contains("Setup is complete") {
									println!("Testnet is up and running!");
									let _ = setup_complete_tx.send(());
																	return Ok(());
								}
							},
							Ok(None) => {
								return Err(anyhow::anyhow!("Unexpected end of stderr stream"));
							}
							Err(e) => {
								return Err(anyhow::anyhow!("Error reading stderr: {}", e));
							}
						}
					}
				}
			}
		});

		setup_complete_rx.await.expect("Failed to receive setup completion signal");
		println!("Setup complete message received.");

		let node_connection_url = "http://127.0.0.1:8080".to_string();
		let node_connection_url = Url::from_str(node_connection_url.as_str())
			.map_err(|_| BridgeContractCounterpartyError::SerializationError)?;
		let rest_client = Client::new(node_connection_url.clone());

		let faucet_url = "http://127.0.0.1:8081".to_string();
		let faucet_url = Url::from_str(faucet_url.as_str())
			.map_err(|_| BridgeContractCounterpartyError::SerializationError)?;
		let faucet_client = Arc::new(RwLock::new(FaucetClient::new(
			faucet_url.clone(),
			node_connection_url.clone(),
		)));

		let mut rng = ::rand::rngs::StdRng::from_seed([3u8; 32]);
		Ok((
			MovementClient {
				counterparty_address: DUMMY_ADDRESS,
				initiator_address: Vec::new(), // dummy for now
				rest_client,
				faucet_client: Some(faucet_client),
				signer: Arc::new(LocalAccount::generate(&mut rng)),
			},
			child,
		))
	}

	pub fn publish_for_test(&mut self) -> Result<()> {
		let random_seed = rand::thread_rng().gen_range(0, 1000000).to_string();

		let mut process = Command::new("movement")
			.args(&["init"])
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.spawn()
			.expect("Failed to execute command");

		let private_key_hex = hex::encode(self.signer.private_key().to_bytes());

		let stdin: &mut std::process::ChildStdin =
			process.stdin.as_mut().expect("Failed to open stdin");

		let movement_dir = PathBuf::from(".movement");

		if movement_dir.exists() {
			stdin.write_all(b"yes\n").expect("Failed to write to stdin");
		}

		stdin.write_all(b"local\n").expect("Failed to write to stdin");

		let _ = stdin.write_all(format!("{}\n", private_key_hex).as_bytes());

		drop(stdin);

		let addr_output = process.wait_with_output().expect("Failed to read command output");

		if !addr_output.stdout.is_empty() {
			println!("stdout: {}", String::from_utf8_lossy(&addr_output.stdout));
		}

		if !addr_output.stderr.is_empty() {
			eprintln!("stderr: {}", String::from_utf8_lossy(&addr_output.stderr));
		}
		let addr_output_str = String::from_utf8_lossy(&addr_output.stderr);
		let address = addr_output_str
			.split_whitespace()
			.find(|word| word.starts_with("0x"))
			.expect("Failed to extract the Movement account address");

		println!("Extracted address: {}", address);

		let resource_output = Command::new("movement")
			.args(&[
				"account",
				"derive-resource-account-address",
				"--address",
				address,
				"--seed",
				&random_seed,
			])
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.output()
			.expect("Failed to execute command");

		// Print the output of the resource address command for debugging
		if !resource_output.stdout.is_empty() {
			println!("stdout: {}", String::from_utf8_lossy(&resource_output.stdout));
		}
		if !resource_output.stderr.is_empty() {
			eprintln!("stderr: {}", String::from_utf8_lossy(&resource_output.stderr));
		}

		// Extract the resource address from the JSON output
		let resource_output_str = String::from_utf8_lossy(&resource_output.stdout);
		let resource_address = resource_output_str
			.lines()
			.find(|line| line.contains("\"Result\""))
			.and_then(|line| line.split('"').nth(3))
			.expect("Failed to extract the resource account address");

		// Ensure the address has a "0x" prefix
		let formatted_resource_address = if resource_address.starts_with("0x") {
			resource_address.to_string()
		} else {
			format!("0x{}", resource_address)
		};

		// Set counterparty module address to resource address, for function calls:
		self.counterparty_address = AccountAddress::from_hex_literal(&formatted_resource_address)?;

		println!("Derived resource address: {}", formatted_resource_address);

		let current_dir = env::current_dir().expect("Failed to get current directory");
		println!("Current directory: {:?}", current_dir);

		let move_toml_path = PathBuf::from("../move-modules/Move.toml");

		// Read the existing content of Move.toml
		let move_toml_content =
			fs::read_to_string(&move_toml_path).expect("Failed to read Move.toml file");

		// Update the content of Move.toml with the new addresses
		let updated_content = move_toml_content
			.lines()
			.map(|line| match line {
				_ if line.starts_with("resource_addr = ") => {
					format!(r#"resource_addr = "{}""#, formatted_resource_address)
				}
				_ if line.starts_with("atomic_bridge = ") => {
					format!(r#"atomic_bridge = "{}""#, formatted_resource_address)
				}
				_ if line.starts_with("moveth = ") => {
					format!(r#"moveth = "{}""#, formatted_resource_address)
				}
				_ if line.starts_with("master_minter = ") => {
					format!(r#"master_minter = "{}""#, formatted_resource_address)
				}
				_ if line.starts_with("minter = ") => {
					format!(r#"minter = "{}""#, formatted_resource_address)
				}
				_ if line.starts_with("admin = ") => {
					format!(r#"admin = "{}""#, formatted_resource_address)
				}
				_ if line.starts_with("origin_addr = ") => {
					format!(r#"origin_addr = "{}""#, address)
				}
				_ if line.starts_with("source_account = ") => {
					format!(r#"source_account = "{}""#, address)
				}
				_ => line.to_string(),
			})
			.collect::<Vec<_>>()
			.join("\n");

		// Write the updated content back to Move.toml
		let mut file =
			fs::File::create(&move_toml_path).expect("Failed to open Move.toml file for writing");
		file.write_all(updated_content.as_bytes())
			.expect("Failed to write updated Move.toml file");

		println!("Move.toml updated successfully.");

		let output2 = Command::new("movement")
			.args(&[
				"move",
				"create-resource-account-and-publish-package",
				"--assume-yes",
				"--address-name",
				"moveth",
				"--seed",
				&random_seed,
				"--package-dir",
				"../move-modules",
			])
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.output()
			.expect("Failed to execute command");

		if !output2.stdout.is_empty() {
			eprintln!("stdout: {}", String::from_utf8_lossy(&output2.stdout));
		}

		if !output2.stderr.is_empty() {
			eprintln!("stderr: {}", String::from_utf8_lossy(&output2.stderr));
		}

		if movement_dir.exists() {
			fs::remove_dir_all(movement_dir).expect("Failed to delete .movement directory");
			println!(".movement directory deleted successfully.");
		}

		// Read the existing content of Move.toml
		let move_toml_content =
			fs::read_to_string(&move_toml_path).expect("Failed to read Move.toml file");

		// Directly assign the address
		let final_address = "0xcafe";

		// Directly assign the formatted resource address
		let final_formatted_resource_address =
			"0xc3bb8488ab1a5815a9d543d7e41b0e0df46a7396f89b22821f07a4362f75ddc5";

		let updated_content = move_toml_content
			.lines()
			.map(|line| match line {
				_ if line.starts_with("resource_addr = ") => {
					format!(r#"resource_addr = "{}""#, final_formatted_resource_address)
				}
				_ if line.starts_with("atomic_bridge = ") => {
					format!(r#"atomic_bridge = "{}""#, final_formatted_resource_address)
				}
				_ if line.starts_with("moveth = ") => {
					format!(r#"moveth = "{}""#, final_formatted_resource_address)
				}
				_ if line.starts_with("master_minter = ") => {
					format!(r#"master_minter = "{}""#, final_formatted_resource_address)
				}
				_ if line.starts_with("minter = ") => {
					format!(r#"minter = "{}""#, final_formatted_resource_address)
				}
				_ if line.starts_with("admin = ") => {
					format!(r#"admin = "{}""#, final_formatted_resource_address)
				}
				_ if line.starts_with("origin_addr = ") => {
					format!(r#"origin_addr = "{}""#, final_address)
				}
				_ if line.starts_with("pauser = ") => {
					format!(r#"pauser = "{}""#, "0xdafe")
				}
				_ if line.starts_with("denylister = ") => {
					format!(r#"denylister = "{}""#, "0xcade")
				}
				_ => line.to_string(),
			})
			.collect::<Vec<_>>()
			.join("\n");

		// Write the updated content back to Move.toml
		let mut file =
			fs::File::create(&move_toml_path).expect("Failed to open Move.toml file for writing");
		file.write_all(updated_content.as_bytes())
			.expect("Failed to write updated Move.toml file");

		println!("Move.toml addresses updated successfully at the end of the test.");

		Ok(())
	}

	pub fn rest_client(&self) -> &Client {
		&self.rest_client
	}

	pub fn signer(&self) -> &LocalAccount {
		&self.signer
	}

	pub fn faucet_client(&self) -> Result<&Arc<RwLock<FaucetClient>>> {
		if let Some(faucet_client) = &self.faucet_client {
			Ok(faucet_client)
		} else {
			Err(anyhow::anyhow!("Faucet client not initialized"))
		}
	}
}

#[async_trait::async_trait]
impl BridgeContractCounterparty for MovementClient {
	type Address = MovementAddress;
	type Hash = [u8; 32];

	async fn lock_bridge_transfer(
		&mut self,
		bridge_transfer_id: BridgeTransferId<Self::Hash>,
		hash_lock: HashLock<Self::Hash>,
		initiator: InitiatorAddress<Vec<u8>>,
		recipient: RecipientAddress<Self::Address>,
		amount: Amount,
	) -> BridgeContractCounterpartyResult<()> {
		let amount_value = match amount.0 {
			AssetType::Moveth(value) => value,
			_ => return Err(BridgeContractCounterpartyError::SerializationError),
		};

		let args = vec![
			utils::serialize_vec(&initiator.0)?,
			utils::serialize_vec(&bridge_transfer_id.0[..])?,
			utils::serialize_vec(&hash_lock.0[..])?,
			utils::serialize_vec(&recipient.0 .0)?,
			utils::serialize_u64(&amount_value)?,
		];

		let payload = utils::make_aptos_payload(
			self.counterparty_address,
			COUNTERPARTY_MODULE_NAME,
			"lock_bridge_transfer",
			Vec::new(),
			args,
		);

		utils::send_and_confirm_aptos_transaction(&self.rest_client, self.signer.as_ref(), payload)
			.await
			.map_err(|_| BridgeContractCounterpartyError::LockTransferError)?;

		Ok(())
	}

	async fn complete_bridge_transfer(
		&mut self,
		bridge_transfer_id: BridgeTransferId<Self::Hash>,
		preimage: HashLockPreImage,
	) -> BridgeContractCounterpartyResult<()> {
		let args2 = vec![
			utils::serialize_vec(&bridge_transfer_id.0[..])?,
			utils::serialize_vec(&preimage.0)?,
		];

		let payload = utils::make_aptos_payload(
			self.counterparty_address,
			COUNTERPARTY_MODULE_NAME,
			"complete_bridge_transfer",
			Vec::new(),
			args2,
		);

		utils::send_and_confirm_aptos_transaction(&self.rest_client, self.signer.as_ref(), payload)
			.await
			.map_err(|_| BridgeContractCounterpartyError::CompleteTransferError)?;

		Ok(())
	}

	async fn abort_bridge_transfer(
		&mut self,
		bridge_transfer_id: BridgeTransferId<Self::Hash>,
	) -> BridgeContractCounterpartyResult<()> {
		let args3 = vec![utils::serialize_vec(&bridge_transfer_id.0[..])?];
		let payload = utils::make_aptos_payload(
			self.counterparty_address,
			COUNTERPARTY_MODULE_NAME,
			"abort_bridge_transfer",
			Vec::new(),
			args3,
		);

		utils::send_and_confirm_aptos_transaction(&self.rest_client, self.signer.as_ref(), payload)
			.await
			.map_err(|_| BridgeContractCounterpartyError::AbortTransferError)?;

		Ok(())
	}

	async fn get_bridge_transfer_details(
		&mut self,
		bridge_transfer_id: BridgeTransferId<[u8; 32]>,
	) -> Result<
		Option<BridgeTransferDetails<MovementAddress, [u8; 32]>>,
		BridgeContractCounterpartyError,
	> {
		// Convert the bridge_transfer_id to a hex string
		let bridge_transfer_id_hex = format!("0x{}", hex::encode(bridge_transfer_id.0));

		// Construct the ViewRequest
		let view_request = ViewRequest {
			function: EntryFunctionId {
				module: MoveModuleId {
					address: self.counterparty_address.clone().into(),
					name: aptos_api_types::IdentifierWrapper(
						Identifier::new("atomic_bridge_counterparty")
							.map_err(|_| BridgeContractCounterpartyError::FunctionViewError)?,
					),
				},
				name: aptos_api_types::IdentifierWrapper(
					Identifier::new("bridge_transfers")
						.map_err(|_| BridgeContractCounterpartyError::FunctionViewError)?,
				),
			},
			type_arguments: vec![],
			arguments: vec![serde_json::json!(bridge_transfer_id_hex)],
		};

		// Send the request to the "/view" endpoint using JSON
		let response: Response<Vec<serde_json::Value>> = self
			.rest_client
			.view(&view_request, None)
			.await
			.map_err(|_| BridgeContractCounterpartyError::CallError)?;

		// Extract and parse the response
		let values = response.inner();

		if values.len() != 6 {
			return Err(BridgeContractCounterpartyError::InvalidResponseLength);
		}

		let originator = utils::val_as_str(values.get(0))?;
		let recipient = utils::val_as_str(values.get(1))?;
		let amount = utils::val_as_str(values.get(2))?
			.parse::<u64>()
			.map_err(|_| BridgeContractCounterpartyError::SerializationError)?;
		let hash_lock = utils::val_as_str(values.get(3))?;
		let state = utils::val_as_u64(values.get(5))? as u8;

		// Convert the originator, recipient, and hash_lock
		let originator_address = AccountAddress::from_hex_literal(originator)
			.map_err(|_| BridgeContractCounterpartyError::SerializationError)?;
		let recipient_address_bytes = hex::decode(&recipient[2..])
			.map_err(|_| BridgeContractCounterpartyError::SerializationError)?;
		let hash_lock_array: [u8; 32] = hex::decode(&hash_lock[2..])
			.map_err(|_| BridgeContractCounterpartyError::SerializationError)?
			.try_into()
			.map_err(|_| BridgeContractCounterpartyError::SerializationError)?;

		// Create the BridgeTransferDetails struct
		let details: BridgeTransferDetails<MovementAddress, [u8; 32]> = BridgeTransferDetails {
			bridge_transfer_id,
			initiator_address: InitiatorAddress(MovementAddress(originator_address)),
			recipient_address: RecipientAddress(recipient_address_bytes),
			amount: Amount(AssetType::Moveth(amount)),
			hash_lock: HashLock(hash_lock_array),
			state,
		};
		Ok(Some(details))
	}
}

#[async_trait::async_trait]
impl BridgeContractInitiator for MovementClient {
	type Address = MovementAddress;
	type Hash = [u8; 32];

	async fn initiate_bridge_transfer(
		&mut self,
		_initiator: InitiatorAddress<MovementAddress>,
		recipient: RecipientAddress<Vec<u8>>,
		hash_lock: HashLock<Self::Hash>,
		amount: Amount,
	) -> BridgeContractInitiatorResult<()> {
		let amount_value = match amount.0 {
			AssetType::Moveth(value) => value,
			_ => return Err(BridgeContractInitiatorError::ConversionError),
		};
		debug!("Amount value: {:?}", amount_value);

		let args = vec![
			utils::serialize_vec_initiator(&recipient.0)?,
			utils::serialize_vec_initiator(&hash_lock.0[..])?,
			utils::serialize_u64_initiator(&amount_value)?,
		];

		let payload = utils::make_aptos_payload(
			self.counterparty_address,
			"atomic_bridge_initiator",
			"initiate_bridge_transfer",
			Vec::new(),
			args,
		);

		utils::send_and_confirm_aptos_transaction(&self.rest_client, self.signer.as_ref(), payload)
			.await
			.map_err(|_| BridgeContractInitiatorError::InitiateTransferError)?;

		Ok(())
	}

	async fn complete_bridge_transfer(
		&mut self,
		bridge_transfer_id: BridgeTransferId<<MovementClient as BridgeContractCounterparty>::Hash>,
		secret: HashLockPreImage,
	) -> BridgeContractInitiatorResult<()> {
		let args = vec![
			utils::serialize_vec_initiator(&bridge_transfer_id.0[..])?,
			utils::serialize_vec_initiator(&secret.0)?,
		];

		let payload = utils::make_aptos_payload(
			self.counterparty_address,
			"atomic_bridge_initiator",
			"complete_bridge_transfer",
			Vec::new(),
			args,
		);

		utils::send_and_confirm_aptos_transaction(&self.rest_client, self.signer.as_ref(), payload)
			.await
			.map_err(|_| BridgeContractInitiatorError::CompleteTransferError)?;

		Ok(())
	}

	async fn refund_bridge_transfer(
		&mut self,
		bridge_transfer_id: BridgeTransferId<<MovementClient as BridgeContractCounterparty>::Hash>,
	) -> BridgeContractInitiatorResult<()> {
		let args = vec![utils::serialize_vec_initiator(&bridge_transfer_id.0[..])?];

		let payload = utils::make_aptos_payload(
			self.counterparty_address,
			"atomic_bridge_initiator",
			"refund_bridge_transfer",
			Vec::new(),
			args,
		);

		utils::send_and_confirm_aptos_transaction(&self.rest_client, self.signer.as_ref(), payload)
			.await
			.map_err(|_| BridgeContractInitiatorError::ConversionError)?;

		Ok(())
	}

	async fn get_bridge_transfer_details(
		&mut self,
		bridge_transfer_id: BridgeTransferId<<MovementClient as BridgeContractCounterparty>::Hash>,
	) -> BridgeContractInitiatorResult<Option<BridgeTransferDetails<MovementAddress, [u8; 32]>>> {
		let bridge_transfer_id_hex = format!("0x{}", hex::encode(bridge_transfer_id.0));

		let view_request = ViewRequest {
			function: EntryFunctionId {
				module: MoveModuleId {
					address: self.counterparty_address.clone().into(),
					name: aptos_api_types::IdentifierWrapper(
						Identifier::new("atomic_bridge_initiator")
							.map_err(|_| BridgeContractInitiatorError::FunctionViewError)?,
					),
				},
				name: aptos_api_types::IdentifierWrapper(
					Identifier::new("bridge_transfers")
						.map_err(|_| BridgeContractInitiatorError::FunctionViewError)?,
				),
			},
			type_arguments: vec![],
			arguments: vec![serde_json::json!(bridge_transfer_id_hex)],
		};

		let response: Response<Vec<serde_json::Value>> = self
			.rest_client
			.view(&view_request, None)
			.await
			.map_err(|_| BridgeContractInitiatorError::CallError)?;

		let values = response.inner();

		if values.len() != 6 {
			return Err(BridgeContractInitiatorError::InvalidResponseLength);
		}

		let originator = utils::val_as_str_initiator(values.get(0))?;
		let recipient = utils::val_as_str_initiator(values.get(1))?;
		let amount = utils::val_as_str_initiator(values.get(2))?
			.parse::<u64>()
			.map_err(|_| BridgeContractInitiatorError::SerializationError)?;
		let hash_lock = utils::val_as_str_initiator(values.get(3))?;
		let state = utils::val_as_u64_initiator(values.get(5))? as u8;

		let originator_address = AccountAddress::from_hex_literal(originator)
			.map_err(|_| BridgeContractInitiatorError::SerializationError)?;
		let recipient_address_bytes = hex::decode(&recipient[2..])
			.map_err(|_| BridgeContractInitiatorError::SerializationError)?;
		let hash_lock_array: [u8; 32] = hex::decode(&hash_lock[2..])
			.map_err(|_| BridgeContractInitiatorError::SerializationError)?
			.try_into()
			.map_err(|_| BridgeContractInitiatorError::SerializationError)?;

		let details = BridgeTransferDetails {
			bridge_transfer_id,
			initiator_address: InitiatorAddress(MovementAddress(originator_address)),
			recipient_address: RecipientAddress(recipient_address_bytes),
			amount: Amount(AssetType::Moveth(amount)),
			hash_lock: HashLock(hash_lock_array),
			state,
		};

		Ok(Some(details))
	}
}

// Should feature `bridge-test` flag after https://github.com/movementlabsxyz/movement/pull/574 is merged
impl MovementClient {
	pub async fn counterparty_set_timelock(
		&mut self,
		time_lock: u64,
	) -> Result<(), BridgeContractCounterpartyError> {
		let args = vec![utils::serialize_u64(&time_lock)?];

		let payload = utils::make_aptos_payload(
			self.counterparty_address,
			"atomic_bridge_counterparty",
			"set_time_lock_duration",
			Vec::new(),
			args,
		);

		println!("payload {:?}", &payload);

		utils::send_and_confirm_aptos_transaction(&self.rest_client, self.signer.as_ref(), payload)
			.await
			.map_err(|_| BridgeContractCounterpartyError::CallError)?;

		Ok(())
	}

	pub async fn counterparty_time_lock_duration(
		&mut self,
	) -> Result<u64, BridgeContractCounterpartyError> {
		let view_request = ViewRequest {
			function: EntryFunctionId {
				module: MoveModuleId {
					address: self.counterparty_address.into(),
					name: aptos_api_types::IdentifierWrapper(
						Identifier::new("atomic_bridge_counterparty")
							.map_err(|_| BridgeContractCounterpartyError::FunctionViewError)?,
					),
				},
				name: aptos_api_types::IdentifierWrapper(
					Identifier::new("get_time_lock_duration")
						.map_err(|_| BridgeContractCounterpartyError::FunctionViewError)?,
				),
			},
			type_arguments: vec![],
			arguments: vec![],
		};

		let response: Response<Vec<serde_json::Value>> = self
			.rest_client
			.view(&view_request, None)
			.await
			.map_err(|_| BridgeContractCounterpartyError::CallError)?;

		let values = response.inner();
		let timelock = utils::val_as_u64(values.first())?;
		Ok(timelock)
	}

	pub async fn initiator_set_timelock(
		&mut self,
		time_lock: u64,
	) -> Result<(), BridgeContractInitiatorError> {
		let args = vec![utils::serialize_u64(&time_lock).expect("Failed to serialize time lock")];

		println!("counterparty address {:?}", &self.counterparty_address);

		let payload = utils::make_aptos_payload(
			self.counterparty_address,
			"atomic_bridge_initiator",
			"set_time_lock_duration",
			Vec::new(),
			args,
		);

		utils::send_and_confirm_aptos_transaction(&self.rest_client, self.signer.as_ref(), payload)
			.await
			.map_err(|_| BridgeContractInitiatorError::CallError)?;

		Ok(())
	}

	pub async fn initiator_time_lock_duration(
		&mut self,
	) -> Result<u64, BridgeContractInitiatorError> {
		let view_request = ViewRequest {
			function: EntryFunctionId {
				module: MoveModuleId {
					address: self.counterparty_address.into(),
					name: aptos_api_types::IdentifierWrapper(
						Identifier::new("atomic_bridge_initiator")
							.map_err(|_| BridgeContractInitiatorError::FunctionViewError)?,
					),
				},
				name: aptos_api_types::IdentifierWrapper(
					Identifier::new("get_time_lock_duration")
						.map_err(|_| BridgeContractInitiatorError::FunctionViewError)?,
				),
			},
			type_arguments: vec![],
			arguments: vec![],
		};

		let response: Response<Vec<serde_json::Value>> = self
			.rest_client
			.view(&view_request, None)
			.await
			.map_err(|_| BridgeContractInitiatorError::CallError)?;

		let values = response.inner();
		let timelock = utils::val_as_u64_initiator(values.first())?;
		Ok(timelock)
	}
}
