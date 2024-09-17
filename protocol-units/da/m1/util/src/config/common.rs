use alloy::signers::local::PrivateKeySigner;
use celestia_types::nmt::Namespace;
/*use ecdsa::{
	elliptic_curve::{
		generic_array::ArrayLength,
		ops::Invert,
		point::PointCompression,
		sec1::{FromEncodedPoint, ModulusSize, ToEncodedPoint},
		subtle::CtOption,
		AffinePoint, CurveArithmetic, FieldBytesSize, PrimeCurve, Scalar,
	},
	hazmat::{DigestPrimitive, SignPrimitive, VerifyPrimitive},
	SignatureSize,
};*/
use godfig::env_default;
use std::collections::HashSet;
use std::env;

// The default hostname for the Celestia RPC
env_default!(
	default_celestia_rpc_listen_hostname,
	"CELESTIA_RPC_LISTEN_HOSTNAME",
	String,
	"0.0.0.0".to_string()
);

// The default port for the Celestia RPC
env_default!(default_celestia_rpc_listen_port, "CELESTIA_RPC_LISTEN_PORT", u16, 26657);

// The default Celestia RPC connection hostname
env_default!(
	default_celestia_rpc_connection_hostname,
	"CELESTIA_RPC_CONNECTION_HOSTNAME",
	String,
	"0.0.0.0".to_string()
);

// The default Celestia RPC connection port
env_default!(default_celestia_rpc_connection_port, "CELESTIA_RPC_CONNECTION_PORT", u16, 26657);

// The default hostname for the Celestia Node websocket
env_default!(
	default_celestia_websocket_listen_hostname,
	"CELESTIA_WEBSOCKET_LISTEN_HOSTNAME",
	String,
	"0.0.0.0".to_string()
);

// The default port for the Celestia Node websocket
env_default!(default_celestia_websocket_listen_port, "CELESTIA_WEBSOCKET_LISTEN_PORT", u16, 26658);

// The default Celestia Node websocket connection hostname
env_default!(
	default_celestia_websocket_connection_hostname,
	"CELESTIA_WEBSOCKET_CONNECTION_HOSTNAME",
	String,
	"0.0.0.0".to_string()
);

// The default Celestia Node websocket connection port
env_default!(
	default_celestia_websocket_connection_port,
	"CELESTIA_WEBSOCKET_CONNECTION_PORT",
	u16,
	26658
);

// The default M1 DA Light Node listen hostname
env_default!(
	default_m1_da_light_node_listen_hostname,
	"M1_DA_LIGHT_NODE_LISTEN_HOSTNAME",
	String,
	"0.0.0.0".to_string()
);

// The default M1 DA Light Node listen port
env_default!(default_m1_da_light_node_listen_port, "M1_DA_LIGHT_NODE_LISTEN_PORT", u16, 30730);

// The default M1 DA Light Node connection hostname
env_default!(
	default_m1_da_light_node_connection_hostname,
	"M1_DA_LIGHT_NODE_CONNECTION_HOSTNAME",
	String,
	"0.0.0.0".to_string()
);

// The default M1 DA Light Node connection port
env_default!(
	default_m1_da_light_node_connection_port,
	"M1_DA_LIGHT_NODE_CONNECTION_PORT",
	u16,
	30730
);

// The default Celestia Namespace
pub fn default_celestia_namespace() -> Namespace {
	match std::env::var("CELESTIA_NAMESPACE") {
		Ok(val) => match serde_json::from_str(&val) {
			Ok(namespace) => namespace,
			// todo: get rid of this unwrap somehow, event though it should never fail
			Err(_) => Namespace::new_v0(b"movement").unwrap(),
		},
		// todo: get rid of this unwrap somehow, event though it should never fail
		Err(_) => Namespace::new_v0(b"movement").unwrap(),
	}
}

// The default Celestia chain id
env_default!(default_celestia_chain_id, "CELESTIA_CHAIN_ID", String, "movement".to_string());

// Whether to force a new chain
env_default!(default_celestia_force_new_chain, "CELESTIA_FORCE_NEW_CHAIN", bool, true);

// Whether to use replace args for Celestia appd
env_default!(default_celestia_appd_use_replace_args, "CELESTIA_USE_REPLACE_ARGS", bool, false);

// The replacement args for Celestia appd
pub fn default_celestia_appd_replace_args() -> Vec<String> {
	match std::env::var("CELESTIA_REPLACE_ARGS") {
		Ok(val) => val.split(',').map(|s| s.to_string()).collect(),
		Err(_) => vec![],
	}
}

// Whether to use replace args for Celestia bridge
env_default!(
	default_celestia_bridge_use_replace_args,
	"CELESTIA_BRIDGE_USE_REPLACE_ARGS",
	bool,
	false
);

// The replacement args for Celestia bridge
pub fn default_celestia_bridge_replace_args() -> Vec<String> {
	match std::env::var("CELESTIA_BRIDGE_REPLACE_ARGS") {
		Ok(val) => val.split(',').map(|s| s.to_string()).collect(),
		Err(_) => vec![],
	}
}

// Whether to use replace args for Celestia bridge
env_default!(default_m1_da_light_node_is_initial, "M1_DA_LIGHT_NODE_IS_INITIAL", bool, true);

/// The default da signing private key
pub fn default_da_signing_private_key() -> String {
	let random_wallet = PrivateKeySigner::random();
	let random_wallet_string = random_wallet.to_bytes().to_string();
	env::var("DA_SIGNING_PRIVATE_KEY").unwrap_or(random_wallet_string)
}

pub fn default_da_signers_sec1_keys() -> HashSet<Vec<u8>> {
	match std::env::var("DA_SIGNERS_SEC1_KEYS") {
		Ok(val) => val.split(',').map(|s| s.as_bytes().to_vec()).collect(),
		Err(_) => {
			// always trust yourself
			let mut set = HashSet::new();
			let signer = default_da_signing_private_key();
			set.insert(signer.as_bytes().to_vec());
			set
		}
	}
}
