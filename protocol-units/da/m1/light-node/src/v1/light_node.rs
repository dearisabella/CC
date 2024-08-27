use m1_da_light_node_grpc::light_node_service_server::{LightNodeService, LightNodeServiceServer};
use m1_da_light_node_util::config::Config;
use tonic::transport::Server;
use tracing::info;

pub trait LightNodeV1Operations: LightNodeService + Send + Sync + Sized + Clone {
	/// Initializes from environment variables.
	async fn try_from_config(config: Config) -> Result<Self, anyhow::Error>;

	/// Runs the background tasks.
	async fn run_background_tasks(&self) -> Result<(), anyhow::Error>;

	/// Tries to get the service address
	fn try_service_address(&self) -> Result<String, anyhow::Error>;

	/// Runs the server
	async fn run_server(&self) -> Result<(), anyhow::Error> {
		let reflection = tonic_reflection::server::Builder::configure()
			.register_encoded_file_descriptor_set(m1_da_light_node_grpc::FILE_DESCRIPTOR_SET)
			.build()?;

		let address = self.try_service_address()?;
		info!("Server listening on: {}", address);
		Server::builder()
			.accept_http1(true)
			.add_service(LightNodeServiceServer::new(self.clone()))
			.add_service(reflection)
			.serve(address.parse()?)
			.await?;

		Ok(())
	}

	/// Runs the server and the background tasks.
	async fn run(self) -> Result<(), anyhow::Error> {
		let background_handle = self.run_background_tasks();

		let background_tasks = async move {
			background_handle.await?;
			Ok::<_, anyhow::Error>(())
		};
		let server = self.run_server();

		info!("Running server and background tasks.");
		tokio::try_join!(server, background_tasks)?;

		Ok(())
	}
}
