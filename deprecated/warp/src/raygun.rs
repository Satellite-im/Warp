// #[cfg(not(target_arch = "wasm32"))]
// pub mod ffi {
//     use super::{
//         Conversation, ConversationType, FFIResult_FFIVec_Conversation, MessageEventStream,
//         RayGunEventStream,
//     };
//     use crate::async_on_block;
//     use crate::crypto::{FFIVec_DID, DID};
//     use crate::error::Error;
//     use crate::ffi::{FFIResult, FFIResult_Null, FFIResult_String, FFIVec_String};
//     use crate::raygun::{
//         EmbedState, FFIResult_FFIVec_Message, FFIVec_Reaction, Message, MessageOptions, PinState,
//         RayGunAdapter, Reaction, ReactionState,
//     };
//     // use chrono::{DateTime, NaiveDateTime, Utc};
//     use futures::StreamExt;
//     use std::ffi::{CStr, CString};
//     use std::os::raw::c_char;
//     use std::str::{FromStr, Utf8Error};
//     use uuid::Uuid;

//     #[allow(clippy::await_holding_lock)]
//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn raygun_create_conversation(
//         ctx: *mut RayGunAdapter,
//         did_key: *const DID,
//     ) -> FFIResult<Conversation> {
//         if ctx.is_null() {
//             return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
//         }

//         if did_key.is_null() {
//             return FFIResult::err(Error::Any(anyhow::anyhow!("did_key cannot be null")));
//         }

//         let adapter = &mut *ctx;

//         async_on_block(adapter.create_conversation(&*did_key)).into()
//     }

//     #[allow(clippy::await_holding_lock)]
//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn raygun_list_conversations(
//         ctx: *const RayGunAdapter,
//     ) -> FFIResult_FFIVec_Conversation {
//         if ctx.is_null() {
//             return FFIResult_FFIVec_Conversation::err(Error::Any(anyhow::anyhow!(
//                 "Context cannot be null"
//             )));
//         }

//         let adapter = &*ctx;

//         async_on_block(adapter.list_conversations()).into()
//     }

//     #[allow(clippy::await_holding_lock)]
//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn raygun_get_message(
//         ctx: *const RayGunAdapter,
//         convo_id: *const c_char,
//         message_id: *const c_char,
//     ) -> FFIResult<Message> {
//         if ctx.is_null() {
//             return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
//         }

//         if convo_id.is_null() {
//             return FFIResult::err(Error::Any(anyhow::anyhow!(
//                 "Conversation id cannot be null"
//             )));
//         }

//         if message_id.is_null() {
//             return FFIResult::err(Error::Any(anyhow::anyhow!(
//                 "Conversation id cannot be null"
//             )));
//         }

//         let convo_id = match Uuid::from_str(&CStr::from_ptr(convo_id).to_string_lossy()) {
//             Ok(uuid) => uuid,
//             Err(e) => return FFIResult::err(Error::Any(anyhow::anyhow!(e))),
//         };

//         let message_id = match Uuid::from_str(&CStr::from_ptr(message_id).to_string_lossy()) {
//             Ok(uuid) => uuid,
//             Err(e) => return FFIResult::err(Error::Any(anyhow::anyhow!(e))),
//         };

//         let adapter = &*ctx;
//         async_on_block(adapter.get_message(convo_id, message_id)).into()
//     }

//     #[allow(clippy::await_holding_lock)]
//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn raygun_get_messages(
//         ctx: *const RayGunAdapter,
//         convo_id: *const c_char,
//         option: *const MessageOptions,
//     ) -> FFIResult_FFIVec_Message {
//         if ctx.is_null() {
//             return FFIResult_FFIVec_Message::err(Error::Any(anyhow::anyhow!(
//                 "Context cannot be null"
//             )));
//         }

//         if convo_id.is_null() {
//             return FFIResult_FFIVec_Message::err(Error::Any(anyhow::anyhow!(
//                 "Conversation id cannot be null"
//             )));
//         }

//         let option = match option.is_null() {
//             true => MessageOptions::default(),
//             false => (*option).clone(),
//         };

//         let convo_id = match Uuid::from_str(&CStr::from_ptr(convo_id).to_string_lossy()) {
//             Ok(uuid) => uuid,
//             Err(e) => return FFIResult_FFIVec_Message::err(Error::Any(anyhow::anyhow!(e))),
//         };

