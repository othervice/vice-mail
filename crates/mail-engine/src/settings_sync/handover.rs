//! Stage 22 (decision 0076): the password transfer in the core. The protocol and what a device
//! decides from a listing are in [`super::transfer`]; this module keeps the state in memory,
//! writes and deletes the entries through the sync folder's connection, stores what arrives,
//! and builds what the settings page shows.
//!
//! - **New device:** `start_password_transfer` writes the request with a fresh key pair whose
//!   private half lives only in [`TransferState::own`]; a restart drops it, and the next pass
//!   deletes the request. Every pass looks for a challenge (answered with the reveal) and the
//!   response; while a request runs, the worker checks the folder every two seconds.
//! - **Giving device:** requests of other devices appear in the status after a pass (the
//!   settings page runs one when the user opens "Give passwords"). `open_password_request`
//!   writes the challenge; once the reveal fits the commitment the code is shown, and
//!   `give_passwords` seals the chosen passwords to the revealed key.
//! - **Only matching servers:** the giving device sends a password only for an account whose
//!   IMAP and SMTP servers are the same on both devices, and the new device stores one only if
//!   the servers sent with it are exactly its own (decision 0040). Never sign-in tokens; writes
//!   stay off. Passwords are stored only for accounts that wait for theirs.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::Ordering;

use mail_model::{
    AccountId, CoreError, CoreEvent, CredentialKind, PasswordRequest, PasswordRequestAccount,
    PasswordRequestAccountState, PasswordRequestState, PasswordTransfer, PasswordTransferState,
    SettingsSyncFailure, SettingsSyncStatus, SyncFolder,
};
use mail_store::StoredAccount;

use super::crypto::{OpaqueId, OpenError, SyncKey};
use super::error::SettingsSyncError;
use super::imap::ImapTransport;
use super::local::lock_clock;
use super::service::{core_error, default_device_name, lock};
use super::transfer::{
    GivenItem, Giving, GivingState, OpenedEntry, OwnRequest, RequestId, RequestKey, SeenRequest,
    TRANSFER_LIFETIME_MS, TransferEntry, TransferKind, TransferMessage, WantedAccount, commitment,
    nonce, plan_transfers, seal_items, transfer_listing,
};
use super::transport::{EntryHandle, RemoteEntry, SettingsTransport};
use crate::core::{AppCore, Connector};
use crate::credentials::CredentialRef;

/// How this device's last request ended (shown until the user closes it or asks again).
#[derive(Debug, Clone)]
struct Finished {
    state: PasswordTransferState,
    other_device: Option<String>,
    expires_ms: i64,
    accounts: Vec<AccountId>,
    stored: Vec<AccountId>,
    skipped: Vec<AccountId>,
}

/// The transfer's state in memory (never stored: a restart ends every exchange).
#[derive(Debug, Default)]
pub(crate) struct TransferState {
    own: Option<OwnRequest>,
    finished: Option<Finished>,
    giving: BTreeMap<RequestId, Giving>,
    seen: Vec<SeenRequest>,
    /// Transfer entries were in the folder at the last look: the next check does not skip.
    pub(super) pending: bool,
}

impl TransferState {
    /// The worker checks the folder quickly: a request of this device runs, or a challenge of
    /// this device waits for the new device's key.
    fn active(&self) -> bool {
        self.own.is_some()
            || self
                .giving
                .values()
                .any(|g| g.state == GivingState::Connecting)
    }

    fn finish(
        &mut self,
        state: PasswordTransferState,
        stored: Vec<AccountId>,
        skipped: Vec<AccountId>,
    ) {
        if let Some(own) = self.own.take() {
            self.finished = Some(Finished {
                state,
                other_device: own.chosen.map(|c| c.device_name),
                expires_ms: own.expires_ms,
                accounts: own.accounts.iter().map(|a| a.account).collect(),
                stored,
                skipped,
            });
        }
    }

    /// Everything goes (the sync was turned off or reset).
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }
}

