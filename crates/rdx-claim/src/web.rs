//! The page in the browser: reads the link's fragment, talks to the Worker, asks for a passkey,
//! and shows the result. Everything secret stays in this tab: the fragment is removed from the
//! address bar as soon as it is read, and only signed requests and ciphertext are sent.

use std::rc::Rc;

use js_sys::{Array, Function, Object, Promise, Reflect, Uint8Array};
use rdx_intake::claim::Opened;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{Document, HtmlElement, Window};

use crate::Claim;

const OPEN: &str = "/claim/open";
const RELEASE: &str = "/claim/release";

type Result<T> = std::result::Result<T, String>;

fn window() -> Window {
    web_sys::window().expect("the claim page runs in a browser window")
}

fn document() -> Document {
    window().document().expect("a window has a document")
}

fn element(id: &str) -> HtmlElement {
    document()
        .get_element_by_id(id)
        .unwrap_or_else(|| panic!("claim.html has no #{id}"))
        .unchecked_into()
}

fn say(text: &str) {
    element("status").set_text_content(Some(text));
}

fn js_error(e: JsValue) -> String {
    e.dyn_ref::<js_sys::Error>()
        .map(|e| String::from(e.message()))
        .or_else(|| e.as_string())
        .unwrap_or_else(|| format!("{e:?}"))
}

fn now() -> u64 {
    js_sys::Date::now() as u64
}

/// The browser's CSPRNG (`crypto.getRandomValues`).
struct PageRandom;

impl rdx_host::Random for PageRandom {
    fn fill(&mut self, buf: &mut [u8]) -> rdx_host::Result<()> {
        let crypto = window()
            .crypto()
            .map_err(|_| rdx_host::CapabilityError::new("this browser has no crypto"))?;
        // getRandomValues fills at most 65536 bytes per call.
        for chunk in buf.chunks_mut(65_536) {
            crypto
                .get_random_values_with_u8_array(chunk)
                .map_err(|_| rdx_host::CapabilityError::new("crypto.getRandomValues failed"))?;
        }
        Ok(())
    }
}

fn random_bytes(n: usize) -> Result<Uint8Array> {
    let mut buf = vec![0u8; n];
    rdx_host::Random::fill(&mut PageRandom, &mut buf).map_err(|e| e.to_string())?;
    Ok(Uint8Array::from(buf.as_slice()))
}

fn object(pairs: &[(&str, JsValue)]) -> Object {
    let o = Object::new();
    for (k, v) in pairs {
        Reflect::set(&o, &(*k).into(), v).expect("setting a field on a fresh object");
    }
    o
}

fn get(target: &JsValue, key: &str) -> JsValue {
    Reflect::get(target, &key.into()).unwrap_or(JsValue::UNDEFINED)
}

#[wasm_bindgen(start)]
pub fn start() {
    let w = window();
    let hash = w.location().hash().unwrap_or_default();
    // Keep the key out of the address bar, history and anything that reads them.
    if let (Ok(history), Ok(path)) = (w.history(), w.location().pathname()) {
        let _ = history.replace_state_with_url(&JsValue::NULL, "", Some(&path));
    }
    let claim = match Claim::parse(&hash) {
        Ok(c) => Rc::new(c),
        Err(e) => {
            say(&format!(
                "{e}. Open the link from the \"Verify your address\" email again."
            ));
            return;
        }
    };
    element("address").set_text_content(Some(&claim.address));
    element("start").set_hidden(false);
    say("");

    let button = element("go");
    let on_click = Closure::<dyn FnMut()>::new(move || {
        let claim = claim.clone();
        let button = element("go");
        button.set_attribute("disabled", "").ok();
        spawn_local(async move {
            match run(&claim).await {
                Ok(()) => element("start").set_hidden(true),
                Err(e) => {
                    say(&e);
                    button.remove_attribute("disabled").ok();
                }
            }
        });
    });
    button.set_onclick(Some(on_click.as_ref().unchecked_ref()));
    on_click.forget();
}

async fn run(claim: &Claim) -> Result<()> {
    say("Checking your link…");
    let body = serde_json::json!({ "request": claim.open_request(now()) });
    let opened: Opened = serde_json::from_str(&post(OPEN, &body.to_string()).await?)
        .map_err(|e| format!("the server's answer is unreadable: {e}"))?;

    say("Create a passkey for this address when your browser asks.");
    let prf = passkey(&claim.address).await?;

    say("Moving your mail to your new keys…");
    let finished = claim
        .finish(&opened, prf, PageRandom, now())
        .map_err(|e| e.to_string())?;
    let body = serde_json::to_string(&finished.release).expect("requests serialize");
    post(RELEASE, &body).await?;

    // Ciphertext only: the passkey unlocks it. The PWA (piece 14) picks it up from here.
    if let Ok(Some(storage)) = window().local_storage() {
        let record = String::from_utf8_lossy(&finished.device_record);
        let _ = storage.set_item(&format!("rdx/keys/device/{}", claim.address), &record);
    }
    let n = opened.documents.len();
    say(&format!(
        "Done. {n} email{} from {} {} now encrypted to your passkey, and new mail from it will be \
         too.",
        if n == 1 { "" } else { "s" },
        claim.address,
        if n == 1 { "is" } else { "are" },
    ));
    element("share").set_text_content(Some(&finished.share));
    element("done").set_hidden(false);
    Ok(())
}

