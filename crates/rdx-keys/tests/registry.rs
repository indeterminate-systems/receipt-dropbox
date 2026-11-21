//! The signed address-to-keys registry: genesis, add, rotate, revoke, and tampering.

use rdx_keys::registry::{Authority, DeviceStatus, RECOVERY};
use rdx_keys::{Keys, Registry, Seed};

fn rng(seed: &str) -> rdx_keys::Drbg {
    rdx_keys::Drbg::new(seed)
}

const ADDR: &str = "ana@example.org";

struct World {
    recovery: Keys,
    laptop: Keys,
    registry: Registry,
}

fn world() -> World {
    let recovery = Seed::generate(&mut rng("seed")).recovery_keys();
    let laptop = Keys::generate(&mut rng("laptop"));
    let registry =
        Registry::genesis(ADDR, "s-0001", &recovery, "laptop", &laptop.public(), 1).unwrap();
    World {
        recovery,
        laptop,
        registry,
    }
}

#[test]
fn genesis_lists_the_first_device_and_the_recovery_key() {
    let w = world();
    let s = w.registry.state().unwrap();
    assert_eq!(s.supporter, "s-0001");
    assert_eq!(s.recovery, w.recovery.public());
    assert_eq!(s.recipients(), vec![w.laptop.public(), w.recovery.public()]);
}

#[test]
fn an_active_device_adds_another_and_both_are_recipients() {
    let mut w = world();
    let phone = Keys::generate(&mut rng("phone"));
    w.registry
        .add(
            "phone",
            &phone.public(),
            &Authority::Device("laptop", &w.laptop),
            2,
        )
        .unwrap();
    let s = w.registry.state().unwrap();
    assert_eq!(s.recipients().len(), 3);
    assert!(s.recipients().contains(&phone.public()));
}

#[test]
fn ids_are_never_reused() {
    let mut w = world();
    let other = Keys::generate(&mut rng("other"));
    let err = w
        .registry
        .add(
            "laptop",
            &other.public(),
            &Authority::Device("laptop", &w.laptop),
            2,
        )
        .unwrap_err();
    assert!(err.to_string().contains("never reused"), "{err}");
    assert_eq!(w.registry.events.len(), 1, "a refused event is not kept");
}

#[test]
fn rotation_replaces_the_keys_and_only_the_device_or_recovery_may_rotate() {
    let mut w = world();
    let phone = Keys::generate(&mut rng("phone"));
    let by_laptop = Authority::Device("laptop", &w.laptop);
    w.registry
        .add("phone", &phone.public(), &by_laptop, 2)
        .unwrap();
    let new = Keys::generate(&mut rng("laptop-2"));
    assert!(
        w.registry
            .rotate("phone", &new.public(), &by_laptop, 3)
            .is_err()
    );
    w.registry
        .rotate("laptop", &new.public(), &by_laptop, 3)
        .unwrap();
    let s = w.registry.state().unwrap();
    assert_eq!(s.devices["laptop"].keys, new.public());
    assert_eq!(s.devices["laptop"].rotations, 1);
    // The old key no longer signs for the laptop.
    assert!(w.registry.revoke("phone", "lost", &by_laptop, 4).is_err());
    w.registry
        .revoke("phone", "lost", &Authority::Device("laptop", &new), 4)
        .unwrap();
}

#[test]
fn revoked_devices_are_not_recipients_and_cannot_sign() {
    let mut w = world();
    let phone = Keys::generate(&mut rng("phone"));
    w.registry
        .add(
            "phone",
            &phone.public(),
            &Authority::Device("laptop", &w.laptop),
            2,
        )
        .unwrap();
    w.registry
        .revoke("laptop", "lost", &Authority::Recovery(&w.recovery), 3)
        .unwrap();
    let s = w.registry.state().unwrap();
    assert_eq!(
        s.devices["laptop"].status,
        DeviceStatus::Revoked {
            at: 3,
            reason: "lost".into()
        }
    );
    assert_eq!(s.recipients(), vec![phone.public(), w.recovery.public()]);
    let x = Keys::generate(&mut rng("x"));
    assert!(
        w.registry
            .add("x", &x.public(), &Authority::Device("laptop", &w.laptop), 4)
            .is_err()
    );
    assert_eq!(
        s.signer(RECOVERY),
        Some(w.recovery.public().signing_public_key())
    );
}

#[test]
fn a_stranger_cannot_sign_for_a_device() {
    let mut w = world();
    let stranger = Keys::generate(&mut rng("stranger"));
    let err = w
        .registry
        .add(
            "evil",
            &stranger.public(),
            &Authority::Device("laptop", &stranger),
            2,
        )
        .unwrap_err();
    assert!(err.to_string().contains("signature"), "{err}");
}

#[test]
fn a_dropped_or_reordered_event_breaks_the_chain() {
    let mut w = world();
    let by = Authority::Device("laptop", &w.laptop);
    for (i, id) in ["a", "b"].iter().enumerate() {
        let k = Keys::generate(&mut rng(id));
        w.registry.add(id, &k.public(), &by, 2 + i as u64).unwrap();
    }
    let mut dropped = w.registry.clone();
    dropped.events.remove(1);
    assert!(dropped.state().is_err());
    let mut swapped = w.registry.clone();
    swapped.events.swap(1, 2);
    assert!(swapped.state().is_err());
    let mut other = w.registry.clone();
    other.address = "eve@example.org".into();
    assert!(other.state().is_err());
}

#[test]
fn registries_serialize_as_event_urs() {
    let w = world();
    let json = serde_json::to_value(&w.registry).unwrap();
    assert!(
        json["events"][0]
            .as_str()
            .unwrap()
            .starts_with("ur:envelope/")
    );
    let back: Registry = serde_json::from_value(json).unwrap();
    assert_eq!(
        back.state().unwrap().recipients(),
        w.registry.state().unwrap().recipients()
    );
}
