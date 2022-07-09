use serde::{Deserialize, Serialize};
use uuid::Uuid;
use warp::raygun::Reaction;
use warp::{
    error::Error,
    raygun::{Message, PinState, ReactionState, SenderId},
    sync::{Arc, Mutex},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MessagingEvents {
    NewMessage(Message),
    EditMessage(Uuid, Uuid, Vec<String>),
    DeleteMessage(Uuid, Uuid),
    DeleteConversation(Uuid),
    PinMessage(Uuid, SenderId, Uuid, PinState),
    ReactMessage(Uuid, SenderId, Uuid, ReactionState, String),
    Ping(Uuid, SenderId),
}

// #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
// pub struct Payload {
//     event: RayGunEvents,
//     signature: Vec<u8>,
//     pk: Vec<u8>,
// }

pub fn process_message_event(
    conversation: Arc<Mutex<Vec<Message>>>,
    events: &MessagingEvents,
) -> Result<(), Error> {
    match events.clone() {
        MessagingEvents::NewMessage(message) => conversation.lock().push(message),
        MessagingEvents::EditMessage(convo_id, message_id, val) => {
            let mut messages = conversation.lock();

            let index = messages
                .iter()
                .position(|conv| conv.conversation_id() == convo_id && conv.id() == message_id)
                .ok_or(Error::ArrayPositionNotFound)?;

            let message = messages
                .get_mut(index)
                .ok_or(Error::ArrayPositionNotFound)?;

            *message.value_mut() = val;
        }
        MessagingEvents::DeleteMessage(convo_id, message_id) => {
            let mut messages = conversation.lock();

            let index = messages
                .iter()
                .position(|conv| conv.conversation_id() == convo_id && conv.id() == message_id)
                .ok_or(Error::ArrayPositionNotFound)?;

            let _ = messages.remove(index);
        }
        MessagingEvents::PinMessage(convo_id, _, message_id, state) => {
            let mut messages = conversation.lock();

            let index = messages
                .iter()
                .position(|conv| conv.conversation_id() == convo_id && conv.id() == message_id)
                .ok_or(Error::ArrayPositionNotFound)?;

            let message = messages
                .get_mut(index)
                .ok_or(Error::ArrayPositionNotFound)?;

            match state {
                PinState::Pin => *message.pinned_mut() = true,
                PinState::Unpin => *message.pinned_mut() = false,
            }
        }
        MessagingEvents::ReactMessage(convo_id, sender, message_id, state, emoji) => {
            let mut messages = conversation.lock();

            let index = messages
                .iter()
                .position(|conv| conv.conversation_id() == convo_id && conv.id() == message_id)
                .ok_or(Error::ArrayPositionNotFound)?;

            let message = messages
                .get_mut(index)
                .ok_or(Error::ArrayPositionNotFound)?;

            let reactions = message.reactions_mut();

            match state {
                ReactionState::Add => {
                    let index = match reactions
                        .iter()
                        .position(|reaction| reaction.emoji().eq(&emoji))
                    {
                        Some(index) => index,
                        None => {
                            let mut reaction = Reaction::default();
                            reaction.set_emoji(&emoji);
                            reaction.set_users(vec![sender]);
                            reactions.push(reaction);
                            return Ok(());
                        }
                    };

                    let reaction = reactions
                        .get_mut(index)
                        .ok_or(Error::ArrayPositionNotFound)?;

                    reaction.users_mut().push(sender);
                }
                ReactionState::Remove => {
                    let index = reactions
                        .iter()
                        .position(|reaction| {
                            reaction.users().contains(&sender) && reaction.emoji().eq(&emoji)
                        })
                        .ok_or(Error::ArrayPositionNotFound)?;

                    let reaction = reactions
                        .get_mut(index)
                        .ok_or(Error::ArrayPositionNotFound)?;

                    let user_index = reaction
                        .users()
                        .iter()
                        .position(|reaction_sender| reaction_sender.eq(&sender))
                        .ok_or(Error::ArrayPositionNotFound)?;

                    reaction.users_mut().remove(user_index);

                    if reaction.users().is_empty() {
                        //Since there is no users listed under the emoji, the reaction should be removed from the message
                        reactions.remove(index);
                    }
                }
            }
        }
        MessagingEvents::DeleteConversation(convo_id) => conversation
            .lock()
            .retain(|item| item.conversation_id() != convo_id),

        MessagingEvents::Ping(_, _) => {}
    }
    Ok(())
}