/// Whether `account` can take or give a password through a transfer: a password or an API
/// token (never a sign-in), and its kind.
fn transferable(account: &StoredAccount) -> Option<CredentialKind> {
    let key = CredentialRef::parse(&account.credential_ref).ok()?;
    if key.oauth_provider().is_some() {
        return None;
    }
    Some(CredentialKind::of_credential_ref(&account.credential_ref))
}

/// Whether this device can give the password of `wanted`.
fn readiness(
    wanted: &WantedAccount,
    accounts: &[StoredAccount],
    adopted: &[AccountId],
) -> PasswordRequestAccountState {
    let Some(account) = accounts.iter().find(|a| a.id == wanted.account) else {
        return PasswordRequestAccountState::Unknown;
    };
    let Some(kind) = transferable(account) else {
        return PasswordRequestAccountState::NoPassword;
    };
    if adopted.contains(&account.id) {
        return PasswordRequestAccountState::NoPassword;
    }
    if kind != wanted.sign_in || account.imap != wanted.imap || account.smtp != wanted.smtp {
        return PasswordRequestAccountState::ServersDiffer;
    }
    PasswordRequestAccountState::Ready
}

fn invalid_request(request_id: &str) -> Result<RequestId, CoreError> {
    RequestId::parse(request_id)
        .ok_or_else(|| CoreError::InvalidArgument("not a password request id".into()))
}

fn gone() -> CoreError {
    CoreError::NotFound("the password request is gone or ran out".into())
}

impl<C: Connector> AppCore<C> {
    /// The name of this device in the sync's device list.
    async fn transfer_device_name(&self) -> Result<String, CoreError> {
        let me = self.settings_sync().device.to_string();
        Ok(self
            .store_ref()?
            .sync_devices()
            .await?
            .into_iter()
            .find(|d| d.device_id == me)
            .map_or_else(default_device_name, |d| d.name))
    }

    /// The sync key while the sync is on.
    async fn transfer_key(&self) -> Result<std::sync::Arc<SyncKey>, CoreError> {
        let row = self.store_ref()?.sync_state().await?;
        if !row.enabled || row.account.is_none() {
            return Err(SettingsSyncFailure::Off.into());
        }
        self.sync_key()
            .await?
            .ok_or_else(|| SettingsSyncFailure::KeyMissing.into())
    }

    /// Deletes the entries with the opaque ids `delete`, then writes `writes`, over the sync
    /// folder's connection.
    async fn transfer_io(
        &self,
        key: &SyncKey,
        writes: Vec<TransferEntry>,
        delete: Vec<OpaqueId>,
    ) -> Result<(), CoreError> {
        let row = self.store_ref()?.sync_state().await?;
        let (true, Some(account), Some(folder_name)) = (row.enabled, row.account, row.folder)
        else {
            return Err(SettingsSyncFailure::Off.into());
        };
        let state = self.settings_sync();
        let mut runner = state.runner.lock().await;
        let (mut folder, _) = self
            .open_sync_folder(&mut runner, account, &folder_name, false)
            .await?;
        let now = self.now_ms();
        let result = async {
            let mut transport = ImapTransport::new(&mut folder, now);
            let transport_error = |err| core_error(SettingsSyncError::Transport(err));
            if !delete.is_empty() {
                let handles: Vec<EntryHandle> = transport
                    .list()
                    .await
                    .map_err(transport_error)?
                    .into_iter()
                    .filter(|e| delete.iter().any(|d| d.as_str() == e.header.opaque_id))
                    .map(|e| e.handle)
                    .collect();
                if !handles.is_empty() {
                    transport.delete(&handles).await.map_err(transport_error)?;
                }
            }
            for entry in writes {
                let version = lock_clock(&state.clock)
                    .now()
                    .map_err(|err| core_error(err.into()))?;
                let sealed = key.seal_transfer(&entry, version).map_err(core_error)?;
                transport
                    .write(&sealed.header.to_raw(), sealed.ciphertext)
                    .await
                    .map_err(transport_error)?;
            }
            Ok(())
        }
        .await;
        if result.is_ok() {
            *runner = Some(folder);
        } else if let Err(err) = folder.logout().await {
            tracing::debug!(error = %err, "LOGOUT of the settings sync connection failed");
        }
        result
    }