/// POSTs JSON and returns the answer's text, or the error the server explained.
async fn post(path: &str, body: &str) -> Result<String> {
    let init = web_sys::RequestInit::new();
    init.set_method("POST");
    init.set_body(&JsValue::from_str(body));
    let headers = web_sys::Headers::new().map_err(js_error)?;
    headers
        .set("content-type", "application/json")
        .map_err(js_error)?;
    init.set_headers(&headers);
    let response: web_sys::Response = JsFuture::from(window().fetch_with_str_and_init(path, &init))
        .await
        .map_err(|e| format!("couldn't reach the server: {}", js_error(e)))?
        .unchecked_into();
    let text = JsFuture::from(response.text().map_err(js_error)?)
        .await
        .map_err(js_error)?
        .as_string()
        .unwrap_or_default();
    if response.ok() {
        Ok(text)
    } else {
        let why = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v["error"].as_str().map(str::to_owned))
            .unwrap_or(text);
        Err(why)
    }
}

async fn call(target: &JsValue, method: &str, arg: &JsValue) -> Result<JsValue> {
    let f: Function = get(target, method).unchecked_into();
    let promise: Promise = f.call1(target, arg).map_err(js_error)?.unchecked_into();
    JsFuture::from(promise).await.map_err(js_error)
}

/// The PRF output (`eval.first` = [`rdx_keys::PRF_SALT`]) of a new passkey for `address`. Some
/// authenticators return it when the passkey is made; the others need one sign-in with it.
async fn passkey(address: &str) -> Result<Vec<u8>> {
    let credentials: JsValue = window().navigator().credentials().into();
    let salt: JsValue = Uint8Array::from(rdx_keys::PRF_SALT).into();
    let prf_ext = || {
        object(&[(
            "prf",
            object(&[("eval", object(&[("first", salt.clone())]).into())]).into(),
        )])
    };
    let algorithms = Array::of2(
        &object(&[("type", "public-key".into()), ("alg", (-7).into())]),
        &object(&[("type", "public-key".into()), ("alg", (-257).into())]),
    );
    let public_key = object(&[
        ("challenge", random_bytes(32)?.into()),
        ("rp", object(&[("name", "Receipt Dropbox".into())]).into()),
        (
            "user",
            object(&[
                ("id", random_bytes(16)?.into()),
                ("name", address.into()),
                ("displayName", address.into()),
            ])
            .into(),
        ),
        ("pubKeyCredParams", algorithms.into()),
        (
            "authenticatorSelection",
            object(&[
                ("residentKey", "required".into()),
                ("userVerification", "required".into()),
            ])
            .into(),
        ),
        ("timeout", 120_000.into()),
        ("extensions", prf_ext().into()),
    ]);
    let created = call(
        &credentials,
        "create",
        &object(&[("publicKey", public_key.into())]).into(),
    )
    .await
    .map_err(|e| format!("No passkey was made ({e}). Try again."))?;
    if let Some(prf) = prf_result(&created)? {
        return Ok(prf);
    }
    let enabled = get(&get(&extension_results(&created)?, "prf"), "enabled");
    if enabled != JsValue::TRUE {
        return Err(NO_PRF.into());
    }

    say("Sign in with the passkey you just made, to unlock your keys.");
    let allow = Array::of1(&object(&[
        ("type", "public-key".into()),
        ("id", get(&created, "rawId")),
    ]));
    let request = object(&[
        ("challenge", random_bytes(32)?.into()),
        ("allowCredentials", allow.into()),
        ("userVerification", "required".into()),
        ("timeout", 120_000.into()),
        ("extensions", prf_ext().into()),
    ]);
    let asserted = call(
        &credentials,
        "get",
        &object(&[("publicKey", request.into())]).into(),
    )
    .await
    .map_err(|e| format!("The passkey didn't sign in ({e}). Try again."))?;
    prf_result(&asserted)?.ok_or_else(|| NO_PRF.into())
}

const NO_PRF: &str = "This passkey can't unlock encryption keys (it lacks the WebAuthn PRF \
                      extension). Try a current Chrome, Edge, Safari or Firefox, or another \
                      passkey provider.";

fn extension_results(credential: &JsValue) -> Result<JsValue> {
    let f: Function = get(credential, "getClientExtensionResults").unchecked_into();
    f.call0(credential).map_err(js_error)
}

fn prf_result(credential: &JsValue) -> Result<Option<Vec<u8>>> {
    let first = get(
        &get(&get(&extension_results(credential)?, "prf"), "results"),
        "first",
    );
    if first.is_undefined() || first.is_null() {
        return Ok(None);
    }
    Ok(Some(Uint8Array::new(&first).to_vec()))
}
