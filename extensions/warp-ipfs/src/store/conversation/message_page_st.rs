use crate::store::conversation::reference::MessageReferenceList;
use futures::{Stream, StreamExt};
use rust_ipfs::Ipfs;
use std::collections::BTreeSet;
use std::pin::Pin;
use std::task::{Context, Poll};
use warp::raygun::{Message, MessageOptions, MessagesType};

pub struct MessagePageStream {
    ipfs: Ipfs,
    set: BTreeSet<Message>,
    option: MessageOptions,
    message_reference_list: MessageReferenceList,
    current_index: usize,
    page_index: Option<usize>,
    amount_per_page: usize,
    count: usize,
}

impl MessagePageStream {
    pub fn new(
        ipfs: &Ipfs,
        option: MessageOptions,
        message_reference_list: MessageReferenceList,
        count: usize,
    ) -> Self {
        let (page_index, amount_per_page) = match option.messages_type() {
            MessagesType::Pages {
                page,
                amount_per_page,
            } => (
                page,
                amount_per_page
                    .map(|amount| if amount == 0 { u8::MAX as _ } else { amount })
                    .unwrap_or(u8::MAX as _),
            ),
            _ => (None, u8::MAX as _),
        };

        let list = message_reference_list.list(&ipfs).chunks(amount_per_page);

        let st = MessagePageStream {
            ipfs: ipfs.clone(),
            set: BTreeSet::new(),
            option,
            message_reference_list,
            current_index,
            page_index,
            amount_per_page,
            count,
        };

        unimplemented!()
    }
}

impl Stream for MessagePageStream {
    type Item = Vec<Message>;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Pending
    }
}
