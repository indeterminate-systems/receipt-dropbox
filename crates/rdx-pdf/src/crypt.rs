//! The PDF standard security handler (ISO 32000-2 §7.6.4): opening password-protected PDFs.
//!
//! Banks protect statements with a password the account holder knows (in Mexico usually their
//! RFC or date of birth). Revisions 2 to 4 derive the file key with MD5 and encrypt with RC4 or
//! AES-128; revisions 5 and 6 (AES-256) derive it with SHA-2. Either the user password or the
//! owner password opens a file. Nothing here needs randomness, so it builds for both wasm
//! targets; the fixture writer's encryption ([`lock`]) derives its salts from its input.

use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit, generic_array::GenericArray};
use md5::{Digest as _, Md5};
use sha2::{Sha256, Sha384, Sha512};

use crate::PdfError;
use crate::lex::{Dict, Obj};

/// Algorithm 2's padding string, which fills a password to 32 bytes.
const PAD: [u8; 32] = [
    0x28, 0xbf, 0x4e, 0x5e, 0x4e, 0x75, 0x8a, 0x41, 0x64, 0x00, 0x4e, 0x56, 0xff, 0xfa, 0x01, 0x08,
    0x2e, 0x2e, 0x00, 0xb6, 0xd0, 0x68, 0x3e, 0x80, 0x2f, 0x0c, 0xa9, 0xfe, 0x64, 0x53, 0x69, 0x7a,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Method {
    Identity,
    Rc4,
    Aes,
}

/// An opened security handler: the file key and how strings and streams are encrypted.
pub(crate) struct Security {
    key: Vec<u8>,
    revision: i64,
    strings: Method,
    streams: Method,
    pub encrypt_metadata: bool,
}

/// The encryption dictionary's fields that opening needs.
struct Params<'a> {
    revision: i64,
    /// File key length in bytes.
    length: usize,
    o: &'a [u8],
    u: &'a [u8],
    oe: &'a [u8],
    ue: &'a [u8],
    p: i32,
    id: &'a [u8],
    encrypt_metadata: bool,
}

impl Security {
    /// Opens the handler described by `dict` (the trailer's `/Encrypt`) with `password`, the user
    /// or owner password. `id` is the first element of the trailer's `/ID`.
    pub fn open(dict: &Dict, id: &[u8], password: &str) -> Result<Security, PdfError> {
        let name = |k: &[u8]| dict.get(k).and_then(Obj::as_name);
        let int = |k: &[u8]| dict.get(k).and_then(Obj::as_i64);
        let bytes = |k: &[u8]| match dict.get(k) {
            Some(Obj::Str(s)) => s.as_slice(),
            _ => &[],
        };
        let filter = name(b"Filter").unwrap_or_default();
        if filter != b"Standard" {
            return Err(PdfError::Unsupported(format!(
                "encryption handler `{}` (only password protection is supported)",
                String::from_utf8_lossy(filter)
            )));
        }
        let v = int(b"V").unwrap_or(0);
        let revision = int(b"R").unwrap_or(0);
        let (strings, streams, length) = match v {
            1 => (Method::Rc4, Method::Rc4, 5),
            2 | 3 => {
                let bits = int(b"Length").unwrap_or(40).clamp(40, 128);
                (Method::Rc4, Method::Rc4, (bits / 8) as usize)
            }
            4 | 5 => {
                let filters = dict.get(&b"CF"[..]).and_then(Obj::as_dict);
                let method = |key: &[u8]| -> Result<Method, PdfError> {
                    let cf = name(key).unwrap_or(b"Identity");
                    if cf == b"Identity" {
                        return Ok(Method::Identity);
                    }
                    let cfm = filters
                        .and_then(|f| f.get(cf))
                        .and_then(Obj::as_dict)
                        .and_then(|d| d.get(&b"CFM"[..]))
                        .and_then(Obj::as_name)
                        .unwrap_or(b"None");
                    match cfm {
                        b"V2" => Ok(Method::Rc4),
                        b"AESV2" | b"AESV3" => Ok(Method::Aes),
                        b"None" => Ok(Method::Identity),
                        other => Err(PdfError::Unsupported(format!(
                            "crypt filter method `{}`",
                            String::from_utf8_lossy(other)
                        ))),
                    }
                };
                let length = if v == 5 { 32 } else { 16 };
                (method(b"StrF")?, method(b"StmF")?, length)
            }
            other => {
                return Err(PdfError::Unsupported(format!(
                    "encryption algorithm /V {other}"
                )));
            }
        };
        if !(2..=6).contains(&revision) {
            return Err(PdfError::Unsupported(format!(
                "security handler revision /R {revision}"
            )));
        }
        let params = Params {
            revision,
            length,
            o: bytes(b"O"),
            u: bytes(b"U"),
            oe: bytes(b"OE"),
            ue: bytes(b"UE"),
            p: int(b"P").unwrap_or(0) as i32,
            id,
            encrypt_metadata: !matches!(dict.get(&b"EncryptMetadata"[..]), Some(Obj::Bool(false))),
        };
        if params.o.len() < 32 || params.u.len() < 32 {
            return Err(PdfError::Malformed(
                "the encryption dictionary has no valid /O and /U".into(),
            ));
        }
        let open = |password: &str| {
            if revision >= 5 {
                open_aes256(&params, password)
            } else {
                Ok(open_md5(&params, password))
            }
        };
        // A file whose user password is empty opens without one, whatever password is given.
        let key = match open(password)? {
            None if !password.is_empty() => open("")?,
            key => key,
        };
        match key {
            Some(key) => Ok(Security {
                key,
                revision,
                strings,
                streams,
                encrypt_metadata: params.encrypt_metadata,
            }),
            None if password.is_empty() => Err(PdfError::PasswordRequired),
            None => Err(PdfError::WrongPassword),
        }
    }

