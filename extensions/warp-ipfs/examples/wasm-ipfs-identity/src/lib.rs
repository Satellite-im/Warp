use warp::error::Error;
use warp::multipass::identity::IdentityUpdate;
use warp::multipass::MultiPass;
use warp::tesseract::Tesseract;
use warp_ipfs::config::Config;
use warp_ipfs::WarpIpfsBuilder;
use wasm_bindgen::prelude::*;

macro_rules! web_log {
    ( $( $t:tt )* ) => {
        web_sys::console::log_1(&format!( $( $t )* ).into());
    }
}

async fn update_name(account: &mut dyn MultiPass, name: &str) -> Result<(), Error> {
    account
        .update_identity(IdentityUpdate::Username(name.to_string()))
        .await?;
    let ident = account.get_own_identity().await?;
    web_log!("Updated Identity: {}", serde_json::to_string(&ident)?);
    Ok(())
}

async fn update_status(account: &mut dyn MultiPass, status: &str) -> Result<(), Error> {
    account
        .update_identity(IdentityUpdate::StatusMessage(Some(status.to_string())))
        .await?;
    let ident = account.get_own_identity().await?;
    web_log!("Updated Identity: {}", serde_json::to_string(&ident)?);
    Ok(())
}

#[wasm_bindgen]
pub async fn run() -> Result<(), JsError> {
    tracing_wasm::set_as_global_default();
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    let tesseract = Tesseract::default();
    tesseract.unlock(b"super duper pass")?;

    let (mut identity, _, _) = WarpIpfsBuilder::default()
        .set_config(Config::minimal_testing())
        .set_tesseract(tesseract)
        .finalize()
        .await;

    let profile = identity.create_identity(None, None).await?;

    let ident = profile.identity();

    web_log!("Current Identity: {}", serde_json::to_string(&ident)?);

    update_name(&mut *identity, &warp::multipass::generator::generate_name()).await?;
    update_status(&mut *identity, "New status message").await?;
    Ok(())
}
