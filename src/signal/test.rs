use std::pin::Pin;
use std::{cell::RefCell, collections::HashMap, rc::Rc};

use async_trait::async_trait;
use presage::libsignal_service::groups_v2::Role;
use presage::libsignal_service::prelude::AttachmentIdentifier;
use presage::libsignal_service::prelude::ProfileKey;
use presage::libsignal_service::sender::AttachmentSpec;
use presage::model::contacts::Contact;
use presage::model::groups::{Group, Member};
use presage::proto::AttachmentPointer;
use presage::proto::data_message::Quote;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::Stream;
use uuid::Uuid;

use crate::data::{Channel, GroupData, Message};
use crate::receipt::Receipt;
use crate::util::utc_now_timestamp_msec;

use super::{
    Attachment, GroupMasterKeyBytes, ProfileKeyBytes, Received, ResolvedGroup, SignalManager,
};

/// Members of a group with their profile keys
type GroupMembers = Vec<(Uuid, ProfileKeyBytes)>;

/// Signal manager mock which does not send any messages.
pub struct SignalManagerMock {
    user_id: Uuid,
    pub sent_messages: Rc<RefCell<Vec<Message>>>,
    /// Names returned by profile resolution
    pub profile_names: Rc<RefCell<HashMap<Uuid, String>>>,
    /// Ids for which profile resolution was requested
    pub profile_requests: Rc<RefCell<Vec<Uuid>>>,
    /// Contacts in the signal storage
    pub contacts: Rc<RefCell<Vec<(Uuid, String)>>>,
    /// Groups in the signal storage
    pub groups: Rc<RefCell<Vec<(GroupMasterKeyBytes, GroupMembers)>>>,
}

impl SignalManagerMock {
    pub fn new() -> Self {
        Self {
            user_id: Uuid::nil(),
            sent_messages: Default::default(),
            profile_names: Default::default(),
            profile_requests: Default::default(),
            contacts: Default::default(),
            groups: Default::default(),
        }
    }
}

impl Default for SignalManagerMock {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl SignalManager for SignalManagerMock {
    fn user_id(&self) -> Uuid {
        self.user_id
    }

    async fn resolve_group(
        &mut self,
        master_key_bytes: super::GroupMasterKeyBytes,
    ) -> anyhow::Result<ResolvedGroup> {
        Ok(ResolvedGroup {
            name: "some_group".to_string(),
            group_data: GroupData {
                master_key_bytes,
                members: Default::default(),
                revision: 0,
            },
            profile_keys: Default::default(),
        })
    }

    async fn save_attachment(
        &mut self,
        attachment_pointer: AttachmentPointer,
    ) -> anyhow::Result<Attachment> {
        let id = match attachment_pointer.attachment_identifier.unwrap() {
            AttachmentIdentifier::CdnId(id) => id.to_string(),
            AttachmentIdentifier::CdnKey(id) => id,
        };
        Ok(Attachment {
            id,
            content_type: attachment_pointer.content_type.unwrap(),
            filename: "somefile".to_string().into(),
            size: attachment_pointer.size.unwrap(),
        })
    }

    fn send_receipt(&self, _: Uuid, _: Vec<u64>, _: Receipt) {}

    fn send_text(
        &self,
        _channel: &Channel,
        text: String,
        quote_message: Option<&Message>,
        _edit_message_timestamp: Option<u64>,
        _attachments: Vec<(AttachmentSpec, Vec<u8>)>,
    ) -> (Message, oneshot::Receiver<anyhow::Result<()>>) {
        let message: String = crate::emoji::replace_shortcodes(&text).into_owned();
        let timestamp = utc_now_timestamp_msec();
        let quote = quote_message.map(|message| Quote {
            id: Some(message.arrived_at),
            author_aci: Some(message.from_id.to_string()),
            text: message.message.clone(),
            ..Default::default()
        });
        let quote_message = quote.and_then(Message::from_quote).map(Box::new);
        let message = Message {
            from_id: self.user_id(),
            message: Some(message),
            arrived_at: timestamp,
            quote: quote_message,
            attachments: Default::default(),
            reactions: Default::default(),
            receipt: Receipt::Sent,
            body_ranges: Default::default(),
            send_failed: Default::default(),
            edit: Default::default(),
            edited: Default::default(),
            deleted: Default::default(),
            expire_timer: None,
            expires_at: None,
        };
        self.sent_messages.borrow_mut().push(message.clone());
        let (tx, rx) = oneshot::channel();
        let _ = tx.send(Ok(()));
        (message, rx)
    }

