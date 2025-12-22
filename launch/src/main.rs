use anyhow::Result;
use std::{
    fs::{read, write},
    process::Command,
};

use aes_gcm::{Aes256Gcm, KeyInit, aead::Aead, aes::cipher::Array};
use common::KEY;

const ENC_PATH: &str = "steamclient_loader_x64.encrypted";
const LOADER_PATH: &str = "steamclient_loader_x64.exe";
const USER_CONFIGS: &str = "steam_settings/configs.user.ini";

fn main() {
    let _ = ensure_name();

    let key = Array::try_from(&KEY[..32]).expect("Key is 32 bytes");
    let cipher = Aes256Gcm::new(&key);
    let nonce = Array::try_from([0; 12]).expect("Nonce is 12 bytes");

    let ciphertext = read(ENC_PATH).expect("Missing file: {LOADER_PATH}");
    let file = cipher
        .decrypt(&nonce, &*ciphertext)
        .expect("Decryption failure");
    write(LOADER_PATH, file).expect("Unable to write file: {LOADER_PATH}");

    Command::new("launcher/start_age2.bat").status().unwrap();
}

fn ensure_name() -> Result<()> {
    use ini::Ini;
    let mut conf = Ini::load_from_file(USER_CONFIGS)?;

    let user_settings = conf.with_section(Some("user::general"));
    let username = user_settings.get("account_name");
    if username.is_some_and(|u| !u.trim().is_empty()) {
        return Ok(());
    };

    println!("Enter your desired username:");
    let mut username = String::new();
    std::io::stdin().read_line(&mut username)?;

    conf.with_section(Some("user::general"))
        .set("account_name", username.trim());

    conf.write_to_file(USER_CONFIGS)?;

    Ok(())
}
