//! Device keys, passkey wrapping, recovery shares and encryption to recipients.

use bc_envelope::prelude::*;
use rdx_keys::{
    Error, Keys, Seed, add_recipients, decrypt, encrypt, envelope_from_ur, envelope_to_ur,
    public_keys_from_ur, public_keys_to_ur, unwrap, wrap,
};

fn rng(seed: &str) -> rdx_keys::Drbg {
    rdx_keys::Drbg::new(seed)
}

const PRF: [u8; 32] = [1; 32];

#[test]
fn a_passkey_unwraps_the_device_key_it_wrapped() {
    let keys = Keys::generate(&mut rng("device"));
    let wrapped = wrap(&keys, &PRF, &mut rng("wrap")).unwrap();
    let round = envelope_from_ur(&envelope_to_ur(&wrapped)).unwrap();
    let back = unwrap(&round, &PRF).unwrap();
    assert_eq!(back.public(), keys.public());
}

#[test]
fn another_passkey_does_not_unwrap_it() {
    let keys = Keys::generate(&mut rng("device"));
    let wrapped = wrap(&keys, &PRF, &mut rng("wrap")).unwrap();
    assert!(matches!(
        unwrap(&wrapped, &[2; 32]),
        Err(Error::WrongPasskey)
    ));
    assert!(matches!(
        unwrap(&wrapped, &[1; 16]),
        Err(Error::ShortPrf(16))
    ));
}

#[test]
fn the_wrapped_key_is_ciphertext() {
    let keys = Keys::generate(&mut rng("device"));
    let wrapped = wrap(&keys, &PRF, &mut rng("wrap")).unwrap();
    assert!(wrapped.is_encrypted());
}

#[test]
fn any_two_of_three_shares_recover_the_seed() {
    let seed = Seed::generate(&mut rng("seed"));
    let recovery = seed.recovery_keys().public();
    let shares = seed.split(2, 3, &mut rng("split")).unwrap();
    assert_eq!(shares.len(), 3);
    assert!(shares.iter().all(|s| s.starts_with("ur:sskr/")));
    for pair in [[0, 1], [0, 2], [1, 2]] {
        let picked: Vec<String> = pair.iter().map(|i| shares[*i].clone()).collect();
        let back = Seed::combine(&picked).unwrap();
        assert_eq!(back.recovery_keys().public(), recovery);
    }
}

#[test]
fn one_share_recovers_nothing() {
    let seed = Seed::generate(&mut rng("seed"));
    let shares = seed.split(2, 3, &mut rng("split")).unwrap();
    assert!(matches!(Seed::combine(&shares[..1]), Err(Error::Shares(_))));
    assert!(matches!(
        Seed::combine(&["ur:sskr/nonsense".to_owned()]),
        Err(Error::Ur { .. })
    ));
}

#[test]
fn every_recipient_and_nobody_else_can_decrypt() {
    let a = Keys::generate(&mut rng("a"));
    let b = Keys::generate(&mut rng("b"));
    let c = Keys::generate(&mut rng("c"));
    let doc = Envelope::new("statement").add_assertion("amount", -2500);
    let enc = encrypt(&doc, &[a.public(), b.public()], &mut rng("enc")).unwrap();
    assert!(enc.subject().is_encrypted());
    assert!(!envelope_to_ur(&enc).is_empty());
    assert!(decrypt(&enc, &a).unwrap().is_identical_to(&doc));
    assert!(decrypt(&enc, &b).unwrap().is_identical_to(&doc));
    assert!(matches!(decrypt(&enc, &c), Err(Error::NotARecipient)));
}

#[test]
fn the_same_generator_gives_the_same_ciphertext() {
    // No randomness comes from the platform: the host's generator decides everything.
    let a = Keys::generate(&mut rng("a"));
    let doc = Envelope::new("statement");
    let one = encrypt(&doc, &[a.public()], &mut rng("enc")).unwrap();
    let two = encrypt(&doc, &[a.public()], &mut rng("enc")).unwrap();
    assert_eq!(envelope_to_ur(&one), envelope_to_ur(&two));
}

#[test]
fn a_recipient_can_let_a_new_key_in() {
    let a = Keys::generate(&mut rng("a"));
    let new = Keys::generate(&mut rng("new"));
    let doc = Envelope::new("statement");
    let enc = encrypt(&doc, &[a.public()], &mut rng("enc")).unwrap();
    assert!(decrypt(&enc, &new).is_err());
    let more = add_recipients(&enc, &a, &[new.public()], &mut rng("add")).unwrap();
    assert!(decrypt(&more, &new).unwrap().is_identical_to(&doc));
    assert!(decrypt(&more, &a).unwrap().is_identical_to(&doc));
    // Someone who isn't a recipient can't add anyone.
    assert!(matches!(
        add_recipients(&enc, &new, &[new.public()], &mut rng("add")),
        Err(Error::NotARecipient)
    ));
}

#[test]
fn public_keys_travel_as_urs() {
    let a = Keys::generate(&mut rng("a"));
    let ur = public_keys_to_ur(&a.public());
    assert!(ur.starts_with("ur:crypto-pubkeys/"));
    assert_eq!(public_keys_from_ur(&ur).unwrap(), a.public());
}
