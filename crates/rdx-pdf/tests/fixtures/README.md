# Password-protected PDF fixtures

Synthetic one-page PDFs (no real data) that [qpdf](https://qpdf.readthedocs.io/) 12.4 encrypted,
so `tests/locked.rs` checks this crate's decryption against another implementation rather than
against its own writer. Each has the user password `XAXX010101000` (the SAT's generic RFC, as a
Mexican bank would use the holder's RFC) and the owner password `owner-1234`.

| File | Security handler | qpdf `--encrypt` arguments |
| :-- | :-- | :-- |
| `rc4-40.pdf` | revision 2, RC4 40-bit | `XAXX010101000 owner-1234 40` |
| `rc4-128.pdf` | revision 3, RC4 128-bit | `XAXX010101000 owner-1234 128 --use-aes=n` |
| `aes-128.pdf` | revision 4, AES-128 (`AESV2`) | `XAXX010101000 owner-1234 128 --use-aes=y` |
| `aes-256.pdf` | revision 6, AES-256 (`AESV3`), in object streams | `XAXX010101000 owner-1234 256 --object-streams=generate` |
| `owner-only.pdf` | revision 6, empty user password | `'' owner-1234 256` |

The unencrypted page draws "Banco Ejemplo", "Estado de cuenta" and one statement line in
Helvetica, with `/Title (Synthetic statement)` in the document information (an encrypted string).
To regenerate one: `qpdf --allow-weak-crypto --static-id --encrypt <arguments> -- plain.pdf out.pdf`.
