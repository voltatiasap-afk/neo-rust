use discord_webhook_rs::{Error, Webhook};

pub async fn webhook_send(message: String) {
    let result = tokio::task::spawn_blocking(move || {
        let webhook = Webhook::new(
        "https://discord.com/api/webhooks/1467537563421245481/Sm3hc51-zoAB-hHjmKny0Z7MMedNNDCk_r_RJjBIRo9iNMTIt0rQqqBYR0DqbHiiXEY8",
            ).content(format!("```ansi\n{}\n```", message)).send();
        webhook
    }).await;
}