    fn set_transfer_active(&self) {
        let state = self.settings_sync();
        let active = lock(&state.transfer).active();
        state
            .signals
            .transfer_active
            .store(active, Ordering::SeqCst);
        if active {
            state.signals.wake();
        }
    }

    async fn emit_and_status(&self) -> Result<SettingsSyncStatus, CoreError> {
        let status = self.get_settings_sync_status().await?;
        self.emit_event(CoreEvent::SettingsSyncChanged {
            status: status.clone(),
        });
        Ok(status)
    }

    /// The opaque ids of what this device wrote for its request `own`.
    fn own_entry_ids(key: &SyncKey, own: &OwnRequest, me: super::hlc::DeviceId) -> Vec<OpaqueId> {
        [TransferKind::Request, TransferKind::Reveal]
            .into_iter()
            .map(|kind| key.transfer_id(kind, &own.request_id, me))
            .collect()
    }

    // -----------------------------------------------------------------------------------------
    // Commands: the new device

    /// "Take over passwords from another device": writes a request for every account here that
    /// waits for its password (a password or an API token, never a sign-in). The private key
    /// stays in memory; a restart ends the request. Replaces a running request.
    pub async fn start_password_transfer(&self) -> Result<SettingsSyncStatus, CoreError> {
        let key = self.transfer_key().await?;
        let store = self.store_ref()?;
        let adopted = store.sync_adopted_accounts().await?;
        let wanted: Vec<WantedAccount> = store
            .accounts()
            .await?
            .into_iter()
            .filter(|a| adopted.contains(&a.id))
            .filter_map(|a| {
                transferable(&a).map(|sign_in| WantedAccount {
                    account: a.id,
                    sign_in,
                    imap: a.imap,
                    smtp: a.smtp,
                })
            })
            .collect();
        if wanted.is_empty() {
            return Err(CoreError::InvalidArgument(
                "no account here waits for its password".into(),
            ));
        }
        let state = self.settings_sync();
        let me = state.device;
        let previous = lock(&state.transfer).own.take();
        let delete = previous
            .as_ref()
            .map(|own| Self::own_entry_ids(&key, own, me))
            .unwrap_or_default();
        let request_id = RequestId::random();
        let request_key = RequestKey::generate();
        let expires_ms = self.now_ms() + TRANSFER_LIFETIME_MS;
        let request = TransferEntry {
            request_id: request_id.clone(),
            device: me,
            expires_ms,
            message: TransferMessage::Request {
                device_name: self.transfer_device_name().await?,
                commitment: commitment(&request_id, request_key.public_key()),
                accounts: wanted.clone(),
            },
        };
        self.transfer_io(&key, vec![request], delete).await?;
        {
            let mut transfer = lock(&state.transfer);
            transfer.finished = None;
            transfer.own = Some(OwnRequest {
                request_id,
                key: request_key,
                expires_ms,
                accounts: wanted,
                chosen: None,
            });
            transfer.pending = true;
        }
        tracing::info!("password transfer requested");
        self.set_transfer_active();
        self.emit_and_status().await
    }

    /// Ends this device's request (its entries leave the folder) or closes how the last one
    /// ended.
    pub async fn cancel_password_transfer(&self) -> Result<SettingsSyncStatus, CoreError> {
        let state = self.settings_sync();
        let own = {
            let mut transfer = lock(&state.transfer);
            transfer.finished = None;
            transfer.own.take()
        };
        if let Some(own) = own {
            match self.transfer_key().await {
                Ok(key) => {
                    let delete = Self::own_entry_ids(&key, &own, state.device);
                    if let Err(err) = self.transfer_io(&key, Vec::new(), delete).await {
                        // The next pass of any device deletes them (this device's at once).
                        tracing::info!(error = %err, "password request not deleted yet");
                    }
                }
                Err(err) => tracing::debug!(error = %err, "password request left to expire"),
            }
            tracing::info!("password transfer cancelled");
        }
        self.set_transfer_active();
        self.emit_and_status().await
    }

