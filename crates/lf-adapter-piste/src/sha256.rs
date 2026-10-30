//! SHA-256 (FIPS 180-4) écrit à la main, sans dépendance : empreintes des fixtures.
//!
//! Partagé par les contrôles d'intégrité des fixtures (`integrity`, tests) et par
//! l'enregistrement de nouvelles fixtures (`record`, feature `record`, 04 §13.4 : `sha256_raw`,
//! `sha256_file`). `sha2` est confiné à `lf-adapter-audit` et `xtask` (R6) : l'adapter n'en
//! dépend pas, même sous une feature (D9 du plan 2g).

/// Chiffres hexadécimaux minuscules.
const DIGITS: &[u8; 16] = b"0123456789abcdef";

/// Valeurs initiales du hachage (FIPS 180-4, §5.3.3).
const H0: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

/// Constantes de ronde (FIPS 180-4, §4.2.2).
const K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

/// SHA-256 de `bytes` (FIPS 180-4) : les blocs entiers par `as_chunks`, puis le reste complété
/// (`0x80`, des zéros, la longueur en bits sur huit octets) en un ou deux derniers blocs.
pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut state = H0;
    let (blocks, tail) = bytes.as_chunks::<64>();
    for block in blocks {
        compress(&mut state, block);
    }
    let mut last = tail.to_vec();
    last.push(0x80);
    // Zéros de remplissage : amène la longueur à 56 modulo 64 (`tail` fait moins de 64 octets). Un
    // calcul plutôt qu'une boucle `while`, dont le mutant `% -> +` ne s'arrête jamais.
    let zeros = 119_usize.saturating_sub(tail.len()) % 64;
    last.resize(last.len().saturating_add(zeros), 0);
    let bit_len = u64::try_from(bytes.len()).unwrap_or(u64::MAX) << 3;
    last.extend_from_slice(&bit_len.to_be_bytes());
    for block in last.as_chunks::<64>().0 {
        compress(&mut state, block);
    }
    let mut digest = [0_u8; 32];
    for (out, word) in digest.as_chunks_mut::<4>().0.iter_mut().zip(state) {
        *out = word.to_be_bytes();
    }
    digest
}

/// `Ch(x, y, z)` de FIPS 180-4 (§4.1.2) : `y` là où `x` vaut 1, `z` ailleurs. Les deux termes
/// n'ont aucun bit commun (`x` et `!x`) : `^` et `|` y sont équivalents (voir `.cargo/mutants.toml`).
const fn choose(x: u32, y: u32, z: u32) -> u32 {
    (x & y) ^ (!x & z)
}

/// `Maj(x, y, z)` de FIPS 180-4 (§4.1.2) : le bit majoritaire. Bit à bit, `^` et `|` coïncident
/// (voir `.cargo/mutants.toml`).
const fn majority(x: u32, y: u32, z: u32) -> u32 {
    (x & y) ^ (x & z) ^ (y & z)
}

/// Fonction de compression de FIPS 180-4 (§6.2.2) : mêle un bloc de 64 octets à l'état.
#[expect(
    clippy::indexing_slicing,
    reason = "FIPS 180-4 : `t` parcourt 16..64 sur un tableau de 64 mots ; un quartet indexe 16 chiffres"
)]
#[expect(clippy::many_single_char_names, reason = "notation de FIPS 180-4 : a à h et w")]
#[expect(
    clippy::arithmetic_side_effects,
    reason = "FIPS 180-4 : `t` parcourt 16..64, donc `t - 16` reste dans le tableau de 64 mots"
)]
fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0_u32; 64];
    for (word, chunk) in w.iter_mut().zip(block.as_chunks::<4>().0) {
        *word = u32::from_be_bytes(*chunk);
    }
    for t in 16..64 {
        let s0 = w[t - 15].rotate_right(7) ^ w[t - 15].rotate_right(18) ^ (w[t - 15] >> 3);
        let s1 = w[t - 2].rotate_right(17) ^ w[t - 2].rotate_right(19) ^ (w[t - 2] >> 10);
        w[t] = w[t - 16].wrapping_add(s0).wrapping_add(w[t - 7]).wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for (k, word) in K.iter().zip(w) {
        let big_s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = choose(e, f, g);
        let t1 = h.wrapping_add(big_s1).wrapping_add(ch).wrapping_add(*k).wrapping_add(word);
        let big_s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = majority(a, b, c);
        let t2 = big_s0.wrapping_add(maj);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    for (slot, mixed) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *slot = slot.wrapping_add(mixed);
    }
}

/// Chiffres hexadécimaux minuscules de `digest`, deux par octet.
#[expect(
    clippy::indexing_slicing,
    reason = "FIPS 180-4 : `t` parcourt 16..64 sur un tableau de 64 mots ; un quartet indexe 16 chiffres"
)]
pub(crate) fn hex(digest: &[u8]) -> String {
    let mut out = String::with_capacity(digest.len().saturating_mul(2));
    for byte in digest {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_the_fips_180_vectors() {
        let million_a = "a".repeat(1_000_000);
        let vectors: [(&str, &str); 4] = [
            ("", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
            ("abc", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
            (
                "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
                "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
            ),
            (&million_a, "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"),
        ];
        for (message, expected) in vectors {
            assert_eq!(hex(&sha256(message.as_bytes())), expected, "digest of {expected}");
        }
    }

    #[test]
    fn sha256_is_right_at_the_padding_boundaries() {
        // Longueurs qui encadrent le passage du remplissage d'un à deux blocs : 55 octets tiennent
        // avec leur fin de message dans un bloc, 56 et 63 en réclament deux, 64 remplit un bloc
        // entier puis un second de remplissage. Condensats vérifiés avec `sha256sum`.
        let vectors = [
            (55, "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318"),
            (56, "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a"),
            (63, "7d3e74a05d7db15bce4ad9ec0658ea98e3f06eeecf16b4c6fff2da457ddc2f34"),
            (64, "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb"),
        ];
        for (length, expected) in vectors {
            assert_eq!(hex(&sha256(&vec![b'a'; length])), expected, "{length} bytes");
        }
    }

    #[test]
    fn hex_writes_two_lowercase_digits_per_byte() {
        assert_eq!(hex(&[0x00, 0x0f, 0xa5, 0xff]), "000fa5ff");
        assert_eq!(hex(&[]), "");
    }
}