//         let adapter = &*ctx;
//         async_on_block(adapter.get_messages(convo_id, option))
//             .and_then(Vec::<Message>::try_from)
//             .into()
//     }

//     #[allow(clippy::await_holding_lock)]
//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn raygun_send(
//         ctx: *mut RayGunAdapter,
//         convo_id: *const c_char,
//         messages: *const *const c_char,
//         lines: usize,
//     ) -> FFIResult_Null {
//         if ctx.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
//         }

//         if convo_id.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!(
//                 "Conversation ID cannot be null"
//             )));
//         }

//         if messages.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!(
//                 "Message array cannot be null"
//             )));
//         }

//         if lines == 0 {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!(
//                 "Lines has to be more than zero"
//             )));
//         }

//         let convo_id = match Uuid::from_str(&CStr::from_ptr(convo_id).to_string_lossy()) {
//             Ok(uuid) => uuid,
//             Err(e) => return FFIResult_Null::err(Error::UuidError(e)),
//         };

//         let messages = match pointer_to_vec(messages, lines) {
//             Ok(messages) => messages,
//             Err(e) => return FFIResult_Null::err(Error::Any(anyhow::anyhow!(e))),
//         };

//         let adapter = &mut *ctx;
//         async_on_block(adapter.send(convo_id, messages)).into()
//     }

//     #[allow(clippy::await_holding_lock)]
//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn raygun_edit(
//         ctx: *mut RayGunAdapter,
//         convo_id: *const c_char,
//         message_id: *const c_char,
//         messages: *const *const c_char,
//         lines: usize,
//     ) -> FFIResult_Null {
//         if ctx.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
//         }

//         if convo_id.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!(
//                 "Conversation ID cannot be null"
//             )));
//         }

//         if message_id.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!(
//                 "Conversation ID cannot be null"
//             )));
//         }

//         if messages.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!(
//                 "Message array cannot be null"
//             )));
//         }

//         if lines == 0 {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!(
//                 "Lines has to be more than zero"
//             )));
//         }

//         let convo_id = match Uuid::from_str(&CStr::from_ptr(convo_id).to_string_lossy()) {
//             Ok(uuid) => uuid,
//             Err(e) => return FFIResult_Null::err(Error::UuidError(e)),
//         };

//         let msg_id = match Uuid::from_str(&CStr::from_ptr(message_id).to_string_lossy()) {
//             Ok(uuid) => uuid,
//             Err(e) => return FFIResult_Null::err(Error::UuidError(e)),
//         };

//         let messages = match pointer_to_vec(messages, lines) {
//             Ok(messages) => messages,
//             Err(e) => return FFIResult_Null::err(Error::Any(anyhow::anyhow!(e))),
//         };

//         let adapter = &mut *ctx;
//         async_on_block(adapter.edit(convo_id, msg_id, messages)).into()
//     }

//     #[allow(clippy::await_holding_lock)]
//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn raygun_delete(
//         ctx: *mut RayGunAdapter,
//         convo_id: *const c_char,
//         message_id: *const c_char,
//     ) -> FFIResult_Null {
//         if ctx.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
//         }

//         if convo_id.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!(
//                 "Conversation ID cannot be null"
//             )));
//         }

//         let convo_id = match Uuid::from_str(&CStr::from_ptr(convo_id).to_string_lossy()) {
//             Ok(uuid) => uuid,
//             Err(e) => return FFIResult_Null::err(Error::UuidError(e)),
//         };

//         let msg_id = match message_id.is_null() {
//             true => None,
//             false => match Uuid::from_str(&CStr::from_ptr(message_id).to_string_lossy()) {
//                 Ok(uuid) => Some(uuid),
//                 Err(e) => return FFIResult_Null::err(Error::UuidError(e)),
//             },
//         };

//         let adapter = &mut *ctx;
//         async_on_block(adapter.delete(convo_id, msg_id)).into()
//     }

//     #[allow(clippy::await_holding_lock)]
//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn raygun_react(
//         ctx: *mut RayGunAdapter,
//         convo_id: *const c_char,
//         message_id: *const c_char,
//         state: ReactionState,
//         emoji: *const c_char,
//     ) -> FFIResult_Null {
//         if ctx.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
//         }