    // -----------------------------------------------------------------------------------------
    // Commands: the giving device

    /// Answers a request of another device with a challenge; the code follows once the new
    /// device sent its key (a pass, at once while this waits).
    pub async fn open_password_request(
        &self,
        request_id: String,
    ) -> Result<SettingsSyncStatus, CoreError> {
        let id = invalid_request(&request_id)?;
        let key = self.transfer_key().await?;
        let state = self.settings_sync();
        let now = self.now_ms();
        let (expires_ms, answered) = {
            let transfer = lock(&state.transfer);
            let seen = transfer
                .seen
                .iter()
                .find(|s| s.request_id == id && s.expires_ms > now)
                .ok_or_else(gone)?;
            (seen.expires_ms, transfer.giving.contains_key(&id))
        };
        if answered {
            return self.emit_and_status().await;
        }
        let nonce = nonce();
        let challenge = TransferEntry {
            request_id: id.clone(),
            device: state.device,
            expires_ms,
            message: TransferMessage::Challenge {
                device_name: self.transfer_device_name().await?,
                nonce: nonce.clone(),
            },
        };
        self.transfer_io(&key, vec![challenge], Vec::new()).await?;
        {
            let mut transfer = lock(&state.transfer);
            transfer.giving.insert(
                id,
                Giving {
                    nonce,
                    state: GivingState::Connecting,
                },
            );
            transfer.pending = true;
        }
        self.set_transfer_active();
        self.emit_and_status().await
    }

    /// After the user compared the codes: seals the passwords of `accounts` (those the request
    /// asks for, with the same servers here and a password stored) to the new device's key.
    pub async fn give_passwords(
        &self,
        request_id: String,
        accounts: Vec<AccountId>,
    ) -> Result<SettingsSyncStatus, CoreError> {
        let id = invalid_request(&request_id)?;
        let key = self.transfer_key().await?;
        let state = self.settings_sync();
        let now = self.now_ms();
        let (public_key, seen) = {
            let transfer = lock(&state.transfer);
            let seen = transfer
                .seen
                .iter()
                .find(|s| s.request_id == id && s.expires_ms > now)
                .cloned()
                .ok_or_else(gone)?;
            match transfer.giving.get(&id).map(|g| &g.state) {
                Some(GivingState::Confirm { public_key, .. }) => (public_key.clone(), seen),
                _ => {
                    return Err(CoreError::InvalidArgument(
                        "compare the codes on both devices first".into(),
                    ));
                }
            }
        };
        let store = self.store_ref()?;
        let stored = store.accounts().await?;
        let adopted = store.sync_adopted_accounts().await?;
        let chosen: BTreeSet<AccountId> = accounts.into_iter().collect();
        let mut items = Vec::new();
        for wanted in seen.accounts.iter().filter(|w| chosen.contains(&w.account)) {
            if readiness(wanted, &stored, &adopted) != PasswordRequestAccountState::Ready {
                continue;
            }
            let Some(account) = stored.iter().find(|a| a.id == wanted.account) else {
                continue;
            };
            let key = CredentialRef::parse(&account.credential_ref)?;
            let Some(secret) = self.credentials_ref().get(&key).await? else {
                tracing::info!(account = %account.id, "no password stored here; not given");
                continue;
            };
            items.push(GivenItem::new(
                account.id,
                wanted.sign_in,
                account.imap.clone(),
                account.smtp.clone(),
                &secret,
            ));
        }
        if items.is_empty() {
            return Err(CoreError::InvalidArgument(
                "none of the chosen accounts has a password here with the same servers".into(),
            ));
        }
        let count = items.len();
        let sealed = seal_items(&public_key, &id, items).map_err(core_error)?;
        let response = TransferEntry {
            request_id: id.clone(),
            device: state.device,
            expires_ms: seen.expires_ms,
            message: TransferMessage::Response {
                device_name: self.transfer_device_name().await?,
                declined: false,
                sealed: Some(sealed),
            },
        };
        self.transfer_io(&key, vec![response], Vec::new()).await?;
        if let Some(giving) = lock(&state.transfer).giving.get_mut(&id) {
            giving.state = GivingState::Answered { declined: false };
        }
        tracing::info!(count, "passwords given to another device");
        self.set_transfer_active();
        self.emit_and_status().await
    }

