use alloy::primitives::Uint;
use derive_more::{Deref, DerefMut};
use hex::{self, FromHexError};
use rand::{Rng, RngCore};
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;
use std::ops::AddAssign;
use std::{fmt::Debug, hash::Hash};
use thiserror::Error;

use crate::bridge_contracts::{BridgeContractCounterpartyError, BridgeContractInitiatorError};
use crate::bridge_monitoring::{BridgeContractCounterpartyEvent, BridgeContractInitiatorEvent};

pub type SCIResult<A, H> = Result<SmartContractInitiatorEvent<A, H>, SmartContractInitiatorError>;
pub type SCCResult<A, H> =
	Result<SmartContractCounterpartyEvent<A, H>, SmartContractCounterpartyError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmartContractInitiatorEvent<A, H> {
	InitiatedBridgeTransfer(BridgeTransferDetails<A, H>),
	CompletedBridgeTransfer(BridgeTransferId<H>),
	RefundedBridgeTransfer(BridgeTransferId<H>),
}

impl<A, H> From<BridgeContractInitiatorEvent<A, H>> for SmartContractInitiatorEvent<A, H> {
	fn from(event: BridgeContractInitiatorEvent<A, H>) -> Self {
		match event {
			BridgeContractInitiatorEvent::Initiated(details) => {
				SmartContractInitiatorEvent::InitiatedBridgeTransfer(details)
			}
			BridgeContractInitiatorEvent::Completed(id) => {
				SmartContractInitiatorEvent::CompletedBridgeTransfer(id)
			}
			BridgeContractInitiatorEvent::Refunded(id) => {
				SmartContractInitiatorEvent::RefundedBridgeTransfer(id)
			}
		}
	}
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SmartContractInitiatorError {
	#[error("Failed to initiate bridge transfer")]
	InitiateTransferError,
	#[error("Transfer not found")]
	TransferNotFound,
	#[error("Invalid hash lock pre image (secret)")]
	InvalidHashLockPreImage,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SmartContractCounterpartyError {
	#[error("Transfer not found")]
	TransferNotFound,
	#[error("Invalid hash lock pre image (secret)")]
	InvalidHashLockPreImage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmartContractCounterpartyEvent<A, H> {
	LockedBridgeTransfer(LockDetails<A, H>),
	CompletedBridgeTransfer(CounterpartyCompletedDetails<A, H>),
}

impl<A, H> From<BridgeContractCounterpartyEvent<A, H>> for SmartContractCounterpartyEvent<A, H> {
	fn from(event: BridgeContractCounterpartyEvent<A, H>) -> Self {
		match event {
			BridgeContractCounterpartyEvent::Locked(details) => {
				SmartContractCounterpartyEvent::LockedBridgeTransfer(details)
			}
			BridgeContractCounterpartyEvent::Completed(details) => {
				SmartContractCounterpartyEvent::CompletedBridgeTransfer(details)
			}
		}
	}
}

#[derive(Deref, Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BridgeTransferId<H>(pub H);

impl<H> BridgeTransferId<H> {
	pub fn inner(&self) -> &H {
		&self.0
	}
}

impl BridgeTransferId<[u8; 32]> {
	pub fn parse(s: &str) -> Result<Self, FromHexError> {
		let bytes = hex::decode(s)?;
		let array: [u8; 32] =
			bytes.as_slice().try_into().map_err(|_| FromHexError::InvalidStringLength)?;
		Ok(BridgeTransferId(array))
	}
}

impl<H, O> Convert<BridgeTransferId<O>> for BridgeTransferId<H>
where
	H: Convert<O>,
{
	fn convert(me: &BridgeTransferId<H>) -> BridgeTransferId<O> {
		BridgeTransferId(Convert::convert(&me.0))
	}
}

impl<H> From<H> for BridgeTransferId<H> {
	fn from(hash: H) -> Self {
		BridgeTransferId(hash)
	}
}

pub fn convert_bridge_transfer_id<H: From<O>, O>(
	other: BridgeTransferId<O>,
) -> BridgeTransferId<H> {
	BridgeTransferId(From::from(other.0))
}

impl<H> GenUniqueHash for BridgeTransferId<H>
where
	H: GenUniqueHash,
{
	fn gen_unique_hash<R: Rng>(rng: &mut R) -> Self {
		BridgeTransferId(H::gen_unique_hash(rng))
	}
}

#[derive(Deref, Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InitiatorAddress<A>(pub A);

impl From<&str> for InitiatorAddress<Vec<u8>> {
	fn from(value: &str) -> Self {
		Self(value.as_bytes().to_vec())
	}
}

impl From<String> for InitiatorAddress<Vec<u8>> {
	fn from(value: String) -> Self {
		Self(value.as_bytes().to_vec())
	}
}

#[derive(Deref, Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RecipientAddress<A>(pub A);

impl From<&str> for RecipientAddress<Vec<u8>> {
	fn from(value: &str) -> Self {
		Self(value.as_bytes().to_vec())
	}
}

impl From<String> for RecipientAddress<Vec<u8>> {
	fn from(value: String) -> Self {
		Self(value.as_bytes().to_vec())
	}
}

#[derive(Deref, Debug, Clone, PartialEq, Eq, Hash)]
pub struct RecipientAddressCounterparty<A>(pub A);

#[derive(Deref, Debug, Clone, PartialEq, Eq, Hash)]
pub struct InitiatorAddressCounterParty(pub Vec<u8>);

#[derive(Deref, Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct HashLock<H>(pub H);

impl<H> HashLock<H> {
	pub fn inner(&self) -> &H {
		&self.0
	}
}

impl HashLock<[u8; 32]> {
	pub fn parse(s: &str) -> Result<Self, FromHexError> {
		let bytes = hex::decode(s)?;
		let array: [u8; 32] =
			bytes.as_slice().try_into().map_err(|_| FromHexError::InvalidStringLength)?;
		Ok(HashLock(array))
	}
}

pub fn convert_hash_lock<H: From<O>, O>(other: HashLock<O>) -> HashLock<H> {
	HashLock(From::from(other.0))
}

#[derive(Deref, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HashLockPreImage(pub Vec<u8>);

impl AsRef<[u8]> for HashLockPreImage {
	fn as_ref(&self) -> &[u8] {
		&self.0
	}
}

impl HashLockPreImage {
	/// Generate a cryptographically secure random secret
	pub fn random() -> Self {
		let mut rng = rand::thread_rng();
		let mut secret = vec![0u8; 32];
		rng.fill_bytes(&mut secret);
		HashLockPreImage(secret)
	}
}

#[derive(Deref, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeLock(pub u64);

impl From<Uint<256, 4>> for TimeLock {
	fn from(value: Uint<256, 4>) -> Self {
		// Extract the lower 64 bits.
		let lower_64_bits = value.as_limbs()[0];
		TimeLock(lower_64_bits)
	}
}

#[derive(Deref, DerefMut, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Amount(pub AssetType);

/// The type of Asset being used
#[derive(Clone, Debug, PartialEq, Eq, Copy, Serialize, Deserialize)]
pub enum AssetType {
	/// Where the first tuple value is `Eth` and the second tuple value is `Weth`  
	EthAndWeth((u64, u64)),
	Moveth(u64),
}

impl From<Uint<256, 4>> for AssetType {
	fn from(value: Uint<256, 4>) -> Self {
		// Extract the lower 64 bits.
		let lower_64_bits = value.as_limbs()[0];
		AssetType::Moveth(lower_64_bits)
	}
}

#[derive(Error, Debug)]
pub enum ConversionError {
	#[error("Invalid conversion from AssetType to Uint")]
	InvalidConversion,
}

impl TryFrom<AssetType> for Uint<256, 4> {
	type Error = ConversionError;

	fn try_from(value: AssetType) -> Result<Self, Self::Error> {
		match value {
			AssetType::EthAndWeth((eth_value, weth_value)) => {
				// Example logic: combine the values or whatever makes sense in your context
				let combined_value = eth_value as u128 + weth_value as u128;
				Ok(Uint::from(combined_value))
			}
			AssetType::Moveth(value) => Ok(Uint::from(value as u128)),
		}
	}
}

impl AddAssign for AssetType {
	fn add_assign(&mut self, other: Self) {
		match (self, other) {
			(AssetType::Moveth(ref mut a), AssetType::Moveth(b)) => *a += b,
			(AssetType::EthAndWeth((ref mut a, ref mut b)), AssetType::EthAndWeth((c, d))) => {
				*a += c;
				*b += d;
			}
			_ => (),
		}
	}
}

impl Amount {
	pub fn weth(&self) -> u64 {
		match self.0 {
			AssetType::EthAndWeth((_, weth_value)) => weth_value,
			_ => 0,
		}
	}
	pub fn eth(&self) -> u64 {
		match self.0 {
			AssetType::EthAndWeth((eth_value, _)) => eth_value,
			_ => 0,
		}
	}
	pub fn moveth(&self) -> u64 {
		match self.0 {
			AssetType::Moveth(value) => value,
			_ => 0,
		}
	}
	pub fn value(&self) -> u64 {
		match self.0 {
			AssetType::EthAndWeth((weth_value, eth_value)) => weth_value + eth_value,
			AssetType::Moveth(value) => value,
		}
	}
}

impl From<Uint<256, 4>> for Amount {
	fn from(value: Uint<256, 4>) -> Self {
		// Extract the lower 64 bits.
		let lower_64_bits = value.as_limbs()[0];
		Amount(AssetType::EthAndWeth((0, lower_64_bits)))
	}
}

//#[derive(Debug, PartialEq, Eq, Clone)]
//enum State {
//        INITIALIZED,
//        COMPLETED,
//        REFUNDED
//}

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct BridgeTransferDetails<A, H> {
	pub bridge_transfer_id: BridgeTransferId<H>,
	pub initiator_address: InitiatorAddress<A>,
	pub recipient_address: RecipientAddress<Vec<u8>>,
	pub hash_lock: HashLock<H>,
	pub amount: Amount,
	pub state: u8,
}

impl<A, H> Default for BridgeTransferDetails<A, H> {
	fn default() -> Self {
		todo!()
	}
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct LockDetails<A, H> {
	pub bridge_transfer_id: BridgeTransferId<H>,
	pub initiator_address: InitiatorAddress<Vec<u8>>,
	pub recipient_address: RecipientAddress<A>,
	pub hash_lock: HashLock<H>,
	pub amount: Amount,
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct CounterpartyCompletedDetails<A, H> {
	pub bridge_transfer_id: BridgeTransferId<H>,
	pub initiator_address: InitiatorAddress<Vec<u8>>,
	pub recipient_address: RecipientAddress<A>,
	pub hash_lock: HashLock<H>,
	pub secret: HashLockPreImage,
	pub amount: Amount,
}

impl<A, H> CounterpartyCompletedDetails<A, H>
where
	InitiatorAddress<Vec<u8>>: From<InitiatorAddress<A>>,
	RecipientAddress<A>: From<RecipientAddress<Vec<u8>>>,
{
	pub fn from_bridge_transfer_details(
		bridge_transfer_details: BridgeTransferDetails<A, H>,
		secret: HashLockPreImage,
	) -> Self {
		CounterpartyCompletedDetails {
			bridge_transfer_id: bridge_transfer_details.bridge_transfer_id,
			initiator_address: From::from(bridge_transfer_details.initiator_address),
			recipient_address: From::from(bridge_transfer_details.recipient_address),
			hash_lock: bridge_transfer_details.hash_lock,
			secret,
			amount: bridge_transfer_details.amount,
		}
	}
}

impl<A, H> CounterpartyCompletedDetails<A, H> {
	pub fn from_lock_details(lock_details: LockDetails<A, H>, secret: HashLockPreImage) -> Self {
		CounterpartyCompletedDetails {
			bridge_transfer_id: lock_details.bridge_transfer_id,
			initiator_address: lock_details.initiator_address,
			recipient_address: lock_details.recipient_address,
			hash_lock: lock_details.hash_lock,
			secret,
			amount: lock_details.amount,
		}
	}
}

#[derive(Debug, Hash, Eq, PartialEq, Clone)]
pub enum MethodName {
	InitiateBridgeTransfer,
	CompleteBridgeTransferInitiator,
	CompleteBridgeTransferCounterparty,
	RefundBridgeTransfer,
	GetBridgeTransferDetails,
	LockBridgeTransferAssets,
	AbortBridgeTransfer,
}

#[derive(Debug, Error, Clone)]
pub enum AbstractBlockchainClientError {
	#[error("Failed to send transaction")]
	SendError,
	#[error("Random failure occurred")]
	RandomFailure,
}

#[derive(Debug)]
pub enum CounterpartyCall<A, H> {
	CompleteBridgeTransfer(BridgeTransferId<H>, HashLockPreImage),
	LockBridgeTransfer(
		BridgeTransferId<H>,
		HashLock<H>,
		TimeLock,
		InitiatorAddress<Vec<u8>>,
		RecipientAddress<A>,
		Amount,
	),
}

#[derive(Debug)]
pub enum InitiatorCall<A, H> {
	InitiateBridgeTransfer(
		InitiatorAddress<A>,
		RecipientAddress<Vec<u8>>,
		Amount,
		TimeLock,
		HashLock<H>,
	),
	CompleteBridgeTransfer(BridgeTransferId<H>, HashLockPreImage),
}

#[derive(Clone, Debug)]
pub enum ErrorConfig {
	None,
	InitiatorError(BridgeContractInitiatorError),
	CounterpartyError(BridgeContractCounterpartyError),
	CustomError(AbstractBlockchainClientError),
}

#[derive(Debug, Clone)]
pub struct CallConfig {
	pub error: ErrorConfig,
	pub delay: Option<std::time::Duration>,
}

// Types
pub trait BridgeHashType: Debug + PartialEq + Eq + Hash + Unpin + Send + Sync + Clone {}
pub trait BridgeAddressType:
	Debug + PartialEq + Eq + Hash + Unpin + Send + Sync + Clone + From<Vec<u8>>
{
}
pub trait BridgeValueType: Debug + PartialEq + Eq + Clone + Send + Sync + Unpin {}

pub trait Convert<O> {
	fn convert(other: &Self) -> O;
}

// Blankets
impl<T> BridgeHashType for T where T: Debug + PartialEq + Eq + Hash + Unpin + Send + Sync + Clone {}
impl<T> BridgeAddressType for T where
	T: Debug + PartialEq + Eq + Hash + Unpin + Send + Sync + Clone + From<Vec<u8>>
{
}

pub trait GenUniqueHash {
	fn gen_unique_hash<R: Rng>(rng: &mut R) -> Self;
}