    pub fn decrypt_string(&self, num: u32, generation: u16, data: &[u8]) -> Vec<u8> {
        self.decrypt(self.strings, num, generation, data)
    }

    pub fn decrypt_stream(&self, num: u32, generation: u16, data: &[u8]) -> Vec<u8> {
        self.decrypt(self.streams, num, generation, data)
    }

    fn decrypt(&self, method: Method, num: u32, generation: u16, data: &[u8]) -> Vec<u8> {
        match method {
            Method::Identity => data.to_vec(),
            Method::Rc4 => rc4(&self.object_key(num, generation, false), data),
            Method::Aes => {
                let key = if self.revision >= 5 {
                    self.key.clone()
                } else {
                    self.object_key(num, generation, true)
                };
                aes_cbc_decrypt(&key, data)
            }
        }
    }

    /// Algorithm 1: revisions 2 to 4 encrypt each object with its own key.
    fn object_key(&self, num: u32, generation: u16, aes: bool) -> Vec<u8> {
        let mut h = Md5::new();
        h.update(&self.key);
        h.update(&num.to_le_bytes()[..3]);
        h.update(generation.to_le_bytes());
        if aes {
            h.update(b"sAlT");
        }
        let digest = h.finalize();
        digest[..(self.key.len() + 5).min(16)].to_vec()
    }
}

/// Revisions 2 to 4 store the password in PDFDocEncoding, which agrees with Latin-1 on the
/// characters bank passwords use (letters, digits, dates).
fn padded(password: &str) -> [u8; 32] {
    let mut out = PAD;
    let bytes: Vec<u8> = password
        .chars()
        .map(|c| u8::try_from(c as u32).unwrap_or(b'?'))
        .take(32)
        .collect();
    out[..bytes.len()].copy_from_slice(&bytes);
    out[bytes.len()..].copy_from_slice(&PAD[..32 - bytes.len()]);
    out
}

