use proofs::PARAMS;
use zprimitives::PARAMS as ZPARAMS;
use crate::ss58::EncryptionKeyBytes;
use keys;
use primitives::crypto::Ss58Codec;
use zpairing::{bls12_381::Bls12 as zBls12, PrimeField as zPrimeField, PrimeFieldRepr as zPrimeFieldRepr, io};
use pairing::bls12_381::Bls12;
use zjubjub::{
    curve::{fs::Fs as zFs, FixedGenerators as zFixedGenerators}
};
use proofs::keys::EncryptionKey;
use keys::EncryptionKey as zEncryptionKey;
use zprimitives::PkdAddress;
use zcrypto::elgamal as zelgamal;
use polkadot_rs::{Api, hexstr_to_vec};
use parity_codec::Encode;
use rand::{OsRng, Rng};
use hex;
use bip39::{Mnemonic, Language};
use substrate_bip39::mini_secret_from_entropy;

pub struct PrintKeys {
    pub phrase: Option<String>,
    pub seed: [u8; 32],
    pub decryption_key: [u8; 32],
    pub encryption_key: [u8; 32],
    pub ss58_encryption_key: String,
}

impl PrintKeys {
    pub fn generate() -> Self {
        let rng = &mut OsRng::new().expect("should be able to construct RNG");
        let seed: [u8; 32] = rng.gen();
        gen_from_seed(seed, None).unwrap()
    }

    pub fn generate_from_seed(seed: [u8; 32]) -> Self {
        gen_from_seed(seed, None).unwrap()
    }

    pub fn print_from_phrase(phrase: &str, password: Option<&str>, lang: Language) {
        let seed = phrase_to_seed(phrase, password, lang);
        let print_keys = gen_from_seed(seed, Some(phrase)).unwrap();

        println!("Phrase `{}` is account:\n Seed: 0x{}\n Decryption key: 0x{}\n Encryption key (hex): 0x{}\n Address (SS58): {}",
            phrase,
            hex::encode(&print_keys.seed[..]),
            hex::encode(&print_keys.decryption_key[..]),
            hex::encode(&print_keys.encryption_key[..]),
            print_keys.ss58_encryption_key,
        );
    }
}

pub fn phrase_to_seed(phrase: &str, password: Option<&str>, lang: Language) -> [u8; 32] {
    mini_secret_from_entropy(
        Mnemonic::from_phrase(phrase, lang)
            .unwrap_or_else(|_|
                panic!("Phrase is not a valid BIP-39 phrase: \n {}", phrase)
            ).entropy(),
        password.unwrap_or("")
    )
    .expect("32 bytes can always build a key; qed")
    .to_bytes()
}

fn gen_from_seed(seed: [u8; 32], phrase: Option<&str>) -> io::Result<PrintKeys> {
    let pgk = keys::ProofGenerationKey::<zBls12>::from_seed(&seed[..], &ZPARAMS);
    let decryption_key = pgk.into_decryption_key()?;

    let mut dk_buf = [0u8; 32];
    decryption_key.0.into_repr().write_le(&mut &mut dk_buf[..])?;

    let encryption_key = pgk.into_encryption_key(&ZPARAMS)?;

    let mut ek_buf = [0u8; 32];
    encryption_key.write(&mut ek_buf[..])?;
    // .expect("fails to write payment address");

    let ek_ss58 = EncryptionKeyBytes(ek_buf).to_ss58check();

    // let phrase = match phrase {
    //     Some(p) => p,
    //     None => None,
    // }

    Ok(PrintKeys {
        phrase: phrase.map(|e| e.to_string()),
        seed: seed,
        decryption_key: dk_buf,
        encryption_key: ek_buf,
        ss58_encryption_key: ek_ss58,
    })
}

pub fn seed_to_array(seed: &str) -> [u8; 32] {
    let vec = hex::decode(seed).unwrap();
    let mut array = [0u8; 32];
    let slice = &vec[..array.len()];
    array.copy_from_slice(slice);

    array
}

pub struct BalanceQuery {
    pub decrypted_balance: u32,
    pub encrypted_balance: Vec<u8>,
    pub pending_transfer: Vec<u8>,
    pub encrypted_balance_str: String,
    pub pending_transfer_str: String,
}

// Temporary code.
impl BalanceQuery {
    /// Get encrypted and decrypted balance for the decryption key
    pub fn get_balance_from_decryption_key(mut decryption_key: &[u8], api: Api) -> Self {
        let p_g = zFixedGenerators::Diversifier; // 1

        let mut decryption_key_repr = zFs::default().into_repr();
        decryption_key_repr.read_le(&mut decryption_key).unwrap();
        let decryption_key_fs = zFs::from_repr(decryption_key_repr).unwrap();
        let decryption_key = keys::DecryptionKey(decryption_key_fs);

        let encryption_key = zEncryptionKey::from_decryption_key(&decryption_key, &*ZPARAMS);
        let account_id = PkdAddress::from_encryption_key(&encryption_key);

        let mut encrypted_balance_str = api.get_storage(
            "ConfTransfer",
            "EncryptedBalance",
            Some(account_id.encode())
            ).unwrap();

        let mut pending_transfer_str = api.get_storage(
            "ConfTransfer",
            "PendingTransfer",
            Some(account_id.encode())
        ).unwrap();

        let encrypted_balance;
        let decrypted_balance;
        let pending_transfer;
        let p_decrypted_balance;

        // TODO: redundant code
        if encrypted_balance_str.as_str() != "0x00" {
            // TODO: remove unnecessary prefix. If it returns `0x00`, it will be panic.
            for _ in 0..4 {
                encrypted_balance_str.remove(2);
            }

            encrypted_balance = hexstr_to_vec(encrypted_balance_str.clone());
            let ciphertext = zelgamal::Ciphertext::<zBls12>::read(&mut &encrypted_balance[..], &ZPARAMS).expect("Invalid data");
            decrypted_balance = ciphertext.decrypt(&decryption_key, p_g, &ZPARAMS).unwrap();
        } else {
            encrypted_balance = vec![0u8];
            decrypted_balance = 0;
        }

        if pending_transfer_str.as_str() != "0x00" {
            // TODO: remove unnecessary prefix. If it returns `0x00`, it will be panic.
            for _ in 0..4 {
                pending_transfer_str.remove(2);
            }

            pending_transfer = hexstr_to_vec(pending_transfer_str.clone());
            let p_ciphertext = zelgamal::Ciphertext::<zBls12>::read(&mut &pending_transfer[..], &ZPARAMS).expect("Invalid data");
            p_decrypted_balance = p_ciphertext.decrypt(&decryption_key, p_g, &ZPARAMS).unwrap();
        } else {
            pending_transfer = vec![0u8];
            p_decrypted_balance = 0;
        }

        BalanceQuery {
            decrypted_balance: decrypted_balance + p_decrypted_balance,
            encrypted_balance,
            pending_transfer,
            encrypted_balance_str,
            pending_transfer_str,
        }
    }
}



pub fn get_address(seed: &[u8]) -> std::io::Result<Vec<u8>> {
    let address = EncryptionKey::<Bls12>::from_seed(seed, &PARAMS)?;

    let mut address_bytes = vec![];
    address.write(&mut address_bytes)?;

    Ok(address_bytes)
}
