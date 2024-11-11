use std::time::Duration;

use futures::StreamExt;
use futures_timeout::TimeoutExt;
use warp::crypto::rand::{self, Rng};
use warp::error::Error;
use warp::multipass::identity::{Identifier, Identity};
use warp::multipass::{Friends, LocalIdentity, MultiPass, MultiPassEvent, MultiPassEventKind};
use warp::tesseract::Tesseract;
use warp_ipfs::config::Config;
use warp_ipfs::{WarpIpfsBuilder, WarpIpfsInstance};
use wasm_bindgen::prelude::*;
use web_sys::{Document, HtmlElement};

async fn account(username: Option<&str>) -> Result<WarpIpfsInstance, Error> {
    let tesseract = Tesseract::default();
    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    let config = Config::minimal_testing();
    let mut instance = WarpIpfsBuilder::default()
        .set_tesseract(tesseract)
        .set_config(config)
        .await;

    instance.create_identity(username, None).await?;
    Ok(instance)
}

fn username(ident: &Identity) -> String {
    format!("{}#{}", &ident.username(), &ident.short_id())
}

#[wasm_bindgen]
pub async fn run() -> Result<(), JsError> {
    let mut rng = rand::thread_rng();
    tracing_wasm::set_as_global_default();
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));

    let body = Body::from_current_window()?;

    let mut account_a = account(None).await?;
    let mut subscribe_a = account_a.multipass_subscribe().await?;

    let mut account_b = account(None).await?;
    let mut subscribe_b = account_b.multipass_subscribe().await?;

    let ident_a = account_a.identity().await?;
    let ident_b = account_b.identity().await?;

    body.append_p(&format!(
        "{} with {}",
        username(&ident_a),
        ident_a.did_key()
    ))?;

    body.append_p(&format!(
        "{} with {}",
        username(&ident_b),
        ident_b.did_key()
    ))?;
    body.append_p("")?;

    account_a.send_request(ident_b.did_key()).await?;
    let mut sent = false;
    let mut received = false;
    let mut seen_a = false;
    let mut seen_b = false;
    loop {
        tokio::select! {
            Some(ev) = subscribe_a.next() => {
                match ev {
                    MultiPassEventKind::FriendRequestSent { .. } => sent = true,
                    MultiPassEventKind::IdentityUpdate { did } if did.eq(ident_b.did_key()) => seen_b = true,
                    _ => {}
                }
            }
            Some(ev) = subscribe_b.next() => {
                match ev {
                    MultiPassEventKind::FriendRequestReceived { .. } => received = true,
                    MultiPassEventKind::IdentityUpdate { did } if did.eq(ident_a.did_key()) => seen_a = true,
                    _ => {}
                }
            }
        }

        if sent && received && seen_a && seen_b {
            break;
        }
    }

    body.append_p(&format!("{} Outgoing request:", username(&ident_a)))?;

    for outgoing in account_a.list_outgoing_request().await? {
        let identity = outgoing.identity();
        let ident = account_a.get_identity(Identifier::from(identity)).await?;
        body.append_p(&format!("To: {}", username(&ident)))?;
        body.append_p("")?;
    }

    body.append_p(&format!("{} Incoming request:", username(&ident_b)))?;
    for incoming in account_b.list_incoming_request().await? {
        let identity = incoming.identity();
        let ident = account_b.get_identity(Identifier::from(identity)).await?;

        body.append_p(&format!("From: {}", username(&ident)))?;
        body.append_p("")?;
    }

    let coin = rng.gen_range(0..2);
    match coin {
        0 => {
            body.append_p(&format!("Denying {} friend request", username(&ident_a)))?;
            account_b.deny_request(ident_a.did_key()).await?;
        }
        _ => {
            account_b.accept_request(ident_a.did_key()).await?;

            body.append_p(&format!(
                "{} accepted {} request",
                ident_b.username(),
                ident_a.username()
            ))?;

            delay().await;

            body.append_p(&format!("{} Friends:", username(&ident_a)))?;
            for friend in account_a.list_friends().await? {
                let friend = account_a.get_identity(Identifier::did_key(friend)).await?;
                body.append_p(&format!("Username: {}", username(&friend)))?;
                body.append_p(&format!("Public Key: {}", friend.did_key()))?;
                body.append_p("")?;
            }

            body.append_p(&format!("{} Friends:", username(&ident_b)))?;
            for friend in account_b.list_friends().await? {
                let friend = account_b.get_identity(Identifier::did_key(friend)).await?;
                body.append_p(&format!("Username: {}", username(&friend)))?;
                body.append_p(&format!("Public Key: {}", friend.did_key()))?;
                body.append_p("")?;
            }

            if rand::random() {
                account_a.remove_friend(ident_b.did_key()).await?;
                if account_a.has_friend(ident_b.did_key()).await? {
                    body.append_p(&format!(
                        "{} is stuck with {} forever",
                        username(&ident_a),
                        username(&ident_b)
                    ))?;
                } else {
                    body.append_p(&format!(
                        "{} removed {}",
                        username(&ident_a),
                        username(&ident_b)
                    ))?;
                }
            } else {
                account_b.remove_friend(ident_a.did_key()).await?;
                if account_b.has_friend(ident_a.did_key()).await? {
                    body.append_p(&format!(
                        "{} is stuck with {} forever",
                        username(&ident_b),
                        username(&ident_a)
                    ))?;
                } else {
                    body.append_p(&format!(
                        "{} removed {}",
                        username(&ident_b),
                        username(&ident_a)
                    ))?;
                }
            }
        }
    }

    body.append_p("")?;

    Ok(())
}

//Note: Because of the internal nature of this extension and not reliant on a central confirmation, this will be used to add delays to allow the separate
//      background task to complete its action
async fn delay() {
    fixed_delay(1500).await;
}

async fn fixed_delay(millis: u64) {
    _ = futures::future::pending::<()>()
        .timeout(Duration::from_millis(millis))
        .await
}

struct Body {
    body: HtmlElement,
    document: Document,
}

impl Body {
    fn from_current_window() -> Result<Self, JsError> {
        // Use `web_sys`'s global `window` function to get a handle on the global
        // window object.
        let document = web_sys::window()
            .ok_or(js_error("no global `window` exists"))?
            .document()
            .ok_or(js_error("should have a document on window"))?;
        let body = document
            .body()
            .ok_or(js_error("document should have a body"))?;

        Ok(Self { body, document })
    }

    fn append_p(&self, msg: &str) -> Result<(), JsError> {
        let val = self
            .document
            .create_element("p")
            .map_err(|_| js_error("failed to create <p>"))?;
        val.set_text_content(Some(msg));
        self.body
            .append_child(&val)
            .map_err(|_| js_error("failed to append <p>"))?;

        Ok(())
    }
}

fn js_error(msg: &str) -> JsError {
    std::io::Error::new(std::io::ErrorKind::Other, msg).into()
}
