// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The Android Keystore as EDM's store for secrets (gaps row N11), reached through JNI: an
//! AES-256-GCM key, made once under [`ALIAS`] in `AndroidKeyStore` and never leaving it, seals
//! each secret, which is kept in a file of its own under EDM's home (`crate::sealed`). The
//! Keystore chooses each nonce; the secret's name is bound to it as additional data. Whatever
//! Android throws is said in EDM's words (`jvm::with`); nothing here panics.

use std::path::Path;
use std::sync::Mutex;

use jni::objects::{JByteArray, JObject};
use jni::{jni_sig, jni_str, Env, JValue};

use super::jvm;
use crate::sealed::{FileStore, Sealer};

/// The name EDM's key is kept under in the Keystore.
const ALIAS: &str = "edm-secrets";
/// The Keystore's provider.
const PROVIDER: &str = "AndroidKeyStore";
/// AES in GCM, as the Keystore does it.
const TRANSFORMATION: &str = "AES/GCM/NoPadding";
/// `KeyProperties.PURPOSE_ENCRYPT | PURPOSE_DECRYPT`.
const PURPOSES: i32 = 1 | 2;
/// `Cipher.ENCRYPT_MODE` and `Cipher.DECRYPT_MODE`.
const ENCRYPT_MODE: i32 = 1;
const DECRYPT_MODE: i32 = 2;
/// The GCM tag, in bits.
const TAG_BITS: i32 = 128;

/// The Keystore's sealer. Making the key is done under a lock, so two first secrets kept at
/// once do not each make one, the second replacing the key the first was sealed with.
pub(super) struct Keystore {
    making: Mutex<()>,
}

/// EDM's store for secrets in `home`, sealed by the Keystore.
pub(super) fn store(home: &Path) -> FileStore<Keystore> {
    FileStore::new(
        home,
        Keystore {
            making: Mutex::new(()),
        },
    )
}

impl Sealer for Keystore {
    fn seal(&self, name: &str, plain: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
        let _making = edm_core::lock(&self.making);
        jvm::with("seal its secret in the Android Keystore", |env, _| {
            let key = match key(env)? {
                Some(key) => key,
                None => new_key(env)?,
            };
            let cipher = cipher(env)?;
            env.call_method(
                &cipher,
                jni_str!("init"),
                jni_sig!("(ILjava/security/Key;)V"),
                &[JValue::Int(ENCRYPT_MODE), JValue::Object(&key)],
            )?;
            let sealed = finish(env, &cipher, name, plain)?;
            let iv = env
                .call_method(&cipher, jni_str!("getIV"), jni_sig!("()[B"), &[])?
                .l()?;
            Ok((bytes_of(env, iv)?, sealed))
        })
    }

    fn open(&self, name: &str, iv: &[u8], sealed: &[u8]) -> Result<Vec<u8>, String> {
        let opened = jvm::with("open its secret with the Android Keystore", |env, _| {
            let Some(key) = key(env)? else {
                return Ok(None);
            };
            let iv = env.byte_array_from_slice(iv)?;
            let spec = env.new_object(
                jni_str!("javax/crypto/spec/GCMParameterSpec"),
                jni_sig!("(I[B)V"),
                &[JValue::Int(TAG_BITS), JValue::Object(&iv)],
            )?;
            let cipher = cipher(env)?;
            env.call_method(
                &cipher,
                jni_str!("init"),
                jni_sig!("(ILjava/security/Key;Ljava/security/spec/AlgorithmParameterSpec;)V"),
                &[
                    JValue::Int(DECRYPT_MODE),
                    JValue::Object(&key),
                    JValue::Object(&spec),
                ],
            )?;
            finish(env, &cipher, name, sealed).map(Some)
        })?;
        opened.ok_or_else(|| {
            "the key that seals EDM's secrets is not in the Android Keystore (the app's data was \
             moved or restored)"
                .to_string()
        })
    }
}

/// EDM's key in the Keystore, where it has been made.
fn key<'local>(env: &mut Env<'local>) -> jni::errors::Result<Option<JObject<'local>>> {
    let provider = env.new_string(PROVIDER)?;
    let keystore = env
        .call_static_method(
            jni_str!("java/security/KeyStore"),
            jni_str!("getInstance"),
            jni_sig!("(Ljava/lang/String;)Ljava/security/KeyStore;"),
            &[JValue::Object(&provider)],
        )?
        .l()?;
    env.call_method(
        &keystore,
        jni_str!("load"),
        jni_sig!("(Ljava/security/KeyStore$LoadStoreParameter;)V"),
        &[JValue::Object(&JObject::null())],
    )?;
    let alias = env.new_string(ALIAS)?;
    let key = env
        .call_method(
            &keystore,
            jni_str!("getKey"),
            jni_sig!("(Ljava/lang/String;[C)Ljava/security/Key;"),
            &[JValue::Object(&alias), JValue::Object(&JObject::null())],
        )?
        .l()?;
    Ok((!key.is_null()).then_some(key))
}

