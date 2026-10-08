//! Abstraction of a Signal client

use std::path::PathBuf;
use std::pin::Pin;

use async_trait::async_trait;
use presage::libsignal_service::content::Content;
use presage::libsignal_service::sender::AttachmentSpec;
use presage::model::contacts::Contact;
use presage::model::groups::Group;
use presage::proto::AttachmentPointer;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use tokio_stream::Stream;
use uuid::Uuid;

use crate::data::{Channel, GroupData, Message};
use crate::receipt::Receipt;

use super::{GroupMasterKeyBytes, ProfileKeyBytes};

/// Abstract functionalities of Signal required by the app, that is, dependency inversion
#[async_trait(?Send)]
pub trait SignalManager {
    fn clone_boxed(&self) -> Box<dyn SignalManager>;

    fn user_id(&self) -> Uuid;

    async fn resolve_group(
        &mut self,
        master_key_bytes: GroupMasterKeyBytes,
    ) -> anyhow::Result<ResolvedGroup>;

    async fn save_attachment(
        &mut self,
        attachment_pointer: AttachmentPointer,
    ) -> anyhow::Result<Attachment>;

    fn send_receipt(&self, sender_uuid: Uuid, timestamps: Vec<u64>, receipt: Receipt);

    fn send_text(
        &self,
        channel: &Channel,
        text: String,
        quote_message: Option<&Message>,
        edit_message_timestamp: Option<u64>,
        attachments: Vec<(AttachmentSpec, Vec<u8>)>,
    ) -> (Message, oneshot::Receiver<anyhow::Result<()>>);

    fn send_reaction(&self, channel: &Channel, message: &Message, emoji: String, remove: bool);

    fn send_delete(&self, channel: &Channel, target_sent_timestamp: u64);

    fn send_delete_for_me(&self, channel: &Channel, message: &Message);

    async fn profile_name(&self, id: Uuid) -> Option<String>;

    /// Resolves names from users' profiles via Signal server in the background
    ///
    /// Profiles are fetched sequentially. Only successfully resolved non-empty names are sent to
    /// the returned receiver; failures are logged.
    fn resolve_profile_names(
        &self,
        profiles: Vec<(Uuid, ProfileKeyBytes)>,
    ) -> mpsc::UnboundedReceiver<(Uuid, String)>;

    /// Profile key of a user from the signal storage
    async fn profile_key(&self, id: Uuid) -> Option<ProfileKeyBytes>;

    /// Requests the primary device to send all its contacts
    ///
    /// Contacts are received asynchronously in the messages stream as [`Received::Contacts`].
    async fn request_contacts_sync(&self) -> anyhow::Result<()>;

    async fn contact(&self, id: Uuid) -> Option<Contact>;

    async fn receive_messages(&mut self) -> anyhow::Result<Pin<Box<dyn Stream<Item = Received>>>>;

    async fn contacts(&self) -> Box<dyn Iterator<Item = Contact>>;
    async fn groups(&self) -> Box<dyn Iterator<Item = (GroupMasterKeyBytes, Group)>>;
}

/// Item of the incoming messages stream
pub enum Received {
    Content(Box<Content>),
    /// Contacts were synchronized from the primary device and are available in the storage
    Contacts,
}

pub struct ResolvedGroup {
    pub name: String,
    pub group_data: GroupData,
    pub profile_keys: Vec<ProfileKeyBytes>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub id: String,
    pub content_type: String,
    pub filename: PathBuf,
    pub size: u32,
}
