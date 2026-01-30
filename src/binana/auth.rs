use otp::{Algorithm, Secret, Totp};

pub fn validate(code: &String) -> bool {
    let secret = std::env::var("TOTP").unwrap();

    let secrets: Vec<&str> = secret.split(", ").collect();

    let mut output: bool = false;

    println!("{:?}", secrets);
    let totps: Vec<Totp> = secrets
        .into_iter()
        .map(|secret_str| {
            Totp::new(
                Algorithm::SHA1,
                "user".into(),
                "user@neo.com".into(),
                6,
                30,
                Secret::from_bytes(secret_str.as_bytes()),
            )
        })
        .collect();

    for totp in &totps {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let code: u32 = code.parse().expect("invalid num");
        if totp.verify(code, timestamp, 1) {
            output = true;
        } else {
            println!("Hi")
        }
        let code = totp.generate_at(timestamp);
        println!("Code: {}", code);
    }

    output
}

fn main() {
    println!("imported auth!")
}
