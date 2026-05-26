//! tonic gRPC client for tapd (TaprootAssets + AssetWallet services).
//! Foundation for the SatUSD reference wallet SDK (PRD §12.5).
//!
//! tapd (like lnd) serves a self-signed TLS cert marked as a CA, which rustls
//! refuses as an end-entity cert. We connect with a custom rustls verifier that
//! pins the exact served cert (safe for a known local node), via a manual
//! connector — `ClientTlsConfig` offers no escape hatch for this.

use std::sync::Arc;

use hyper_util::rt::TokioIo;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::{ClientConfig, DigitallySignedStruct, Error as RustlsError, SignatureScheme};
use rustls_pki_types::{CertificateDer, ServerName, UnixTime};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tonic::metadata::MetadataValue;
use tonic::service::interceptor::InterceptedService;
use tonic::service::Interceptor;
use tonic::transport::{Channel, Uri};
use tonic::{Request, Status};

pub mod taprpc {
    tonic::include_proto!("taprpc");
}
pub mod assetwalletrpc {
    tonic::include_proto!("assetwalletrpc");
}

pub use assetwalletrpc::asset_wallet_client::AssetWalletClient;
pub use taprpc::taproot_assets_client::TaprootAssetsClient;

pub mod proof_assembly;

/// Injects the `macaroon: <hex>` metadata header tapd requires.
#[derive(Clone)]
pub struct MacaroonInterceptor {
    macaroon: MetadataValue<tonic::metadata::Ascii>,
}

impl Interceptor for MacaroonInterceptor {
    fn call(&mut self, mut req: Request<()>) -> Result<Request<()>, Status> {
        req.metadata_mut().insert("macaroon", self.macaroon.clone());
        Ok(req)
    }
}

pub type TapChannel = InterceptedService<Channel, MacaroonInterceptor>;

/// rustls verifier that accepts exactly one pinned end-entity certificate
/// (the cert tapd serves) and delegates handshake-signature checks to the
/// crypto provider. Ignores the CA basic-constraint issue by design.
#[derive(Debug)]
struct PinnedCertVerifier {
    pinned: CertificateDer<'static>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for PinnedCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        if end_entity.as_ref() == self.pinned.as_ref() {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(RustlsError::General(
                "tapd cert does not match pinned cert".into(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

fn pinned_tls_config(tls_cert_pem: &[u8]) -> Result<ClientConfig, Box<dyn std::error::Error>> {
    let mut reader = std::io::BufReader::new(tls_cert_pem);
    let pinned = rustls_pemfile::certs(&mut reader)
        .next()
        .ok_or("no certificate in PEM")??;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinnedCertVerifier { pinned, provider }))
        .with_no_client_auth();
    config.alpn_protocols = vec![b"h2".to_vec()];
    Ok(config)
}

/// Connect to tapd over its (CA-flagged, self-signed) TLS cert with macaroon auth.
/// `addr` e.g. "127.0.0.1:10029"; `domain` is the SNI name (e.g. "localhost").
pub async fn connect(
    addr: &str,
    tls_cert_pem: &[u8],
    macaroon_hex: &str,
    domain: &str,
) -> Result<TapChannel, Box<dyn std::error::Error>> {
    let connector = TlsConnector::from(Arc::new(pinned_tls_config(tls_cert_pem)?));
    let addr = addr.to_string();
    let server_name = ServerName::try_from(domain.to_string())?;

    let dial_addr = addr.clone();
    let svc = tower::service_fn(move |_: Uri| {
        let connector = connector.clone();
        let server_name = server_name.clone();
        let dial_addr = dial_addr.clone();
        async move {
            let tcp = TcpStream::connect(&dial_addr).await?;
            let tls = connector.connect(server_name, tcp).await?;
            Ok::<_, std::io::Error>(TokioIo::new(tls))
        }
    });

    // The custom connector performs TLS, so the channel URI uses http scheme
    // (https here would make tonic expect its own built-in TLS layer).
    let channel = Channel::from_shared(format!("http://{addr}"))?
        .connect_with_connector(svc)
        .await?;
    let macaroon: MetadataValue<_> = macaroon_hex.parse()?;
    Ok(InterceptedService::new(
        channel,
        MacaroonInterceptor { macaroon },
    ))
}
