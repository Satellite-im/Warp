# Shuttle - Decentralized Offline Service

## Concept

Shuttle is a decentralized, peer-to-peer offline service that securely store and transmit data using LibP2P and IPFS. The system provides a tamper-proof and privacy for peers to store and transmit data offline, and ensures that data is only accessible by the intended parties.

Data transmitted will remains encrypted, with the data between the sender and recipient being encrypted between each other, and the request encrypted between the sender and agent. The data may include an encrypted header used by the agent to record the recipient and any miscellaneous data used to identify the data for the sender or receiver. This will ensures that the privacy of the data is maintained and, that stored data cannot be decrypted by the agent or any unintended recipients but also discoverable by the sender or recipient upon request. All data transmitted will signed to confirm that it has not been tampered with, which allows for trust between all parties, ensuring that data is not altered, modified, or corrupted in transit .

Shuttle requests and responses are encrypted for the intended recipient, which allows for efficient communication and ensures that data is only sent to the intended recipients. To ensure the reliability of the data transmission, Shuttle may use libp2p pubsub or dht to transmit data to other connected agents, and agents may talk to each other to store, confirm, or delete data. These features ensure that the data is always available for the intended recipient, even if the original agent that received the data goes offline. 

To ensure that the agent does not waist resources on data that cannot be transmitted to the intended recipient after a period of time, an agent can allow data to expire after a specific amount of time, which would free up resources. Peers who submit data to the agent, and their recipient, can request the deletion of the data upon a signed request. This will ensure that only the sender or receiver of the data can request its deletion, preventing any unauthorized deletion of data by a third party or a malicious actor unless the data has expired after a specific amount of time. These features enhance the privacy, security, and reliability of the network and its data to ensure the data is only deleted when requested by the intended sender, recipient or agent, ensuring that the network can handle a large number of request from its peers without compromising on performance and stability.


## Motivation
This service will be helpful because within warp (or at least its ipfs components), we do not have a consistent way of storing data for another user before going offline. As such, data is generally queued locally and sent upon being connected to the intended peer, but many other factors would come into play that may not allow for them to receive the data such failing to connect to a relay, relay restrictions (since circuit relay v2 is limited to a specific amount of data since the intended purpose is mostly for DCuTR), direct connection failing, or unable to utilize port forwarding (eg firewall or router restrictions, ISP restrictions, VPN, etc), pubsub not publishing messages to the topic, etc. This service is not restricted to just components in warp, but could expand out to other services for storing and handling of data offchain for an intended recipient. This style can also be a bridge between multiple peers in which that a user does not have to rely on a single service but its peers that it trust to handle such request on its behalf.

## Specification (draft)

### Protocol
- For any nodes operating solely as agents, its recommended to set `Identify` protocol version to `/p2p/shuttle/0.1`
- Nodes operating as clients or direct agents do not need to set this protocol name. 

### DHT (WIP)
- Use of Kademlia is optional, however if it is used, its recommended to provide `/shuttle/primary-agent` as your key to allow discoverability over the network
- Add or allow the prefix `/shuttle/` for record keys 

### Storage
- Recommends storing any data into ipfs block store however the following can also be used
  1. Sqlite
  2. LevelDB
  3. RocksDB
  4. Sled (rust-only)
  5. Memory (used for nodes that operating temporarily unless enough memory is can be used without costing stability)

**Note: If ipfs block store could not be used, its recommended to use an embedded database or k/v store to ensure lower cost and higher greater**

### Data size
- A single blob of data cannot exceed 8MB
  - An agent may restrict data even further but cannot restrict it below 2MB
  - A client may restrict data under a given namespace (eg in warp, friend requests or operations should not exceed 10KB, although its not enforced at this time)

### Signature
- Signature should constructed by a SHA256 digest, which is then signed by the keypair 

### Wire format 

**Note: Data should be serialized with a binary serializer such as bincode, protobuf, etc. This would allow utilizing zero-copy more effectively so we can reduce allocation** 

#### General payload format (using rust struct)

```rust
struct Payload<'a> {
    // Sender does have to be present in the payload
    sender: PublicKey, // Should be Ed25519, not to exceed 256 bits
    // Intended recipient of the payload
    recipient: PublicKey, // Should be Ed25519, not to exceed 256 bits
    // Header that contains data used by the sender or receipient to identify the payload on the agent
    metadata: Cow<'a, [u8]>, // Should not exceed 1KB
    // Encrypted message
    data: Cow<'a, [u8]>, // Should not exceed 8MB (within margin)
    // Signature. 
    signature: Cow<'a, [u8]> // Should not exceed 512 bits
}
```

#### Request format
```rust
enum Identifier {
    // Store the data
    Store

    // Replace previously stored data with current data
    Replace,

    // Find/locate data
    Find,

    // Delete stored data
    Delete,
}

struct Request<'a> {
    // ID assigned to request
    id: Uuid,
    // Simple identifier on how to handle the request
    identifier: Identifier,
    // namespace under which the request belongs
    namespace: Cow<'a, [u8]>,
    // optional key for a key/value lookup
    key: Option<Cow<'a, [u8]>>,
    // Can only be used if Identifier is `Replace` or `Store` 
    payload: Option<Payload<'a>>,
    // signature of the request
    signature: Cow<'a, [u8]>
}
```

#### Response format
```rust
enum Status {
    Ok,
    Confirmed,
    Unauthorized,
    NotFound,
    Error
}

struct Response<'a> {
    id: Uuid,
    status: Status,
    data: Option<Cow<'a, [u8]>>,
    signature: Cow<'a, [u8]>,
}
```

### Interaction (mp-ipfs friend-request example, pseudocode request/response)

User A wants to send a request to User B, however user B is offline or unreachable and therefore would send a request to agent S

1. A connects to S
    i. A prepares a `Request` with identifier `Identifier::Store`, namespace `friend-request`, with their request encoded in `Payload`. `Payload` would include sender being A public key, recipient being B public key, metadata `null` (or possibly store some form of identifier like the public keys of both recipients along with a tag. eg "aPublicKey:bPublicKey:request"), data being the encrypted request between A and B. A signs `Payload`.
    ii. A signs `Request` and transmits it to S
2. S receive `Request` from A
    i. Check the size of the data to ensure it does not exceed data limit
    ii. Validates signature against the `Request` and `Payload`
    iii. Store `Payload` into storage under `Request` namespace 
    iv. Index metadata, if any
    v. Respond to A request with `Status::Confirmed`
3. B comes online and connects to S
    i. Sends a signed `Request` with `Identifier::Find` under namespace `friend-request`.
    ii. S requests and validate `Request` and query its store for any data intended for B
    iii. S responds with the payload to B
    iv. B receives `Payload` and sends a `Response` confirming receiving data, allowing S to delete the payload.
    v. Validate and decrypt `Payload` and process the request accordingly
4. If A is online, B and respond directly to A. If A is offline, repeat process above


**NOTE: Shuttle is a WIP project and subject to change.**