/// Tries `password` as the user password, then as the owner password; the file key if either
/// opens the file.
fn open_md5(p: &Params, password: &str) -> Option<Vec<u8>> {
    let user = padded(password);
    if let Some(key) = check_user_md5(p, &user) {
        return Some(key);
    }
    // Algorithm 7: the owner password decrypts /O to the padded user password.
    let mut h: [u8; 16] = Md5::digest(padded(password)).into();
    if p.revision >= 3 {
        for _ in 0..50 {
            h = Md5::digest(h).into();
        }
    }
    let rc4_key = &h[..p.length.min(16)];
    let mut user = p.o[..32].to_vec();
    if p.revision == 2 {
        user = rc4(rc4_key, &user);
    } else {
        for i in (0..20u8).rev() {
            let k: Vec<u8> = rc4_key.iter().map(|b| b ^ i).collect();
            user = rc4(&k, &user);
        }
    }
    check_user_md5(p, &user.try_into().ok()?)
}

/// Algorithms 2, 4 and 5: the file key from a padded user password, if `/U` confirms it.
fn check_user_md5(p: &Params, padded_user: &[u8; 32]) -> Option<Vec<u8>> {
    let n = if p.revision == 2 { 5 } else { p.length };
    let mut h = Md5::new();
    h.update(padded_user);
    h.update(&p.o[..32]);
    h.update(p.p.to_le_bytes());
    h.update(p.id);
    if p.revision >= 4 && !p.encrypt_metadata {
        h.update([0xff; 4]);
    }
    let mut key: [u8; 16] = h.finalize().into();
    if p.revision >= 3 {
        for _ in 0..50 {
            key = Md5::digest(&key[..n]).into();
        }
    }
    let key = key[..n].to_vec();
    let ok = if p.revision == 2 {
        rc4(&key, &PAD) == p.u[..32]
    } else {
        let mut h = Md5::new();
        h.update(PAD);
        h.update(p.id);
        let mut x = rc4(&key, &h.finalize());
        for i in 1..20u8 {
            let k: Vec<u8> = key.iter().map(|b| b ^ i).collect();
            x = rc4(&k, &x);
        }
        x[..16] == p.u[..16]
    };
    ok.then_some(key)
}

/// Algorithms 2.A, 11 and 12: the file key from the user or owner password of an AES-256 file.
fn open_aes256(p: &Params, password: &str) -> Result<Option<Vec<u8>>, PdfError> {
    if p.o.len() < 48 || p.u.len() < 48 || p.oe.len() < 32 || p.ue.len() < 32 {
        return Err(PdfError::Malformed(
            "the encryption dictionary's /O, /U, /OE or /UE is too short for AES-256".into(),
        ));
    }
    // Revision 6 wants the password normalised with SASLprep; that changes nothing for the ASCII
    // passwords banks use, so the UTF-8 bytes are used as they are.
    let pw = &password.as_bytes()[..password.len().min(127)];
    let (u, o) = (&p.u[..48], &p.o[..48]);
    let hash = |salt: &[u8], udata: &[u8]| hash_2b(p.revision, pw, salt, udata);
    let (intermediate, wrapped) = if hash(&u[32..40], &[])[..] == u[..32] {
        (hash(&u[40..48], &[]), &p.ue[..32])
    } else if hash(&o[32..40], u)[..] == o[..32] {
        (hash(&o[40..48], u), &p.oe[..32])
    } else {
        return Ok(None);
    };
    Ok(Some(aes_cbc_raw(&intermediate, &[0; 16], wrapped, false)))
}

/// Algorithm 2.B (revision 6), or a single SHA-256 (revision 5, Adobe's earlier AES-256).
pub(crate) fn hash_2b(revision: i64, password: &[u8], salt: &[u8], udata: &[u8]) -> [u8; 32] {
    let mut k: Vec<u8> = Sha256::new()
        .chain_update(password)
        .chain_update(salt)
        .chain_update(udata)
        .finalize()
        .to_vec();
    if revision < 6 {
        return k.try_into().expect("SHA-256 is 32 bytes");
    }
    let mut round = 0usize;
    loop {
        let mut k1 = Vec::with_capacity(64 * (password.len() + k.len() + udata.len()));
        for _ in 0..64 {
            k1.extend_from_slice(password);
            k1.extend_from_slice(&k);
            k1.extend_from_slice(udata);
        }
        let e = aes_cbc_raw(&k[..16], &k[16..32], &k1, true);
        let modulo = e[..16].iter().map(|b| *b as u32).sum::<u32>() % 3;
        k = match modulo {
            0 => Sha256::digest(&e).to_vec(),
            1 => Sha384::digest(&e).to_vec(),
            _ => Sha512::digest(&e).to_vec(),
        };
        round += 1;
        let last = *e.last().expect("64 repetitions are never empty") as usize;
        if round >= 64 && last + 32 <= round {
            break;
        }
    }
    k[..32].try_into().expect("at least 32 bytes")
}

