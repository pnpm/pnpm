use std::pin::Pin;
use std::task::{Context, Poll};

use anyhow::Context as _;
use http::Uri;
use libsql_replication::rpc::proxy::{
    proxy_client::ProxyClient, DescribeRequest, DescribeResult, ExecuteResults, ProgramReq,
};
use libsql_replication::rpc::replication::replication_log_client::ReplicationLogClient;
use tonic::{
    body::BoxBody,
    codegen::InterceptedService,
    metadata::{AsciiMetadataValue, BinaryMetadataValue},
    service::Interceptor,
};
use tonic_web::{GrpcWebCall, GrpcWebClientService};
use tower::{Service, ServiceBuilder};
use tower_http::{
    classify::{self, GrpcCode, GrpcErrorsAsFailures, SharedClassifier},
    trace::{self, TraceLayer},
};
use uuid::Uuid;

use crate::util::{ConnectorService, HttpRequestCallback};

use crate::util::box_clone_service::BoxCloneService;

type ResponseBody = trace::ResponseBody<
    GrpcWebCall<hyper::Body>,
    classify::GrpcEosErrorsAsFailures,
    trace::DefaultOnBodyChunk,
    trace::DefaultOnEos,
    trace::DefaultOnFailure,
>;

#[derive(Debug, Clone)]
pub struct Client {
    client_id: Uuid,
    pub(crate) replication: ReplicationLogClient<InterceptedService<GrpcChannel, GrpcInterceptor>>,
    proxy: ProxyClient<InterceptedService<GrpcChannel, GrpcInterceptor>>,
}

impl Client {
    pub fn new(
        connector: ConnectorService,
        origin: Uri,
        auth_token: impl AsRef<str>,
        version: Option<&str>,
        http_request_callback: Option<HttpRequestCallback>,
        maybe_namespace: Option<String>,
    ) -> anyhow::Result<Self> {
        let ver = version.unwrap_or(env!("CARGO_PKG_VERSION"));

        let version: AsciiMetadataValue = format!("libsql-rpc-{ver}")
            .try_into()
            .context("Invalid client version")?;

        let auth_token: AsciiMetadataValue = format!("Bearer {}", auth_token.as_ref())
            .try_into()
            .context("Invalid auth token must be ascii")?;

        let ns = if let Some(ns_from_arg) = maybe_namespace {
            ns_from_arg
        } else if let Ok(ns_from_host) = split_namespace(origin.host().unwrap()) {
            ns_from_host
        } else {
            "default".to_string()
        };
        
        let namespace = BinaryMetadataValue::from_bytes(ns.as_bytes());

        let channel = GrpcChannel::new(connector, http_request_callback);

        let interceptor = GrpcInterceptor {
            auth_token,
            namespace,
            version,
        };

        let replication = ReplicationLogClient::with_origin(
            InterceptedService::new(channel.clone(), interceptor.clone()),
            origin.clone(),
        );

        let proxy = ProxyClient::with_origin(InterceptedService::new(channel, interceptor), origin);

        // Remove default tonic `8mb` message limits since fly may buffer
        // messages causing the msg len to be longer.
        let replication = replication.max_decoding_message_size(usize::MAX);
        let proxy = proxy.max_decoding_message_size(usize::MAX);

        let client_id = Uuid::new_v4();

        Ok(Self {
            client_id,
            replication,
            proxy,
        })
    }

    pub fn new_client_id(&mut self) {
        self.client_id = Uuid::new_v4();
    }

    pub fn client_id(&self) -> String {
        self.client_id.to_string()
    }

    pub async fn execute_program(&self, program: ProgramReq) -> anyhow::Result<ExecuteResults> {
        // TODO(lucio): Map errors correctly
        self.proxy
            .clone()
            .execute(program)
            .await
            .map(|r| r.into_inner())
            .map_err(Into::into)
    }

    pub async fn describe(&self, describe_req: DescribeRequest) -> anyhow::Result<DescribeResult> {
        self.proxy
            .clone()
            .describe(describe_req)
            .await
            .map(|r| r.into_inner())
            .map_err(Into::into)
    }
}

#[derive(Debug, Clone)]
pub struct GrpcChannel {
    client: BoxCloneService<http::Request<BoxBody>, http::Response<ResponseBody>, hyper::Error>,
}

impl GrpcChannel {
    pub fn new(
        connector: ConnectorService,
        http_request_callback: Option<HttpRequestCallback>,
    ) -> Self {
        let client = hyper::Client::builder()
            .pool_idle_timeout(None)
            .pool_max_idle_per_host(3)
            .build(connector);
        let client = GrpcWebClientService::new(client);

        let classifier = GrpcErrorsAsFailures::new().with_success(GrpcCode::FailedPrecondition);

        let svc = ServiceBuilder::new()
            .layer(TraceLayer::new(SharedClassifier::new(classifier)))
            .map_request(move |request: http::Request<BoxBody>| {
                if let Some(cb) = &http_request_callback {
                    let (parts, body) = request.into_parts();
                    let mut req_copy = http::Request::from_parts(parts, ());
                    cb(&mut req_copy);

                    let (parts, _) = req_copy.into_parts();

                    http::Request::from_parts(parts, body)
                } else {
                    request
                }
            })
            .service(client);

        let client = BoxCloneService::new(svc);

        Self { client }
    }
}

impl Service<http::Request<BoxBody>> for GrpcChannel {
    type Response = http::Response<ResponseBody>;
    type Error = hyper::Error;
    type Future =
        Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<BoxBody>) -> Self::Future {
        let fut = self.client.call(req);
        Box::pin(fut)
    }
}

#[derive(Clone)]
/// Contains token and namespace headers to append to every request.
pub struct GrpcInterceptor {
    auth_token: AsciiMetadataValue,
    namespace: BinaryMetadataValue,
    version: AsciiMetadataValue,
}

impl Interceptor for GrpcInterceptor {
    fn call(&mut self, mut req: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
        req.metadata_mut()
            .insert("x-authorization", self.auth_token.clone());
        req.metadata_mut()
            .insert_bin("x-namespace-bin", self.namespace.clone());
        req.metadata_mut()
            .insert("x-libsql-client-version", self.version.clone());
        Ok(req)
    }
}

fn split_namespace(host: &str) -> anyhow::Result<String> {
    let (ns, _) = host
        .split_once('.')
        .ok_or_else(|| anyhow::anyhow!("host header should be in the format <namespace>.<...>"))?;

    if ns.is_empty() {
        anyhow::bail!("Invalid namespace as its empty");
    }

    let ns = ns.to_owned();
    Ok(ns)
}