    /// Declines a request. If this device answered it already, the new device learns it.
    pub async fn decline_password_request(
        &self,
        request_id: String,
    ) -> Result<SettingsSyncStatus, CoreError> {
        let id = invalid_request(&request_id)?;
        let state = self.settings_sync();
        let (expires_ms, answered) = {
            let mut transfer = lock(&state.transfer);
            let expires_ms = transfer
                .seen
                .iter()
                .find(|s| s.request_id == id)
                .map(|s| s.expires_ms)
                .ok_or_else(gone)?;
            let answered = matches!(
                transfer.giving.get(&id).map(|g| &g.state),
                Some(GivingState::Connecting | GivingState::Confirm { .. })
            );
            if !answered {
                transfer.giving.insert(
                    id.clone(),
                    Giving {
                        nonce: String::new(),
                        state: GivingState::Answered { declined: true },
                    },
                );
            }
            (expires_ms, answered)
        };
        if answered {
            let key = self.transfer_key().await?;
            let response = TransferEntry {
                request_id: id.clone(),
                device: state.device,
                expires_ms,
                message: TransferMessage::Response {
                    device_name: self.transfer_device_name().await?,
                    declined: true,
                    sealed: None,
                },
            };
            self.transfer_io(&key, vec![response], Vec::new()).await?;
            if let Some(giving) = lock(&state.transfer).giving.get_mut(&id) {
                giving.state = GivingState::Answered { declined: true };
            }
        }
        tracing::info!("password request declined");
        self.set_transfer_active();
        self.emit_and_status().await
    }

    // -----------------------------------------------------------------------------------------
    // Every pass

