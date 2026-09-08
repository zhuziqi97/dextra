//! Cerebro 设备配对、长期凭据与短期 access token。
//!
//! refresh credential 只进入本机 credential store；SQLite、前端状态和日志
//! 都不持有原文。配对 exchange 与本机存储无法形成跨进程原子事务，因此若
//! store 写入失败，会把已经换出的凭据暂留在当前进程并允许下一次 poll 重试。

use std::sync::{LazyLock, RwLock};
use std::time::{Duration, Instant};

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::sync::{Mutex, Notify};

use crate::app_error::{AppCommandError, AppErrorCode};

const PAIRING_CREATE_PATH: &str = "api/v1/execution-runner-pairings/create";
const PAIRING_EXCHANGE_PATH: &str = "api/v1/execution-runner-pairings/exchange";
const TOKEN_REFRESH_PATH: &str = "api/v1/execution-runner-tokens/refresh";
const RUNNER_CREDENTIAL_INVALID: &str = "RUNNER_CREDENTIAL_INVALID";
const RUNNER_REVOKED: &str = "RUNNER_REVOKED";
const RUNNER_PAIRING_PENDING: &str = "RUNNER_PAIRING_PENDING";
const RUNNER_PAIRING_INVALID: &str = "RUNNER_PAIRING_INVALID";
const RUNNER_PAIRING_EXPIRED: &str = "RUNNER_PAIRING_EXPIRED";
const RUNNER_PAIRING_CONSUMED: &str = "RUNNER_PAIRING_CONSUMED";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PairingPollStatus {
    Pending,
    Paired,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CerebroPairingStart {
    pub handle: String,
    pub cerebro_base_url: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CerebroPairingPoll {
    pub status: PairingPollStatus,
    pub runner_id: Option<String>,
    pub retry_after: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CerebroAuthState {
    pub paired: bool,
    pub cerebro_base_url: Option<String>,
    pub runner_id: Option<String>,
    pub pairing: Option<CerebroPairingStart>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CerebroRunnerAccess {
    pub cerebro_base_url: String,
    pub runner_id: String,
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CerebroMcpPrincipal {
    #[serde(alias = "mcp_url")]
    pub mcp_url: String,
    #[serde(alias = "access_token")]
    pub access_token: String,
    #[serde(alias = "token_type")]
    pub token_type: String,
    #[serde(alias = "expires_in")]
    pub expires_in: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct RunnerCredential {
    cerebro_base_url: String,
    runner_id: String,
    refresh_credential: String,
}

#[derive(Debug, Deserialize)]
struct PairingCreateData {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
}

#[derive(Debug, Serialize)]
struct PairingCreateRequest {}

#[derive(Debug, Serialize)]
struct PairingExchangeRequest<'a> {
    device_code: &'a str,
}

#[derive(Debug, Serialize)]
struct TokenRefreshRequest<'a> {
    refresh_credential: &'a str,
}

#[derive(Debug, Deserialize)]
struct PairingExchangeData {
    runner_id: String,
    refresh_credential: String,
}

#[derive(Debug, Deserialize)]
struct TokenRefreshData {
    access_token: String,
    token_type: String,
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct CerebroApiError {
    code: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct CerebroEnvelope<T> {
    success: bool,
    data: Option<T>,
    error: Option<CerebroApiError>,
}

#[derive(Debug, thiserror::Error)]
enum ClientError {
    #[error("{0}")]
    Network(String),
    #[error("{message}")]
    Api { code: String, message: String },
    #[error("{0}")]
    InvalidResponse(String),
}

impl ClientError {
    fn api_code(&self) -> Option<&str> {
        match self {
            Self::Api { code, .. } => Some(code),
            _ => None,
        }
    }

    fn into_app_error(self) -> AppCommandError {
        match self {
            Self::Network(message) => AppCommandError::network(message),
            Self::InvalidResponse(message) => {
                AppCommandError::new(AppErrorCode::NetworkError, message)
            }
            Self::Api { code, message }
                if matches!(code.as_str(), RUNNER_CREDENTIAL_INVALID | RUNNER_REVOKED) =>
            {
                AppCommandError::authentication_failed(message).with_detail(code)
            }
            Self::Api { code, message } => {
                AppCommandError::invalid_input(message).with_detail(code)
            }
        }
    }
}

pub(super) trait CredentialStore: Send + Sync {
    fn load(&self) -> Result<Option<RunnerCredential>, AppCommandError>;
    fn save(&self, credential: &RunnerCredential) -> Result<(), AppCommandError>;
    fn clear(&self) -> Result<(), AppCommandError>;
}

pub(super) struct SystemCredentialStore;

impl CredentialStore for SystemCredentialStore {
    fn load(&self) -> Result<Option<RunnerCredential>, AppCommandError> {
        let raw = crate::keyring_store::get_cerebro_runner_credential().map_err(|error| {
            AppCommandError::io_error("Failed to read the Cerebro Runner credential")
                .with_detail(error)
        })?;
        raw.map(|value| {
            serde_json::from_str(&value).map_err(|error| {
                AppCommandError::configuration_invalid(
                    "The stored Cerebro Runner credential is invalid",
                )
                .with_detail(error.to_string())
            })
        })
        .transpose()
    }

    fn save(&self, credential: &RunnerCredential) -> Result<(), AppCommandError> {
        let serialized = serde_json::to_string(credential).map_err(|error| {
            AppCommandError::configuration_invalid("Failed to encode the Cerebro Runner credential")
                .with_detail(error.to_string())
        })?;
        crate::keyring_store::set_cerebro_runner_credential(&serialized).map_err(|error| {
            AppCommandError::io_error("Failed to save the Cerebro Runner credential")
                .with_detail(error)
        })
    }

    fn clear(&self) -> Result<(), AppCommandError> {
        crate::keyring_store::delete_cerebro_runner_credential().map_err(|error| {
            AppCommandError::io_error("Failed to remove the Cerebro Runner credential")
                .with_detail(error)
        })
    }
}

#[derive(Debug, Clone)]
enum PendingState {
    AwaitingApproval { device_code: String },
    CredentialIssued { credential: RunnerCredential },
}

#[derive(Debug, Clone)]
struct PendingPairing {
    handle: String,
    cerebro_base_url: String,
    user_code: String,
    verification_uri: String,
    expires_at: Instant,
    interval: u64,
    state: PendingState,
}

impl PendingPairing {
    fn public(&self) -> CerebroPairingStart {
        CerebroPairingStart {
            handle: self.handle.clone(),
            cerebro_base_url: self.cerebro_base_url.clone(),
            user_code: self.user_code.clone(),
            verification_uri: self.verification_uri.clone(),
            expires_in: self
                .expires_at
                .saturating_duration_since(Instant::now())
                .as_secs()
                .max(1),
            interval: self.interval,
        }
    }

    fn expired(&self) -> bool {
        matches!(self.state, PendingState::AwaitingApproval { .. })
            && Instant::now() >= self.expires_at
    }
}

#[derive(Default)]
struct PairingManager {
    pending: Mutex<Option<PendingPairing>>,
}

static PAIRING_MANAGER: LazyLock<PairingManager> = LazyLock::new(PairingManager::default);
static RUNNER_IDENTITY_CHANGED: LazyLock<Notify> = LazyLock::new(Notify::new);

type ProxyKeyedClient = (Vec<(String, String)>, reqwest::Client);
static HTTP_CLIENT: RwLock<Option<ProxyKeyedClient>> = RwLock::new(None);

fn http_client() -> Result<reqwest::Client, AppCommandError> {
    let proxy_values = crate::network::proxy::current_proxy_env_vars();
    if let Ok(guard) = HTTP_CLIENT.read() {
        if let Some((cached, client)) = guard.as_ref() {
            if *cached == proxy_values {
                return Ok(client.clone());
            }
        }
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| {
            AppCommandError::network(format!("Failed to create Cerebro HTTP client: {error}"))
        })?;
    if let Ok(mut guard) = HTTP_CLIENT.write() {
        *guard = Some((proxy_values, client.clone()));
    }
    Ok(client)
}

fn normalize_base_url(raw: &str) -> Result<String, AppCommandError> {
    let mut parsed = reqwest::Url::parse(raw.trim()).map_err(|error| {
        AppCommandError::configuration_invalid("Invalid Cerebro URL").with_detail(error.to_string())
    })?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none_or(str::is_empty) {
        return Err(AppCommandError::configuration_invalid(
            "Cerebro URL must be an executable HTTP or HTTPS address",
        ));
    }
    let path = parsed.path().trim_end_matches('/').to_string();
    parsed.set_path(&format!("{path}/"));
    Ok(parsed.to_string())
}

fn endpoint(base_url: &str, relative_path: &str) -> Result<reqwest::Url, AppCommandError> {
    let mut url = reqwest::Url::parse(base_url).map_err(|error| {
        AppCommandError::configuration_invalid("Invalid stored Cerebro URL")
            .with_detail(error.to_string())
    })?;
    let base_path = url.path().trim_end_matches('/');
    url.set_path(&format!("{base_path}/{relative_path}"));
    url.set_fragment(None);
    Ok(url)
}

async fn post_json<T: DeserializeOwned, B: Serialize + ?Sized>(
    client: &reqwest::Client,
    url: reqwest::Url,
    body: &B,
    bearer_token: Option<&str>,
) -> Result<T, ClientError> {
    let mut request = client.post(url).json(&body);
    if let Some(token) = bearer_token {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .await
        .map_err(|error| ClientError::Network(error.to_string()))?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|error| ClientError::Network(error.to_string()))?;
    let envelope: CerebroEnvelope<T> = serde_json::from_slice(&bytes).map_err(|error| {
        ClientError::InvalidResponse(format!(
            "Cerebro returned an invalid response ({status}): {error}"
        ))
    })?;
    if envelope.success {
        if !status.is_success() {
            return Err(ClientError::InvalidResponse(format!(
                "Cerebro returned success with HTTP status {status}"
            )));
        }
        return envelope.data.ok_or_else(|| {
            ClientError::InvalidResponse("Cerebro success response has no data".to_string())
        });
    }
    let error = envelope.error.ok_or_else(|| {
        ClientError::InvalidResponse(format!(
            "Cerebro error response ({status}) has no error detail"
        ))
    })?;
    Err(ClientError::Api {
        code: error.code,
        message: error.message,
    })
}

fn validate_verification_uri(raw: &str) -> Result<(), AppCommandError> {
    let parsed = reqwest::Url::parse(raw).map_err(|error| {
        AppCommandError::new(
            AppErrorCode::NetworkError,
            "Cerebro returned an invalid verification URL",
        )
        .with_detail(error.to_string())
    })?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(AppCommandError::new(
            AppErrorCode::NetworkError,
            "Cerebro returned a non-executable verification URL",
        ));
    }
    Ok(())
}

async fn start_pairing_with(
    manager: &PairingManager,
    store: &dyn CredentialStore,
    client: &reqwest::Client,
    cerebro_base_url: &str,
) -> Result<CerebroPairingStart, AppCommandError> {
    if store.load()?.is_some() {
        return Err(AppCommandError::already_exists(
            "This Dextra is already paired; disconnect it before pairing another identity",
        ));
    }
    let base_url = normalize_base_url(cerebro_base_url)?;

    let mut slot = manager.pending.lock().await;
    if slot.as_ref().is_some_and(PendingPairing::expired) {
        *slot = None;
    }
    if let Some(existing) = slot.as_ref() {
        if existing.cerebro_base_url == base_url {
            return Ok(existing.public());
        }
        return Err(AppCommandError::already_exists(
            "Another Cerebro pairing is already in progress",
        ));
    }

    let created: PairingCreateData = post_json(
        client,
        endpoint(&base_url, PAIRING_CREATE_PATH)?,
        &PairingCreateRequest {},
        None,
    )
    .await
    .map_err(ClientError::into_app_error)?;
    if created.device_code.is_empty()
        || created.user_code.trim().is_empty()
        || created.expires_in == 0
        || created.interval == 0
    {
        return Err(AppCommandError::new(
            AppErrorCode::NetworkError,
            "Cerebro returned an incomplete pairing response",
        ));
    }
    validate_verification_uri(&created.verification_uri)?;
    let expires_at = Instant::now()
        .checked_add(Duration::from_secs(created.expires_in))
        .ok_or_else(|| {
            AppCommandError::new(
                AppErrorCode::NetworkError,
                "Cerebro returned an invalid pairing lifetime",
            )
        })?;
    let pending = PendingPairing {
        handle: uuid::Uuid::new_v4().to_string(),
        cerebro_base_url: base_url,
        user_code: created.user_code,
        verification_uri: created.verification_uri,
        expires_at,
        interval: created.interval,
        state: PendingState::AwaitingApproval {
            device_code: created.device_code,
        },
    };
    let public = pending.public();
    *slot = Some(pending);
    Ok(public)
}

async fn poll_pairing_with(
    manager: &PairingManager,
    store: &dyn CredentialStore,
    client: &reqwest::Client,
    handle: &str,
) -> Result<CerebroPairingPoll, AppCommandError> {
    let mut slot = manager.pending.lock().await;
    let pending = slot
        .as_mut()
        .filter(|pending| pending.handle == handle)
        .ok_or_else(|| AppCommandError::not_found("Cerebro pairing is not active"))?;

    if pending.expired() {
        *slot = None;
        return Err(
            AppCommandError::authentication_failed("The Cerebro pairing code has expired")
                .with_detail(RUNNER_PAIRING_EXPIRED),
        );
    }

    let credential = match pending.state.clone() {
        PendingState::CredentialIssued { credential } => credential,
        PendingState::AwaitingApproval { device_code } => {
            let exchanged: PairingExchangeData = match post_json(
                client,
                endpoint(&pending.cerebro_base_url, PAIRING_EXCHANGE_PATH)?,
                &PairingExchangeRequest {
                    device_code: &device_code,
                },
                None,
            )
            .await
            {
                Ok(exchanged) => exchanged,
                Err(error) if error.api_code() == Some(RUNNER_PAIRING_PENDING) => {
                    return Ok(CerebroPairingPoll {
                        status: PairingPollStatus::Pending,
                        runner_id: None,
                        retry_after: Some(pending.interval),
                    });
                }
                Err(error)
                    if matches!(
                        error.api_code(),
                        Some(
                            RUNNER_PAIRING_INVALID
                                | RUNNER_PAIRING_EXPIRED
                                | RUNNER_PAIRING_CONSUMED
                        )
                    ) =>
                {
                    *slot = None;
                    return Err(error.into_app_error());
                }
                Err(error) => return Err(error.into_app_error()),
            };
            if exchanged.runner_id.trim().is_empty()
                || exchanged.refresh_credential.trim().is_empty()
            {
                return Err(AppCommandError::new(
                    AppErrorCode::NetworkError,
                    "Cerebro returned an incomplete Runner credential",
                ));
            }
            let credential = RunnerCredential {
                cerebro_base_url: pending.cerebro_base_url.clone(),
                runner_id: exchanged.runner_id,
                refresh_credential: exchanged.refresh_credential,
            };
            pending.state = PendingState::CredentialIssued {
                credential: credential.clone(),
            };
            credential
        }
    };

    store.save(&credential)?;
    RUNNER_IDENTITY_CHANGED.notify_waiters();
    let runner_id = credential.runner_id;
    *slot = None;
    Ok(CerebroPairingPoll {
        status: PairingPollStatus::Paired,
        runner_id: Some(runner_id),
        retry_after: None,
    })
}

async fn auth_state_with(
    manager: &PairingManager,
    store: &dyn CredentialStore,
) -> Result<CerebroAuthState, AppCommandError> {
    if let Some(credential) = store.load()? {
        return Ok(CerebroAuthState {
            paired: true,
            cerebro_base_url: Some(credential.cerebro_base_url),
            runner_id: Some(credential.runner_id),
            pairing: None,
        });
    }
    let mut slot = manager.pending.lock().await;
    if slot.as_ref().is_some_and(PendingPairing::expired) {
        *slot = None;
    }
    Ok(CerebroAuthState {
        paired: false,
        cerebro_base_url: slot
            .as_ref()
            .map(|pending| pending.cerebro_base_url.clone()),
        runner_id: None,
        pairing: slot.as_ref().map(PendingPairing::public),
    })
}

async fn cancel_pairing_with(
    manager: &PairingManager,
    handle: &str,
) -> Result<CerebroAuthState, AppCommandError> {
    let mut slot = manager.pending.lock().await;
    match slot.as_ref() {
        Some(pending) if pending.handle == handle => *slot = None,
        Some(_) => return Err(AppCommandError::not_found("Cerebro pairing is not active")),
        None => {}
    }
    Ok(CerebroAuthState {
        paired: false,
        cerebro_base_url: None,
        runner_id: None,
        pairing: None,
    })
}

async fn refresh_access_token_with(
    store: &dyn CredentialStore,
    client: &reqwest::Client,
) -> Result<CerebroRunnerAccess, AppCommandError> {
    let credential = store.load()?.ok_or_else(|| {
        AppCommandError::configuration_missing("This Dextra is not paired with Cerebro")
    })?;
    match refresh_credential(client, credential).await {
        Err(error) if matches!(error.detail.as_deref(), Some(RUNNER_CREDENTIAL_INVALID | RUNNER_REVOKED)) => {
            store.clear()?;
            Err(error)
        }
        result => result,
    }
}

async fn refresh_credential(
    client: &reqwest::Client,
    credential: RunnerCredential,
) -> Result<CerebroRunnerAccess, AppCommandError> {
    let refreshed: TokenRefreshData = post_json(
        client,
        endpoint(&credential.cerebro_base_url, TOKEN_REFRESH_PATH)?,
        &TokenRefreshRequest {
            refresh_credential: &credential.refresh_credential,
        },
        None,
    )
    .await.map_err(ClientError::into_app_error)?;
    if refreshed.access_token.trim().is_empty()
        || refreshed.token_type != "bearer"
        || refreshed.expires_in == 0
    {
        return Err(AppCommandError::new(
            AppErrorCode::NetworkError,
            "Cerebro returned an invalid Runner access token",
        ));
    }
    Ok(CerebroRunnerAccess {
        cerebro_base_url: credential.cerebro_base_url,
        runner_id: credential.runner_id,
        access_token: refreshed.access_token,
        token_type: refreshed.token_type,
        expires_in: refreshed.expires_in,
    })
}

pub async fn start_pairing(cerebro_base_url: &str) -> Result<CerebroPairingStart, AppCommandError> {
    let client = http_client()?;
    start_pairing_with(
        &PAIRING_MANAGER,
        &super::credential_storage::SelectedCredentialStore::current()?,
        &client,
        cerebro_base_url,
    )
    .await
}

pub async fn poll_pairing(handle: &str) -> Result<CerebroPairingPoll, AppCommandError> {
    let client = http_client()?;
    poll_pairing_with(&PAIRING_MANAGER, &super::credential_storage::SelectedCredentialStore::current()?, &client, handle).await
}

pub async fn get_auth_state() -> Result<CerebroAuthState, AppCommandError> {
    auth_state_with(&PAIRING_MANAGER, &super::credential_storage::SelectedCredentialStore::current()?).await
}

pub async fn cancel_pairing(handle: &str) -> Result<CerebroAuthState, AppCommandError> {
    cancel_pairing_with(&PAIRING_MANAGER, handle).await
}

pub fn forget_runner_credential() -> Result<CerebroAuthState, AppCommandError> {
    super::credential_storage::SelectedCredentialStore::current()?.clear()?;
    RUNNER_IDENTITY_CHANGED.notify_waiters();
    Ok(CerebroAuthState {
        paired: false,
        cerebro_base_url: None,
        runner_id: None,
        pairing: None,
    })
}

pub async fn refresh_access_token() -> Result<CerebroRunnerAccess, AppCommandError> {
    let client = http_client()?;
    refresh_access_token_with(&super::credential_storage::SelectedCredentialStore::current()?, &client).await
}

/// 存活 Bridge 固定启动时身份；后续存储切换只影响新连接。
pub struct RunnerIdentity {
    credential: RunnerCredential,
}

impl RunnerIdentity {
    pub async fn configuration_request<T: DeserializeOwned>(&self, action: &str, body: &impl Serialize) -> Result<T, AppCommandError> {
        let client = http_client()?;
        let access = refresh_credential(&client, self.credential.clone()).await?;
        post_json(&client, endpoint(&access.cerebro_base_url, &format!("api/v1/execution-runner-configurations/{action}"))?, body, Some(&access.access_token))
            .await.map_err(ClientError::into_app_error)
    }

    pub fn current() -> Result<Self, AppCommandError> {
        let credential = super::credential_storage::SelectedCredentialStore::current()?.load()?
            .ok_or_else(|| AppCommandError::configuration_missing("This Dextra is not paired with Cerebro"))?;
        Ok(Self { credential })
    }

}

/// 用户显式切换会结束本次配对，但不删除任一旧存储条目。
pub async fn select_credential_storage(mode: super::credential_storage::StorageMode) -> Result<super::credential_storage::StorageSettings, AppCommandError> {
    let mut pending = PAIRING_MANAGER.pending.lock().await;
    let settings = super::credential_storage::select(mode)?;
    *pending = None;
    RUNNER_IDENTITY_CHANGED.notify_waiters();
    Ok(settings)
}

pub fn import_runner_credential() -> Result<(), AppCommandError> {
    super::credential_storage::import_existing()?;
    RUNNER_IDENTITY_CHANGED.notify_waiters();
    Ok(())
}

/// 等待配对或断开动作唤醒后台 Runner 连接。
pub(crate) async fn wait_for_runner_identity_change() {
    RUNNER_IDENTITY_CHANGED.notified().await;
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex as StdMutex};

    use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};

    use super::*;

    fn contract_example(path: &str, kind: &str) -> serde_json::Value {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/contracts/cerebro-runner.openapi.json"
        ))
        .unwrap();
        fixture["x-dextra-contract-examples"][path][kind].clone()
    }

    #[test]
    fn client_decodes_current_server_identity_and_configuration_contracts() {
        // 消费服务端生成样例；新增合法响应字段不会要求维护测试字段全集。
        let _: CerebroEnvelope<PairingCreateData> = serde_json::from_value(contract_example(&format!("/{PAIRING_CREATE_PATH}"), "response")).unwrap();
        let _: CerebroEnvelope<PairingExchangeData> = serde_json::from_value(contract_example(&format!("/{PAIRING_EXCHANGE_PATH}"), "response")).unwrap();
        let _: CerebroEnvelope<TokenRefreshData> = serde_json::from_value(contract_example(&format!("/{TOKEN_REFRESH_PATH}"), "response")).unwrap();
        let _: CerebroEnvelope<super::super::configuration::ClientConfiguration> = serde_json::from_value(contract_example("/api/v1/execution-runner-configurations/query", "response")).unwrap();
        let _: CerebroEnvelope<super::super::configuration::FolderCredential> = serde_json::from_value(contract_example("/api/v1/execution-runner-configurations/credential", "response")).unwrap();
    }

    #[derive(Default)]
    struct MemoryStore {
        value: StdMutex<Option<RunnerCredential>>,
        fail_saves: AtomicUsize,
    }

    impl MemoryStore {
        fn failing_once() -> Self {
            Self {
                value: StdMutex::new(None),
                fail_saves: AtomicUsize::new(1),
            }
        }
    }

    impl CredentialStore for MemoryStore {
        fn load(&self) -> Result<Option<RunnerCredential>, AppCommandError> {
            Ok(self.value.lock().unwrap().clone())
        }

        fn save(&self, credential: &RunnerCredential) -> Result<(), AppCommandError> {
            if self
                .fail_saves
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
            {
                return Err(AppCommandError::io_error("test store unavailable"));
            }
            *self.value.lock().unwrap() = Some(credential.clone());
            Ok(())
        }

        fn clear(&self) -> Result<(), AppCommandError> {
            *self.value.lock().unwrap() = None;
            Ok(())
        }
    }

    #[derive(Clone, Default)]
    struct MockCerebro {
        exchange_calls: Arc<AtomicUsize>,
        pending_first: Arc<AtomicBool>,
        revoke_refresh: Arc<AtomicBool>,
        refresh_credentials: Arc<StdMutex<Vec<String>>>,
        mcp_calls: Arc<AtomicUsize>,
        verification_uri: Arc<StdMutex<String>>,
    }

    async fn create_pairing(State(state): State<MockCerebro>) -> Json<serde_json::Value> {
        Json(serde_json::json!({
            "success": true,
            "data": {
                "device_code": "cvn_device_pair.secret",
                "user_code": "ABCD-EFGH-JKLM",
                "verification_uri": state.verification_uri.lock().unwrap().clone(),
                "expires_in": 600,
                "interval": 5
            }
        }))
    }

    async fn exchange_pairing(State(state): State<MockCerebro>) -> Json<serde_json::Value> {
        state.exchange_calls.fetch_add(1, Ordering::SeqCst);
        if state.pending_first.swap(false, Ordering::SeqCst) {
            return Json(serde_json::json!({
                "success": false,
                "error": {
                    "code": "RUNNER_PAIRING_PENDING",
                    "message": "设备配对尚未批准"
                }
            }));
        }
        Json(serde_json::json!({
            "success": true,
            "data": {
                "runner_id": "runner-1",
                "refresh_credential": "cvn_runner_credential.secret"
            }
        }))
    }

    async fn refresh_token(State(state): State<MockCerebro>, Json(body): Json<serde_json::Value>) -> Json<serde_json::Value> {
        state.refresh_credentials.lock().unwrap().push(body["refresh_credential"].as_str().unwrap().to_string());
        if state.revoke_refresh.load(Ordering::SeqCst) {
            return Json(serde_json::json!({
                "success": false,
                "error": {
                    "code": "RUNNER_REVOKED",
                    "message": "Dextra 凭据已撤销"
                }
            }));
        }
        Json(serde_json::json!({
            "success": true,
            "data": {
                "access_token": "short-lived-access",
                "token_type": "bearer",
                "expires_in": 900
            }
        }))
    }

    async fn issue_folder_credential(
        State(state): State<MockCerebro>,
        headers: HeaderMap,
        Json(body): Json<serde_json::Value>,
    ) -> Json<serde_json::Value> {
        state.mcp_calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(
            headers
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer short-lived-access")
        );
        assert_eq!(body["target_id"], "target-1");
        Json(serde_json::json!({
            "success": true,
            "data": {
                "mcp_url": "http://cerebro.test/mcp/stream",
                "access_token": "mcp-short-lived",
                "expires_at": "2030-01-01T00:10:00Z"
            }
        }))
    }

    async fn mock_server(
        pending_first: bool,
    ) -> (String, MockCerebro, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let state = MockCerebro::default();
        state.pending_first.store(pending_first, Ordering::SeqCst);
        *state.verification_uri.lock().unwrap() =
            format!("http://{address}/execution/runner-pairing");
        let app = Router::new()
            .route(
                "/nested/api/v1/execution-runner-pairings/create",
                post(create_pairing),
            )
            .route(
                "/nested/api/v1/execution-runner-pairings/exchange",
                post(exchange_pairing),
            )
            .route(
                "/nested/api/v1/execution-runner-tokens/refresh",
                post(refresh_token),
            )
            .route(
                "/nested/api/v1/execution-runner-configurations/credential",
                post(issue_folder_credential),
            )
            .with_state(state.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{address}/nested?tenant=local"), state, task)
    }

    #[tokio::test]
    async fn live_identity_keeps_its_original_runner_after_storage_changes() {
        let (base_url, server, handle) = mock_server(false).await;
        let store = MemoryStore::default();
        store.save(&RunnerCredential {
            cerebro_base_url: base_url.clone(), runner_id: "original-runner".into(),
            refresh_credential: "original-credential".into(),
        }).unwrap();
        let identity = RunnerIdentity { credential: store.load().unwrap().unwrap() };
        identity.configuration_request::<super::super::configuration::FolderCredential>("credential", &serde_json::json!({"target_id": "target-1", "rotate": false})).await.unwrap();
        store.save(&RunnerCredential {
            cerebro_base_url: base_url, runner_id: "new-runner".into(),
            refresh_credential: "new-credential".into(),
        }).unwrap();
        identity.configuration_request::<super::super::configuration::FolderCredential>("credential", &serde_json::json!({"target_id": "target-1", "rotate": false})).await.unwrap();
        assert_eq!(*server.refresh_credentials.lock().unwrap(), vec!["original-credential", "original-credential"]);
        handle.abort();
    }

    #[tokio::test]
    async fn device_flow_persists_only_refresh_credential_and_refreshes_access() {
        let (base_url, server, task) = mock_server(true).await;
        let manager = PairingManager::default();
        let store = MemoryStore::default();
        let client = reqwest::Client::new();

        let started = start_pairing_with(&manager, &store, &client, &base_url)
            .await
            .unwrap();
        assert_eq!(started.user_code, "ABCD-EFGH-JKLM");
        assert_eq!(started.interval, 5);
        let pending_state = auth_state_with(&manager, &store).await.unwrap();
        let pending_json = serde_json::to_string(&pending_state).unwrap();
        assert!(!pending_json.contains("cvn_device_pair.secret"));
        assert!(!pending_json.contains("cvn_runner_credential.secret"));

        let pending = poll_pairing_with(&manager, &store, &client, &started.handle)
            .await
            .unwrap();
        assert_eq!(pending.status, PairingPollStatus::Pending);
        assert_eq!(pending.retry_after, Some(5));
        let paired = poll_pairing_with(&manager, &store, &client, &started.handle)
            .await
            .unwrap();
        assert_eq!(paired.status, PairingPollStatus::Paired);
        assert_eq!(paired.runner_id.as_deref(), Some("runner-1"));
        assert_eq!(server.exchange_calls.load(Ordering::SeqCst), 2);

        let auth = auth_state_with(&manager, &store).await.unwrap();
        assert!(auth.paired);
        assert_eq!(auth.runner_id.as_deref(), Some("runner-1"));
        assert!(auth.pairing.is_none());
        let access = refresh_access_token_with(&store, &client).await.unwrap();
        assert_eq!(access.access_token, "short-lived-access");
        assert_eq!(access.expires_in, 900);
        assert_eq!(
            store.load().unwrap().unwrap().refresh_credential,
            "cvn_runner_credential.secret"
        );

        server.revoke_refresh.store(true, Ordering::SeqCst);
        let error = refresh_access_token_with(&store, &client)
            .await
            .unwrap_err();
        assert!(matches!(error.code, AppErrorCode::AuthenticationFailed));
        assert_eq!(error.detail.as_deref(), Some("RUNNER_REVOKED"));
        assert!(store.load().unwrap().is_none());
        task.abort();
    }

    #[tokio::test]
    async fn folder_credential_uses_runner_access_header() {
        let (base_url, server, task) = mock_server(false).await;
        let store = MemoryStore::default();
        store
            .save(&RunnerCredential {
                cerebro_base_url: normalize_base_url(&base_url).unwrap(),
                runner_id: "runner-1".to_string(),
                refresh_credential: "cvn_runner_credential.secret".to_string(),
            })
            .unwrap();

        let identity = RunnerIdentity { credential: store.load().unwrap().unwrap() };
        let principal: super::super::configuration::FolderCredential = identity.configuration_request("credential", &serde_json::json!({"target_id": "target-1", "rotate": false})).await.unwrap();

        assert_eq!(principal.mcp_url, "http://cerebro.test/mcp/stream");
        assert_eq!(principal.access_token, "mcp-short-lived");
        assert_eq!(server.mcp_calls.load(Ordering::SeqCst), 1);
        task.abort();
    }

    #[tokio::test]
    async fn exchanged_credential_survives_a_local_store_retry_without_reexchange() {
        let (base_url, server, task) = mock_server(false).await;
        let manager = PairingManager::default();
        let store = MemoryStore::failing_once();
        let client = reqwest::Client::new();
        let started = start_pairing_with(&manager, &store, &client, &base_url)
            .await
            .unwrap();

        let first = poll_pairing_with(&manager, &store, &client, &started.handle)
            .await
            .unwrap_err();
        assert!(matches!(first.code, AppErrorCode::IoError));
        assert_eq!(server.exchange_calls.load(Ordering::SeqCst), 1);

        let second = poll_pairing_with(&manager, &store, &client, &started.handle)
            .await
            .unwrap();
        assert_eq!(second.status, PairingPollStatus::Paired);
        assert_eq!(server.exchange_calls.load(Ordering::SeqCst), 1);
        assert!(store.load().unwrap().is_some());
        task.abort();
    }

    #[tokio::test]
    async fn active_pairing_is_resumable_and_a_different_url_is_refused() {
        let (base_url, _server, task) = mock_server(false).await;
        let manager = PairingManager::default();
        let store = MemoryStore::default();
        let client = reqwest::Client::new();
        let first = start_pairing_with(&manager, &store, &client, &base_url)
            .await
            .unwrap();
        let resumed = start_pairing_with(&manager, &store, &client, &base_url)
            .await
            .unwrap();
        assert_eq!(resumed.handle, first.handle);

        let error = start_pairing_with(&manager, &store, &client, &format!("{base_url}/different"))
            .await
            .unwrap_err();
        assert!(matches!(error.code, AppErrorCode::AlreadyExists));
        let cancelled = cancel_pairing_with(&manager, &first.handle).await.unwrap();
        assert!(cancelled.pairing.is_none());
        task.abort();
    }
}
