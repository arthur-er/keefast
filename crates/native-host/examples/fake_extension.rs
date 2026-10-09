//! Plays the KeePassXC-Browser extension against a running keefast, through the real native
//! host, as Chromium would: starts the host, exchanges keys, asks for the database hash,
//! associates (approve the Pairing in keefast) and tests the association.
//!
//! `cargo run -p keefast-native-host --example fake_extension -- target/debug/keefast-native-host`

use std::io::{self, Write};
use std::process::{Command, Stdio};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use crypto_box::aead::{Aead, AeadCore, OsRng};
use crypto_box::{PublicKey, SalsaBox, SecretKey};
use keefast_native_host::ipc::{read_frame, write_frame};
use serde_json::{Value, json};

fn main() -> io::Result<()> {
    let host = std::env::args()
        .nth(1)
        .unwrap_or("target/debug/keefast-native-host".into());
    let mut child = Command::new(host)
        .arg("chrome-extension://oboonakemofpalcgghocfoadofidjkkk/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let mut to_host = child.stdin.take().unwrap();
    let mut from_host = child.stdout.take().unwrap();
    let mut call = |request: Value| -> io::Result<Value> {
        println!("→ {request}");
        write_frame(&mut to_host, request.to_string().as_bytes())?;
        let reply = read_frame(&mut from_host)?.expect("a reply");
        let reply: Value = serde_json::from_slice(&reply).unwrap();
        println!("← {reply}");
        io::stdout().flush()?;
        Ok(reply)
    };

    let secret = SecretKey::generate(&mut OsRng);
    let public = BASE64.encode(secret.public_key().as_bytes());
    let id_key = BASE64.encode(SecretKey::generate(&mut OsRng).public_key().as_bytes());
    let nonce = BASE64.encode(SalsaBox::generate_nonce(&mut OsRng));
    let keys = call(json!({
        "action": "change-public-keys", "publicKey": public, "nonce": nonce, "clientID": "fake",
    }))?;
    let Some(host_key) = keys["publicKey"].as_str() else {
        return Ok(());
    };
    let host_key = PublicKey::from(<[u8; 32]>::try_from(BASE64.decode(host_key).unwrap()).unwrap());
    let crypto = SalsaBox::new(&host_key, &secret);

    let mut encrypted = |action: &str, message: Value| -> io::Result<Value> {
        let nonce = SalsaBox::generate_nonce(&mut OsRng);
        let sealed = crypto
            .encrypt(&nonce, message.to_string().as_bytes())
            .unwrap();
        let reply = call(json!({
            "action": action, "message": BASE64.encode(sealed),
            "nonce": BASE64.encode(nonce), "clientID": "fake",
        }))?;
        let (Some(nonce), Some(sealed)) = (reply["nonce"].as_str(), reply["message"].as_str())
        else {
            return Ok(reply);
        };
        let nonce = BASE64.decode(nonce).unwrap();
        let plain = crypto
            .decrypt(nonce.as_slice().into(), &BASE64.decode(sealed).unwrap()[..])
            .expect("the reply decrypts");
        let message: Value = serde_json::from_slice(&plain).unwrap();
        println!("  decrypted: {message}");
        Ok(message)
    };

    encrypted("get-databasehash", json!({ "action": "get-databasehash" }))?;
    println!("Approve the Pairing in keefast…");
    let associated = encrypted(
        "associate",
        json!({ "action": "associate", "key": public, "idKey": id_key }),
    )?;
    if let Some(id) = associated["id"].as_str() {
        encrypted(
            "test-associate",
            json!({ "action": "test-associate", "id": id, "key": id_key }),
        )?;
    }
    drop(to_host);
    child.wait()?;
    Ok(())
}