//         if convo_id.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!(
//                 "Conversation ID cannot be null"
//             )));
//         }

//         if message_id.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Message id cannot be null")));
//         }

//         if emoji.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Emoji cannot be null")));
//         }

//         let convo_id = match Uuid::from_str(&CStr::from_ptr(convo_id).to_string_lossy()) {
//             Ok(uuid) => uuid,
//             Err(e) => return FFIResult_Null::err(Error::UuidError(e)),
//         };

//         let msg_id = match Uuid::from_str(&CStr::from_ptr(message_id).to_string_lossy()) {
//             Ok(uuid) => uuid,
//             Err(e) => return FFIResult_Null::err(Error::UuidError(e)),
//         };

//         let emoji = CStr::from_ptr(emoji).to_string_lossy().to_string();

//         let adapter = &mut *ctx;
//         async_on_block(adapter.react(convo_id, msg_id, state, emoji)).into()
//     }

//     #[allow(clippy::await_holding_lock)]
//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn raygun_pin(
//         ctx: *mut RayGunAdapter,
//         convo_id: *const c_char,
//         message_id: *const c_char,
//         state: PinState,
//     ) -> FFIResult_Null {
//         if ctx.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
//         }

//         if convo_id.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!(
//                 "Conversation ID cannot be null"
//             )));
//         }

//         if message_id.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Message id cannot be null")));
//         }

//         let convo_id = match Uuid::from_str(&CStr::from_ptr(convo_id).to_string_lossy()) {
//             Ok(uuid) => uuid,
//             Err(e) => return FFIResult_Null::err(Error::UuidError(e)),
//         };

//         let msg_id = match Uuid::from_str(&CStr::from_ptr(message_id).to_string_lossy()) {
//             Ok(uuid) => uuid,
//             Err(e) => return FFIResult_Null::err(Error::UuidError(e)),
//         };

//         let adapter = &mut *ctx;
//         async_on_block(adapter.pin(convo_id, msg_id, state)).into()
//     }

//     #[allow(clippy::await_holding_lock)]
//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn raygun_reply(
//         ctx: *mut RayGunAdapter,
//         convo_id: *const c_char,
//         message_id: *const c_char,
//         messages: *const *const c_char,
//         lines: usize,
//     ) -> FFIResult_Null {
//         if ctx.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
//         }

//         if convo_id.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!(
//                 "Conversation ID cannot be null"
//             )));
//         }

//         if message_id.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Message id cannot be null")));
//         }

//         if messages.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Messages cannot be null")));
//         }

//         let convo_id = match Uuid::from_str(&CStr::from_ptr(convo_id).to_string_lossy()) {
//             Ok(uuid) => uuid,
//             Err(e) => return FFIResult_Null::err(Error::UuidError(e)),
//         };

//         let msg_id = match Uuid::from_str(&CStr::from_ptr(message_id).to_string_lossy()) {
//             Ok(uuid) => uuid,
//             Err(e) => return FFIResult_Null::err(Error::UuidError(e)),
//         };

//         let messages = match pointer_to_vec(messages, lines) {
//             Ok(messages) => messages,
//             Err(e) => return FFIResult_Null::err(Error::Any(anyhow::anyhow!(e))),
//         };

//         let adapter = &mut *ctx;
//         async_on_block(adapter.reply(convo_id, msg_id, messages)).into()
//     }

//     #[allow(clippy::await_holding_lock)]
//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn raygun_embeds(
//         ctx: *mut RayGunAdapter,
//         convo_id: *const c_char,
//         message_id: *const c_char,
//         state: EmbedState,
//     ) -> FFIResult_Null {
//         if ctx.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
//         }

//         if convo_id.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!(
//                 "Conversation ID cannot be null"
//             )));
//         }

//         if message_id.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Message id cannot be null")));
//         }

//         let convo_id = match Uuid::from_str(&CStr::from_ptr(convo_id).to_string_lossy()) {
//             Ok(uuid) => uuid,
//             Err(e) => return FFIResult_Null::err(Error::UuidError(e)),
//         };

//         let msg_id = match Uuid::from_str(&CStr::from_ptr(message_id).to_string_lossy()) {
//             Ok(uuid) => uuid,
//             Err(e) => return FFIResult_Null::err(Error::UuidError(e)),
//         };