/// A new AES-256 key for GCM without padding, for sealing and opening, made in the Keystore
/// under [`ALIAS`].
fn new_key<'local>(env: &mut Env<'local>) -> jni::errors::Result<JObject<'local>> {
    let alias = env.new_string(ALIAS)?;
    let builder = env.new_object(
        jni_str!("android/security/keystore/KeyGenParameterSpec$Builder"),
        jni_sig!("(Ljava/lang/String;I)V"),
        &[JValue::Object(&alias), JValue::Int(PURPOSES)],
    )?;
    let gcm = env.new_string("GCM")?;
    let modes = env.new_object_array(1, jni_str!("java/lang/String"), &gcm)?;
    env.call_method(
        &builder,
        jni_str!("setBlockModes"),
        jni_sig!("([Ljava/lang/String;)Landroid/security/keystore/KeyGenParameterSpec$Builder;"),
        &[JValue::Object(&modes)],
    )?;
    let none = env.new_string("NoPadding")?;
    let paddings = env.new_object_array(1, jni_str!("java/lang/String"), &none)?;
    env.call_method(
        &builder,
        jni_str!("setEncryptionPaddings"),
        jni_sig!("([Ljava/lang/String;)Landroid/security/keystore/KeyGenParameterSpec$Builder;"),
        &[JValue::Object(&paddings)],
    )?;
    env.call_method(
        &builder,
        jni_str!("setKeySize"),
        jni_sig!("(I)Landroid/security/keystore/KeyGenParameterSpec$Builder;"),
        &[JValue::Int(256)],
    )?;
    let spec = env
        .call_method(
            &builder,
            jni_str!("build"),
            jni_sig!("()Landroid/security/keystore/KeyGenParameterSpec;"),
            &[],
        )?
        .l()?;
    let algorithm = env.new_string("AES")?;
    let provider = env.new_string(PROVIDER)?;
    let generator = env
        .call_static_method(
            jni_str!("javax/crypto/KeyGenerator"),
            jni_str!("getInstance"),
            jni_sig!("(Ljava/lang/String;Ljava/lang/String;)Ljavax/crypto/KeyGenerator;"),
            &[JValue::Object(&algorithm), JValue::Object(&provider)],
        )?
        .l()?;
    env.call_method(
        &generator,
        jni_str!("init"),
        jni_sig!("(Ljava/security/spec/AlgorithmParameterSpec;)V"),
        &[JValue::Object(&spec)],
    )?;
    let key = env
        .call_method(
            &generator,
            jni_str!("generateKey"),
            jni_sig!("()Ljavax/crypto/SecretKey;"),
            &[],
        )?
        .l()?;
    if key.is_null() {
        return Err(jni::errors::Error::NullPtr("the Keystore's new key"));
    }
    Ok(key)
}

/// `Cipher.getInstance("AES/GCM/NoPadding")`.
fn cipher<'local>(env: &mut Env<'local>) -> jni::errors::Result<JObject<'local>> {
    let transformation = env.new_string(TRANSFORMATION)?;
    env.call_static_method(
        jni_str!("javax/crypto/Cipher"),
        jni_str!("getInstance"),
        jni_sig!("(Ljava/lang/String;)Ljavax/crypto/Cipher;"),
        &[JValue::Object(&transformation)],
    )?
    .l()
}

/// The initialised `cipher` run over `input`, with the secret's `name` as additional data.
fn finish(
    env: &mut Env<'_>,
    cipher: &JObject<'_>,
    name: &str,
    input: &[u8],
) -> jni::errors::Result<Vec<u8>> {
    let aad = env.byte_array_from_slice(name.as_bytes())?;
    env.call_method(
        cipher,
        jni_str!("updateAAD"),
        jni_sig!("([B)V"),
        &[JValue::Object(&aad)],
    )?;
    let input = env.byte_array_from_slice(input)?;
    let output = env
        .call_method(
            cipher,
            jni_str!("doFinal"),
            jni_sig!("([B)[B"),
            &[JValue::Object(&input)],
        )?
        .l()?;
    bytes_of(env, output)
}

/// A Java `byte[]` as Rust's bytes; `null` is an error.
fn bytes_of(env: &mut Env<'_>, obj: JObject<'_>) -> jni::errors::Result<Vec<u8>> {
    if obj.is_null() {
        return Err(jni::errors::Error::NullPtr("the Keystore's bytes"));
    }
    let array = env.cast_local::<JByteArray>(obj)?;
    env.convert_byte_array(&array)
}
