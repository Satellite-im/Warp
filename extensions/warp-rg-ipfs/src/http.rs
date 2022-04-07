use ipfs_api_backend_hyper::{IpfsApi, IpfsClient, TryFromUri};

pub struct IpfsMessaging {
    pub ipfs_client: IpfsClient<hyper_tls::HttpsConnector<hyper::client::HttpConnector>>,
}