pub(crate) fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut s: [u8; 256] = std::array::from_fn(|i| i as u8);
    let mut j = 0u8;
    for i in 0..256 {
        j = j.wrapping_add(s[i]).wrapping_add(key[i % key.len()]);
        s.swap(i, j as usize);
    }
    let (mut i, mut j) = (0u8, 0u8);
    data.iter()
        .map(|b| {
            i = i.wrapping_add(1);
            j = j.wrapping_add(s[i as usize]);
            s.swap(i as usize, j as usize);
            b ^ s[s[i as usize].wrapping_add(s[j as usize]) as usize]
        })
        .collect()
}

// Boxed: the AES-256 key schedule is much larger than AES-128's.
enum Aes {
    A128(Box<aes::Aes128>),
    A256(Box<aes::Aes256>),
}

impl Aes {
    fn new(key: &[u8]) -> Aes {
        match key.len() {
            16 => Aes::A128(Box::new(aes::Aes128::new(GenericArray::from_slice(key)))),
            32 => Aes::A256(Box::new(aes::Aes256::new(GenericArray::from_slice(key)))),
            n => unreachable!("AES keys here are 16 or 32 bytes, not {n}"),
        }
    }

    fn encrypt(&self, block: &mut [u8]) {
        let block = GenericArray::from_mut_slice(block);
        match self {
            Aes::A128(c) => c.encrypt_block(block),
            Aes::A256(c) => c.encrypt_block(block),
        }
    }

    fn decrypt(&self, block: &mut [u8]) {
        let block = GenericArray::from_mut_slice(block);
        match self {
            Aes::A128(c) => c.decrypt_block(block),
            Aes::A256(c) => c.decrypt_block(block),
        }
    }
}

/// CBC without padding over whole blocks; a trailing partial block is dropped.
fn aes_cbc_raw(key: &[u8], iv: &[u8], data: &[u8], encrypt: bool) -> Vec<u8> {
    let cipher = Aes::new(key);
    let mut prev: [u8; 16] = iv.try_into().expect("16-byte IV");
    let mut out = Vec::with_capacity(data.len());
    let (blocks, _) = data.as_chunks::<16>();
    for chunk in blocks {
        let mut block = *chunk;
        if encrypt {
            block.iter_mut().zip(prev).for_each(|(b, p)| *b ^= p);
            cipher.encrypt(&mut block);
            prev = block;
        } else {
            cipher.decrypt(&mut block);
            block.iter_mut().zip(prev).for_each(|(b, p)| *b ^= p);
            prev = *chunk;
        }
        out.extend_from_slice(&block);
    }
    out
}

/// A string or stream as AESV2 and AESV3 store it: a 16-byte IV, then CBC with PKCS#7 padding.
/// Padding that doesn't check out is left in place rather than refused, as viewers do.
fn aes_cbc_decrypt(key: &[u8], data: &[u8]) -> Vec<u8> {
    if data.len() < 16 {
        return Vec::new();
    }
    let mut out = aes_cbc_raw(key, &data[..16], &data[16..], false);
    if let Some(&n) = out.last() {
        let n = n as usize;
        if (1..=16).contains(&n)
            && n <= out.len()
            && out[out.len() - n..].iter().all(|b| *b == n as u8)
        {
            out.truncate(out.len() - n);
        }
    }
    out
}

/// Encryption for the fixture writer: AES-256 (revision 6), with every salt, IV and the file key
/// derived from `seed` so the same input gives the same file. Not for real secrecy.
#[cfg(feature = "write")]
pub(crate) mod lock {
    use super::*;

