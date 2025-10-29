use super::authenticate::Credential;
use super::dialog::DialogStateSender;
use super::{DialogId, dialog::Dialog, server_dialog::ServerInviteDialog};
use crate::Result;
use crate::dialog::dialog::{DialogInner, DialogStateReceiver};
use crate::transaction::key::TransactionRole;
use crate::transaction::make_tag;
use crate::transaction::{endpoint::EndpointInnerRef, transaction::Transaction};
use rsip::Request;
use std::sync::atomic::{AtomicU32, Ordering};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};
use tracing::info;

/// Internal Dialog Layer State
///
/// `DialogLayerInner` contains the core state for managing multiple SIP dialogs.
/// It maintains a registry of active dialogs and tracks sequence numbers for
/// dialog creation.
///
/// # Fields
///
/// * `last_seq` - Atomic counter for generating unique sequence numbers
/// * `dialogs` - Thread-safe map of active dialogs indexed by DialogId
///
/// # Thread Safety
///
/// This structure is designed to be shared across multiple threads safely:
/// * `last_seq` uses atomic operations for lock-free increments
/// * `dialogs` uses RwLock for concurrent read access with exclusive writes
pub struct DialogLayerInner {
    pub(super) last_seq: AtomicU32,
    pub(super) dialogs: RwLock<HashMap<DialogId, Dialog>>,
}
pub type DialogLayerInnerRef = Arc<DialogLayerInner>;

/// SIP Dialog Layer
///
/// `DialogLayer` provides high-level dialog management functionality for SIP
/// applications. It handles dialog creation, lookup, and lifecycle management
/// while coordinating with the transaction layer.
///
/// # Key Responsibilities
///
/// * Creating and managing SIP dialogs
/// * Dialog identification and routing
/// * Dialog state tracking and cleanup
/// * Integration with transaction layer
/// * Sequence number management
///
/// # Usage Patterns
///
/// ## Server-side Dialog Creation
///
/// ```rust,no_run
/// use rsipstack::dialog::dialog_layer::DialogLayer;
/// use rsipstack::transaction::endpoint::EndpointInner;
/// use std::sync::Arc;
///
/// # fn example() -> rsipstack::Result<()> {
/// # let endpoint: Arc<EndpointInner> = todo!();
/// # let transaction = todo!();
/// # let state_sender = todo!();
/// # let credential = None;
/// # let contact_uri = None;
/// // Create dialog layer
/// let dialog_layer = DialogLayer::new(endpoint.clone());
///
/// // Handle incoming INVITE transaction
/// let server_dialog = dialog_layer.get_or_create_server_invite(
///     &transaction,
///     state_sender,
///     credential,
///     contact_uri
/// )?;
///
/// // Accept the call
/// server_dialog.accept(None, None)?;
/// # Ok(())
/// # }
/// ```
///
/// ## Dialog Lookup and Routing
///
/// ```rust,no_run
/// # use rsipstack::dialog::dialog_layer::DialogLayer;
/// # async fn example() -> rsipstack::Result<()> {
/// # let dialog_layer: DialogLayer = todo!();
/// # let request = todo!();
/// # let transaction = todo!();
/// // Find existing dialog for incoming request
/// if let Some(mut dialog) = dialog_layer.match_dialog(&request) {
///     // Route to existing dialog
///     dialog.handle(transaction).await?;
/// } else {
///     // Create new dialog or reject
/// }
/// # Ok(())
/// # }
/// ```
///
/// ## Dialog Cleanup
///
/// ```rust,no_run
/// # use rsipstack::dialog::dialog_layer::DialogLayer;
/// # fn example() {
/// # let dialog_layer: DialogLayer = todo!();
/// # let dialog_id = todo!();
/// // Remove completed dialog
/// dialog_layer.remove_dialog(&dialog_id);
/// # }
/// ```
///
/// # Dialog Lifecycle
///
/// 1. **Creation** - Dialog created from incoming INVITE or outgoing request
/// 2. **Early State** - Dialog exists but not yet confirmed
/// 3. **Confirmed** - Dialog established with 2xx response and ACK
/// 4. **Active** - Dialog can exchange in-dialog requests
/// 5. **Terminated** - Dialog ended with BYE or error
/// 6. **Cleanup** - Dialog removed from layer
///
/// # Thread Safety
///
/// DialogLayer is thread-safe and can be shared across multiple tasks:
/// * Dialog lookup operations are concurrent
/// * Dialog creation is serialized when needed
/// * Automatic cleanup prevents memory leaks
pub struct DialogLayer {
    pub endpoint: EndpointInnerRef,
    pub inner: DialogLayerInnerRef,
}

impl DialogLayer {
    pub fn new(endpoint: EndpointInnerRef) -> Self {
        Self {
            endpoint,
            inner: Arc::new(DialogLayerInner {
                last_seq: AtomicU32::new(0),
                dialogs: RwLock::new(HashMap::new()),
            }),
        }
    }

    pub fn get_or_create_server_invite(
        &self,
        tx: &Transaction,
        state_sender: DialogStateSender,
        credential: Option<Credential>,
        local_contact: Option<rsip::Uri>,
    ) -> Result<ServerInviteDialog> {
        let mut id = DialogId::try_from(&tx.original)?;
        if !id.to_tag.is_empty() {
            let dlg = self.inner.dialogs.read().unwrap().get(&id).cloned();
            match dlg {
                Some(Dialog::ServerInvite(dlg)) => return Ok(dlg),
                _ => {
                    return Err(crate::Error::DialogError(Box::new((
                        "the dialog not found".to_string(),
                        id,
                        rsip::StatusCode::CallTransactionDoesNotExist,
                    ))));
                }
            }
        }
        id.to_tag = make_tag().to_string(); // generate to tag

        let dlg_inner = DialogInner::new(
            TransactionRole::Server,
            id.clone(),
            tx.original.clone(),
            self.endpoint.clone(),
            state_sender,
            credential,
            local_contact,
            tx.tu_sender.clone(),
        )?;

        let dialog = ServerInviteDialog {
            inner: Arc::new(dlg_inner),
        };
        self.inner
            .dialogs
            .write()
            .unwrap()
            .insert(id.clone(), Dialog::ServerInvite(dialog.clone()));
        info!(%id, "server invite dialog created");
        Ok(dialog)
    }

    pub fn increment_last_seq(&self) -> u32 {
        self.inner.last_seq.fetch_add(1, Ordering::Relaxed);
        self.inner.last_seq.load(Ordering::Relaxed)
    }

    pub fn len(&self) -> usize {
        self.inner.dialogs.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.dialogs.read().unwrap().is_empty()
    }

    pub fn all_dialog_ids(&self) -> Vec<DialogId> {
        self.inner
            .dialogs
            .read()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>()
    }

    pub fn get_dialog(&self, id: &DialogId) -> Option<Dialog> {
        match self.inner.dialogs.read() {
            Ok(dialogs) => dialogs.get(id).cloned(),
            Err(_) => None,
        }
    }

    pub fn remove_dialog(&self, id: &DialogId) {
        info!(%id, "remove dialog");
        if let Some(d) = self.inner.dialogs.write().unwrap().remove(id) {
            d.on_remove()
        }
    }

    pub fn match_dialog(&self, req: &Request) -> Option<Dialog> {
        let id = DialogId::try_from(req).ok()?;
        self.get_dialog(&id)
    }

    pub fn new_dialog_state_channel(&self) -> (DialogStateSender, DialogStateReceiver) {
        tokio::sync::mpsc::unbounded_channel()
    }
}