    /// Looks at the transfer entries of a pass's listing (see the module docs). Problems are
    /// logged; the pass goes on.
    pub(crate) async fn handle_transfers<T: SettingsTransport>(
        &self,
        transport: &mut T,
        key: &SyncKey,
        listing: &[RemoteEntry],
        now: i64,
    ) {
        let state = self.settings_sync();
        let sorted = transfer_listing(listing, now);
        let mut delete: Vec<EntryHandle> = sorted.expired.clone();
        let mut opened: Vec<OpenedEntry> = Vec::new();
        if !sorted.fetch.is_empty() {
            let handles: Vec<EntryHandle> = sorted.fetch.iter().map(|(h, _)| *h).collect();
            match transport.read(&handles).await {
                Ok(bodies) => {
                    let bodies: BTreeMap<EntryHandle, Vec<u8>> = bodies.into_iter().collect();
                    for (handle, header) in &sorted.fetch {
                        let Some(body) = bodies.get(handle) else {
                            continue;
                        };
                        match key.open_transfer(header, body) {
                            Ok(entry) => opened.push(OpenedEntry {
                                handle: *handle,
                                header: header.clone(),
                                entry,
                            }),
                            // Changed by the server, or not of this app: transfer entries are
                            // short-lived, so they go.
                            Err(
                                err @ (OpenError::Damaged
                                | OpenError::HeaderMismatch(_)
                                | OpenError::Malformed(_)),
                            ) => {
                                tracing::warn!(uid = handle.0, %err, "password transfer entry rejected");
                                delete.push(*handle);
                            }
                            Err(err) => {
                                tracing::debug!(uid = handle.0, %err, "password transfer entry left alone");
                            }
                        }
                    }
                }
                Err(err) => tracing::warn!(error = %err, "password transfer entries not read"),
            }
        }
        let me = state.device;
        let (plan, own_key) = {
            let mut transfer = lock(&state.transfer);
            let expired = transfer.own.as_ref().is_some_and(|o| o.expires_ms <= now);
            if expired {
                transfer.finish(PasswordTransferState::Expired, Vec::new(), Vec::new());
                tracing::info!("password request ran out");
            }
            let TransferState { own, giving, .. } = &mut *transfer;
            let plan = plan_transfers(&opened, now, me, own.as_mut(), giving);
            transfer.seen.clone_from(&plan.seen);
            if plan.own_missing && transfer.own.is_some() {
                transfer.finish(PasswordTransferState::Failed, Vec::new(), Vec::new());
                tracing::warn!("this device's password request vanished from the folder");
            }
            // The answer came: the key leaves the state with the request.
            let own_key = plan.response.as_ref().and_then(|_| transfer.own.take());
            (plan, own_key)
        };
        delete.extend(plan.deletes.iter().copied());
        if let (Some(message), Some(own)) = (&plan.response, own_key) {
            let (state_after, stored, skipped) = self.receive_passwords(&own, message).await;
            delete.extend(
                opened
                    .iter()
                    .filter(|e| e.entry.request_id == own.request_id)
                    .map(|e| e.handle),
            );
            let mut transfer = lock(&state.transfer);
            transfer.own = Some(own);
            transfer.finish(state_after, stored, skipped);
        }
        delete.sort_unstable();
        delete.dedup();
        if !delete.is_empty()
            && let Err(err) = transport.delete(&delete).await
        {
            tracing::info!(error = %err, "password transfer entries not deleted yet");
        }
        for entry in plan.writes {
            let written = match lock_clock(&state.clock).now() {
                Ok(version) => key
                    .seal_transfer(&entry, version)
                    .map_err(|e| e.to_string()),
                Err(err) => Err(err.to_string()),
            };
            let result = match written {
                Ok(sealed) => transport
                    .write(&sealed.header.to_raw(), sealed.ciphertext)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
                Err(err) => Err(err),
            };
            // Written again by the next pass.
            if let Err(err) = result {
                tracing::warn!(error = %err, "password transfer entry not written");
            }
        }
        lock(&state.transfer).pending = !opened.is_empty() || !sorted.expired.is_empty();
        self.set_transfer_active();
    }

    /// Stores the passwords of a response: only for accounts asked for that still wait for
    /// theirs, with the same kind of secret and exactly the servers sent along.
    async fn receive_passwords(
        &self,
        own: &OwnRequest,
        message: &TransferMessage,
    ) -> (PasswordTransferState, Vec<AccountId>, Vec<AccountId>) {
        let asked: Vec<AccountId> = own.accounts.iter().map(|a| a.account).collect();
        let TransferMessage::Response {
            declined, sealed, ..
        } = message
        else {
            return (PasswordTransferState::Failed, Vec::new(), asked);
        };
        if *declined {
            tracing::info!("the other device declined the password request");
            return (PasswordTransferState::Declined, Vec::new(), Vec::new());
        }
        let items = match sealed
            .as_deref()
            .map(|sealed| own.key.open(&own.request_id, sealed))
        {
            Some(Ok(items)) => items,
            Some(Err(err)) => {
                tracing::warn!(%err, "the passwords from the other device could not be opened");
                return (PasswordTransferState::Failed, Vec::new(), asked);
            }
            None => return (PasswordTransferState::Failed, Vec::new(), asked),
        };
        let Ok(store) = self.store_ref() else {
            return (PasswordTransferState::Failed, Vec::new(), asked);
        };
        let adopted = store.sync_adopted_accounts().await.unwrap_or_default();
        let mut stored = Vec::new();
        for item in items {
            if !asked.contains(&item.account) || stored.contains(&item.account) {
                continue;
            }
            let account = match store.account(item.account).await {
                Ok(Some(account)) => account,
                _ => continue,
            };
            let fits = adopted.contains(&account.id)
                && transferable(&account) == Some(item.sign_in)
                && account.imap == item.imap
                && account.smtp == item.smtp;
            if !fits {
                tracing::info!(account = %account.id, "password from another device not stored: other servers here, or not waiting");
                continue;
            }
            match self.install_password(account, item.secret()).await {
                Ok(()) => stored.push(item.account),
                Err(err) => {
                    tracing::warn!(account = %item.account, error = %err, "password from another device not stored");
                }
            }
        }
        let skipped = asked
            .iter()
            .filter(|a| !stored.contains(a))
            .copied()
            .collect();
        tracing::info!(
            stored = stored.len(),
            "passwords from another device stored"
        );
        if !stored.is_empty() {
            self.emit_event(CoreEvent::AccountsChanged);
        }
        (PasswordTransferState::Done, stored, skipped)
    }