    pub struct Lock {
        pub key: [u8; 32],
        seed: Vec<u8>,
    }

    fn derive(seed: &[u8], label: &str) -> [u8; 32] {
        Sha256::new()
            .chain_update(seed)
            .chain_update(label)
            .finalize()
            .into()
    }

    impl Lock {
        pub fn new(seed: &[u8]) -> Lock {
            Lock {
                key: derive(seed, "file key"),
                seed: seed.to_vec(),
            }
        }

        /// The encryption dictionary for `password` as both user and owner password.
        pub fn dictionary(&self, password: &str) -> String {
            let pw = &password.as_bytes()[..password.len().min(127)];
            let salts = derive(&self.seed, "salts");
            let (uv, uk, ov, ok) = (&salts[0..8], &salts[8..16], &salts[16..24], &salts[24..32]);
            let mut u = hash_2b(6, pw, uv, &[]).to_vec();
            u.extend_from_slice(uv);
            u.extend_from_slice(uk);
            let ue = aes_cbc_raw(&hash_2b(6, pw, uk, &[]), &[0; 16], &self.key, true);
            let mut o = hash_2b(6, pw, ov, &u).to_vec();
            o.extend_from_slice(ov);
            o.extend_from_slice(ok);
            let oe = aes_cbc_raw(&hash_2b(6, pw, ok, &u), &[0; 16], &self.key, true);
            let p: i32 = -4;
            let mut perms = [0u8; 16];
            perms[..4].copy_from_slice(&p.to_le_bytes());
            perms[4..8].copy_from_slice(&[0xff; 4]);
            perms[8..12].copy_from_slice(b"Tadb");
            perms[12..].copy_from_slice(&salts[..4]);
            let perms = aes_cbc_raw(&self.key, &[0; 16], &perms, true);
            format!(
                "<< /Filter /Standard /V 5 /R 6 /Length 256 \
                 /CF << /StdCF << /CFM /AESV3 /AuthEvent /DocOpen /Length 32 >> >> \
                 /StmF /StdCF /StrF /StdCF /O <{}> /U <{}> /OE <{}> /UE <{}> /P {p} /Perms <{}> >>",
                hex(&o),
                hex(&u),
                hex(&oe),
                hex(&ue),
                hex(&perms)
            )
        }

        /// A stream's data as stored: an IV derived from the object number, then AES-256-CBC
        /// with PKCS#7 padding.
        pub fn encrypt(&self, num: usize, data: &[u8]) -> Vec<u8> {
            let iv = &derive(&self.seed, &format!("iv {num}"))[..16];
            let pad = 16 - data.len() % 16;
            let mut padded = data.to_vec();
            padded.extend(std::iter::repeat_n(pad as u8, pad));
            let mut out = iv.to_vec();
            out.extend(aes_cbc_raw(&self.key, iv, &padded, true));
            out
        }

        pub fn id(&self) -> String {
            hex(&derive(&self.seed, "id")[..16])
        }
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rc4_matches_the_classic_test_vector() {
        // "Plaintext" under the key "Key".
        assert_eq!(
            rc4(b"Key", b"Plaintext"),
            [0xbb, 0xf3, 0x16, 0xe8, 0xd9, 0x40, 0xaf, 0x0a, 0xd3]
        );
    }

    #[test]
    fn aes_cbc_round_trips_and_strips_padding() {
        let key = [7u8; 16];
        let iv = [9u8; 16];
        let mut data = b"thirteen byte".to_vec();
        data.extend([3, 3, 3]);
        let mut stored = iv.to_vec();
        stored.extend(aes_cbc_raw(&key, &iv, &data, true));
        assert_eq!(aes_cbc_decrypt(&key, &stored), b"thirteen byte");
    }

    #[test]
    fn passwords_are_padded_to_32_bytes() {
        assert_eq!(padded(""), PAD);
        let p = padded("abc");
        assert_eq!(&p[..3], b"abc");
        assert_eq!(&p[3..], &PAD[..29]);
    }
}
