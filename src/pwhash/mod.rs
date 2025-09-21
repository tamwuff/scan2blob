pub fn crypt(plaintext: &str) -> String {
    let crypt_params: sha_crypt::Sha512Params =
        sha_crypt::Sha512Params::new(5000).expect("sha_crypt");
    sha_crypt::sha512_simple(plaintext, &crypt_params).unwrap()
}

pub fn verify(plaintext: &str, hashed_password: &str) -> bool {
    sha_crypt::sha512_check(plaintext, hashed_password).is_ok()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn hash_verify_round_trip() {
        let plaintext = "Hello, world!";
        let password = crypt(plaintext);
        assert!(verify(plaintext, &password));
    }

    #[test]
    fn hash_verify_incorrect() {
        let plaintext = "Hello, world!";
        let incorrect_plaintext = "scraaaawwwk";
        let password = crypt(plaintext);
        assert!(!verify(incorrect_plaintext, &password));
    }

    #[test]
    fn can_verify_from_standard_mcf() {
        let password = r"$6$xQ0B16KjqnvTXfa/$WEyOdGVoTc2S9qKP7R0iYg3yv9FlLuHFPgZ9eLYgx630/4Rj3sQcNxP4W4rB8XsrI9d9lIHImcSH0237Y7.7e.";
        let plaintext = "Hello, world!";
        assert!(verify(plaintext, password));
    }
}