//         let adapter = &mut *ctx;
//         async_on_block(adapter.embeds(convo_id, msg_id, state)).into()
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn raygun_subscribe(
//         ctx: *mut RayGunAdapter,
//     ) -> FFIResult<RayGunEventStream> {
//         if ctx.is_null() {
//             return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
//         }

//         let rg = &mut *(ctx);

//         async_on_block(rg.subscribe()).into()
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn raygun_stream_next(ctx: *mut RayGunEventStream) -> FFIResult_String {
//         if ctx.is_null() {
//             return FFIResult_String::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
//         }

//         let stream = &mut *(ctx);

//         match async_on_block(stream.next()) {
//             Some(event) => serde_json::to_string(&event).map_err(Error::from).into(),
//             None => FFIResult_String::err(Error::Any(anyhow::anyhow!(
//                 "Error obtaining data from stream"
//             ))),
//         }
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn raygun_get_conversation_stream(
//         ctx: *mut RayGunAdapter,
//         conversation_id: *const c_char,
//     ) -> FFIResult<MessageEventStream> {
//         if ctx.is_null() {
//             return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
//         }

//         if conversation_id.is_null() {
//             return FFIResult::err(Error::Any(anyhow::anyhow!(
//                 "Conversation ID cannot be null"
//             )));
//         }

//         let conversation_id =
//             match Uuid::from_str(&CStr::from_ptr(conversation_id).to_string_lossy()) {
//                 Ok(uuid) => uuid,
//                 Err(e) => return FFIResult::err(Error::UuidError(e)),
//             };

//         let rg = &mut *(ctx);

//         async_on_block(rg.get_conversation_stream(conversation_id)).into()
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn message_stream_next(ctx: *mut MessageEventStream) -> FFIResult_String {
//         if ctx.is_null() {
//             return FFIResult_String::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
//         }

//         let stream = &mut *(ctx);

//         match async_on_block(stream.next()) {
//             Some(event) => serde_json::to_string(&event).map_err(Error::from).into(),
//             None => FFIResult_String::err(Error::Any(anyhow::anyhow!(
//                 "Error obtaining data from stream"
//             ))),
//         }
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn message_id(ctx: *const Message) -> *mut c_char {
//         if ctx.is_null() {
//             return std::ptr::null_mut();
//         }
//         let adapter = &*ctx;
//         match CString::new(adapter.id().to_string()) {
//             Ok(c) => c.into_raw(),
//             Err(_) => std::ptr::null_mut(),
//         }
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn message_conversation_id(ctx: *const Message) -> *mut c_char {
//         if ctx.is_null() {
//             return std::ptr::null_mut();
//         }
//         let adapter = &*ctx;
//         match CString::new(adapter.conversation_id().to_string()) {
//             Ok(c) => c.into_raw(),
//             Err(_) => std::ptr::null_mut(),
//         }
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn message_sender(ctx: *const Message) -> *mut DID {
//         if ctx.is_null() {
//             return std::ptr::null_mut();
//         }
//         let adapter = &*ctx;
//         Box::into_raw(Box::new(adapter.sender()))
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn message_date(ctx: *const Message) -> *mut c_char {
//         if ctx.is_null() {
//             return std::ptr::null_mut();
//         }
//         let adapter = &*ctx;
//         match CString::new(adapter.date().to_string()) {
//             Ok(c) => c.into_raw(),
//             Err(_) => std::ptr::null_mut(),
//         }
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn message_pinned(ctx: *const Message) -> bool {
//         if ctx.is_null() {
//             return false;
//         }
//         let adapter = &*ctx;
//         adapter.pinned()
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn message_reactions(ctx: *const Message) -> *mut FFIVec_Reaction {
//         if ctx.is_null() {
//             return std::ptr::null_mut();
//         }
//         let adapter = &*ctx;
//         Box::into_raw(Box::new(adapter.reactions().into()))
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn message_replied(ctx: *const Message) -> *mut c_char {
//         if ctx.is_null() {
//             return std::ptr::null_mut();
//         }
//         let adapter = &*ctx;
//         match adapter.replied() {
//             Some(id) => match CString::new(id.to_string()) {
//                 Ok(c) => c.into_raw(),
//                 Err(_) => std::ptr::null_mut(),
//             },
//             None => std::ptr::null_mut(),
//         }
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn message_lines(ctx: *const Message) -> *mut FFIVec_String {
//         if ctx.is_null() {
//             return std::ptr::null_mut();
//         }
//         let adapter = &*ctx;
//         let lines = adapter.lines();

//         Box::into_raw(Box::new(lines.into()))
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn reaction_emoji(ctx: *const Reaction) -> *mut c_char {
//         if ctx.is_null() {
//             return std::ptr::null_mut();
//         }
//         let adapter = &*ctx;
//         match CString::new(adapter.emoji()) {
//             Ok(c) => c.into_raw(),
//             Err(_) => std::ptr::null_mut(),
//         }
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn reaction_users(ctx: *const Reaction) -> *mut FFIVec_DID {
//         if ctx.is_null() {
//             return std::ptr::null_mut();
//         }
//         let adapter = &*ctx;
//         Box::into_raw(Box::new(adapter.users().into()))
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn conversation_id(conversation: *const Conversation) -> *mut c_char {
//         if conversation.is_null() {
//             return std::ptr::null_mut();
//         }

//         let id = Conversation::id(&*conversation);

//         match CString::new(id.to_string()) {
//             Ok(c) => c.into_raw(),
//             Err(_) => std::ptr::null_mut(),
//         }
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn conversation_name(conversation: *const Conversation) -> *mut c_char {
//         if conversation.is_null() {
//             return std::ptr::null_mut();
//         }

//         match Conversation::name(&*conversation) {
//             Some(name) => match CString::new(name) {
//                 Ok(c) => c.into_raw(),
//                 Err(_) => std::ptr::null_mut(),
//             },
//             None => std::ptr::null_mut(),
//         }
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn conversation_type(
//         conversation: *const Conversation,
//     ) -> ConversationType {
//         if conversation.is_null() {
//             return ConversationType::Direct;
//         }

//         Conversation::conversation_type(&*conversation)
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn conversation_recipients(
//         conversation: *const Conversation,
//     ) -> *mut FFIVec_DID {
//         if conversation.is_null() {
//             return std::ptr::null_mut();
//         }

//         Box::into_raw(Box::new(
//             Vec::from_iter(Conversation::recipients(&*conversation)).into(),
//         ))
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn messageoptions_new() -> *mut MessageOptions {
//         Box::into_raw(Box::default())
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn messageoptions_set_range(
//         option: *mut MessageOptions,
//         start: usize,
//         end: usize,
//     ) -> *mut MessageOptions {
//         if option.is_null() {
//             return std::ptr::null_mut();
//         }

//         let opt = Box::from_raw(option);

//         let opt = opt.set_range(start..end);

//         Box::into_raw(Box::new(opt))
//     }

//     // #[allow(clippy::missing_safety_doc)]
//     // #[no_mangle]
//     // pub unsafe extern "C" fn messageoptions_set_date_range(
//     //     option: *mut MessageOptions,
//     //     start: i64,
//     //     end: i64,
//     // ) -> *mut MessageOptions {
//     //     if option.is_null() {
//     //         return std::ptr::null_mut();
//     //     }

//     //     let opt = Box::from_raw(option);

//     //     let start = match convert_timstamp(start) {
//     //         Some(s) => s,
//     //         None => return std::ptr::null_mut(),
//     //     };
//     //     let end = match convert_timstamp(end) {
//     //         Some(s) => s,
//     //         None => return std::ptr::null_mut(),
//     //     };

//     //     let opt = opt.set_date_range(start..end);

//     //     Box::into_raw(Box::new(opt))
//     // }

//     #[allow(clippy::missing_safety_doc)]
//     unsafe fn pointer_to_vec(
//         data: *const *const c_char,
//         len: usize,
//     ) -> Result<Vec<String>, Utf8Error> {
//         std::slice::from_raw_parts(data, len)
//             .iter()
//             .map(|arg| CStr::from_ptr(*arg).to_str().map(ToString::to_string))
//             .collect()
//     }

//     // fn convert_timstamp(timestamp: i64) -> Option<DateTime<Utc>> {
//     //     NaiveDateTime::from_timestamp_opt(timestamp, 0)
//     //         .map(|naive| DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
//     // }
// }
