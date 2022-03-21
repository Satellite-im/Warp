# RayGun Interface

## Overview

The `RayGun` interface is used to send, find, and interact with messages on the system.  It also provides hooks to read messages from specific conversation groups. Each conversation group will be stored inside it's own dimension so that we can easily, and rapidly retrieve messages on the fly. `RayGun` will automatically check for new messages after fetching and if required it will deploy two responses over the hook. The first will contain the cached data, the second will contain any updated data. This allows us to update the UI in a optimistic fashion. 

## Methods

#### Retrieving Messages

RayGun will retrieve messages in two steps, if enabled, the cache will be queried first. Secondly we will check to make sure the message ID we have locally matches with the latest message ID remotely. If they do not match RayGun will return a second set of messages containing the updated list. You can also pass the `smart` flag in so that instead of recieving the entire list again it will return only the updated messages which makes it easier to update in the UI. You can also provide a `date_range` to get messages between two ranges. You can also provide two IDs via the `id_range` to get all the messages between two ids. Lastly you can provide a `limit` to avoid getting too many messages. For paginated results you can also add a `skip` parameter along with the limit.


```rust
#[derive(Clone, PartialEq, Eq)]
pub struct MessageOptions<'a> {
    pub smart: Option<bool>,
    pub date_range: Option<&'a [DateTime<Utc>; 2]>,
    pub id_range: Option<&'a [Uuid; 2]>,
    pub limit: Option<i64>,
    pub skip: Option<i64>,
}

RayGun::get_messages(&self, conversation_id: Uuid, options: MessageOptions, callback: Option<Callback>) -> Result<Vec<Message>>;
```

#### Sending Messages

RayGun, by default, does not send any messages. However, it requires an extension to be used to power storing of messages. This will always be proxied through the `RayGun::send_message` method.

```rust
struct Message {
    // ...
    id: "ABC-123"
}

RayGun::send(&mut self, conversation_id: Uuid, message_id: Option<Uuid>, message: Vec<String>) -> Result<()>;
```


#### Editing Messages

Editing messages also derives its functionality, much like the rest of the methods on this page, you can utilize it by simply sending the same message to the pipeline, RayGun will handle versioning automatically.

```rust
struct Message {
    // ...
    id: "ABC-123"
}

RayGun::send(&mut self, conversation_id: Uuid, message_id: Option<Uuid>, message: Vec<String>) -> Result<()>;
```


#### Deleting Messages

The extent at which messages are scrubbed from existance depends on the extensions implementation, however you only need to call delete on a message.

```rust
RayGun::delete(&mut self, conversation_id: Uuid, message_id: Uuid) -> Result<()>;
```

#### Reacting to a Message

Reacting adds a unicode emoji reaction to a message, reactions from all users are compiled when getting a message and the count, as well as a list of who reacted will be provided on the message.

```rust
enum ReactionState {
    Add,
    Remove,
}

struct Message {
    // ...
    id: "ABC-123"
}

RayGun::react(&mut self, conversation_id: Uuid, message_id: Uuid, state: ReactionState, emoji: Option<String>) -> Result<()>;
```

#### Pinning a Message

Pinning requires you to have the correct role in the conversation group to pin. By default P2P chats and group chats allow anyone to pin. However for community servers you will need the correct permission to pin the message.

```rust
enum PinState {
    Pin,
    Unpin,
}

struct Message {
    // ...
    id: "ABC-123",
}

RayGun::pin(&mut self, conversation_id: Uuid, message_id: Uuid, state: PinState) -> Result<()>;
```

#### Reply

Replying to a message simply takes two message payloads. The first, `reaction` being the message that you're replying with. Second, should be the message that you're replying to.

```rust

struct Message {
    // ...
    id: "ABC-123",
}

RayGun::reply(&mut self, conversation_id: Uuid, message_id: Uuid, message: Vec<String>) -> Result<()>;
```

#### Remove Embeds

Removing embeds simply flags messages in the UI to not auto expand embeds.

```rust
enum EmbedState {
    Enabled,
    Disabled,
}

struct Message {
    // ...
    id: "ABC-123",
}

RayGun::embeds(&mut self, conversation_id: Uuid, message_id: Uuid, state: EmbedState) -> Result<()>;
```