    fn send_reaction(&self, _channel: &Channel, _message: &Message, _emoji: String, _remove: bool) {
    }

    fn send_delete(&self, _channel: &Channel, _target_sent_timestamp: u64) {}

    fn send_delete_for_me(&self, _channel: &Channel, _message: &Message) {}

    fn resolve_profile_names(
        &self,
        profiles: Vec<(Uuid, ProfileKeyBytes)>,
    ) -> mpsc::UnboundedReceiver<(Uuid, String)> {
        let (tx, rx) = mpsc::unbounded_channel();
        let profile_names = self.profile_names.borrow();
        for (id, _) in profiles {
            self.profile_requests.borrow_mut().push(id);
            if let Some(name) = profile_names.get(&id) {
                let _ = tx.send((id, name.clone()));
            }
        }
        rx
    }

    async fn profile_key(&self, _id: Uuid) -> Option<ProfileKeyBytes> {
        None
    }

    async fn request_contacts_sync(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn profile_name(&self, _id: Uuid) -> Option<String> {
        None
    }

    async fn contact(&self, id: Uuid) -> Option<Contact> {
        self.contacts().await.find(|contact| contact.uuid == id)
    }

    async fn receive_messages(&mut self) -> anyhow::Result<Pin<Box<dyn Stream<Item = Received>>>> {
        Ok(Box::pin(tokio_stream::empty()))
    }

    fn clone_boxed(&self) -> Box<dyn SignalManager> {
        Box::new(Self {
            user_id: self.user_id,
            sent_messages: self.sent_messages.clone(),
            profile_names: self.profile_names.clone(),
            profile_requests: self.profile_requests.clone(),
            contacts: self.contacts.clone(),
            groups: self.groups.clone(),
        })
    }

    async fn contacts(&self) -> Box<dyn Iterator<Item = Contact>> {
        let contacts: Vec<_> = self
            .contacts
            .borrow()
            .iter()
            .map(|(uuid, name)| Contact {
                uuid: *uuid,
                phone_number: None,
                name: name.clone(),
                verified: Default::default(),
                profile_key: Default::default(),
                expire_timer: 0,
                expire_timer_version: 2,
                inbox_position: 0,
                avatar: None,
            })
            .collect();
        Box::new(contacts.into_iter())
    }

    async fn groups(&self) -> Box<dyn Iterator<Item = (GroupMasterKeyBytes, Group)>> {
        let groups: Vec<_> = self
            .groups
            .borrow()
            .iter()
            .map(|(master_key_bytes, members)| {
                let group = Group {
                    title: "some_group".to_string(),
                    avatar: Default::default(),
                    disappearing_messages_timer: None,
                    access_control: None,
                    revision: 0,
                    members: members
                        .iter()
                        .map(|&(uuid, profile_key)| Member {
                            aci: uuid.into(),
                            role: Role::Default,
                            profile_key: ProfileKey::create(profile_key),
                            joined_at_revision: 0,
                            label: None,
                            label_emoji: None,
                        })
                        .collect(),
                    pending_members: Default::default(),
                    requesting_members: Default::default(),
                    invite_link_password: Default::default(),
                    description: None,
                };
                (*master_key_bytes, group)
            })
            .collect();
        Box::new(groups.into_iter())
    }
}