    // -----------------------------------------------------------------------------------------
    // Status

    /// What the settings page shows about transfers.
    pub(crate) fn transfer_views(
        &self,
        accounts: &[StoredAccount],
        adopted: &[AccountId],
    ) -> (Option<PasswordTransfer>, Vec<PasswordRequest>) {
        let now = self.now_ms();
        let transfer = lock(&self.settings_sync().transfer);
        let own = match (&transfer.own, &transfer.finished) {
            (Some(own), _) => Some(PasswordTransfer {
                state: if own.expires_ms <= now {
                    PasswordTransferState::Expired
                } else if own.chosen.is_some() {
                    PasswordTransferState::Confirm
                } else {
                    PasswordTransferState::Waiting
                },
                code: own.chosen.as_ref().map(|c| c.code.clone()),
                other_device: own.chosen.as_ref().map(|c| c.device_name.clone()),
                expires_at: own.expires_ms,
                accounts: own.accounts.iter().map(|a| a.account).collect(),
                stored: Vec::new(),
                skipped: Vec::new(),
            }),
            (None, Some(done)) => Some(PasswordTransfer {
                state: done.state,
                code: None,
                other_device: done.other_device.clone(),
                expires_at: done.expires_ms,
                accounts: done.accounts.clone(),
                stored: done.stored.clone(),
                skipped: done.skipped.clone(),
            }),
            (None, None) => None,
        };
        let requests = transfer
            .seen
            .iter()
            .filter(|seen| seen.expires_ms > now)
            .map(|seen| {
                let (state, code) = match transfer.giving.get(&seen.request_id).map(|g| &g.state) {
                    None => (PasswordRequestState::Open, None),
                    Some(GivingState::Connecting) => (PasswordRequestState::Connecting, None),
                    Some(GivingState::Confirm { code, .. }) => {
                        (PasswordRequestState::Confirm, Some(code.clone()))
                    }
                    Some(GivingState::OtherDevice) => (PasswordRequestState::OtherDevice, None),
                    Some(GivingState::Failed) => (PasswordRequestState::Failed, None),
                    Some(GivingState::Answered { declined: false }) => {
                        (PasswordRequestState::Given, None)
                    }
                    Some(GivingState::Answered { declined: true }) => {
                        (PasswordRequestState::Declined, None)
                    }
                };
                PasswordRequest {
                    request_id: seen.request_id.to_string(),
                    device_name: seen.device_name.clone(),
                    expires_at: seen.expires_ms,
                    state,
                    code,
                    accounts: seen
                        .accounts
                        .iter()
                        .map(|wanted| PasswordRequestAccount {
                            account_id: wanted.account,
                            state: readiness(wanted, accounts, adopted),
                        })
                        .collect(),
                }
            })
            .collect();
        (own, requests)
    }
}
