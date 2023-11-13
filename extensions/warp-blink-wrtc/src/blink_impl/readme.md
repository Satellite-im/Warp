### BlinkImpl spawns three long running tasks
- `GossipSubListener`: receives messages via IPFS and forwards them to the `BlinkController`
- `GossipSubSender`: contains the user's full DID - both the public and private key - and is responsible for sending and decoding messages. GossipSubSender also can provide a clone of the DID (which returns just the public key) upon request. 
- `BlinkController`: contains the instance of `SimpleWebrtc`. receives all the gossipsub messages, webrtc events, and user commands (invoked by BlinkImpl)

### when BlinkImpl offers a call
- an Offer signal is sent (and retried)
- the sender subscribes to a gossip channel specific to that call
- all recipients subscribe to that gossip channel too, even if they don't join the call (this allows them to detect when all the other participants have left the call - in this case the call would be considered terminated).
- The sender automatically joins the call. 

### when someone joins the call
- they subscribe to another gossip channel, using the call id and their DID. This one is for receiving webrtc specific signals (SDP and ICE mostly). 
- they broadcast an `Announce` signal on the call-wide channel periodically. 
- they track all the `Announce` and `Leave` signals. 
- they periodically go through a list of all participants who joined the call but to whom they aren't yet connected. they compare DIDs and based off of that, one of the peers will initiate a webrtc connection via the `Dial` signal. 

### in response to a dial signal
- the other side automatically accepts and proceeds with the webrtc connection process. 