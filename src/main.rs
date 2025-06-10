use imap_client::client::ImapClient;
use imap_client::input::prompt_imap_config;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let config = match prompt_imap_config() {
        Ok(config) => config,
        Err(e) => {
            log::error!("Failed to get configuration: {}", e);
            println!("Failed to get IMAP configuration. Please try again.");
            return Ok(());
        }
    };

    let client = ImapClient::new(config);

    log::info!("Starting IMAP email fetch");
    match client.fetch_all_emails().await {
        Ok(_) => println!("Email fetching completed successfully"),
        Err(e) => {
            log::error!("{}", e);
            println!("Failed to fetch emails. Please try again.");
        }
    }

    Ok(())
}
