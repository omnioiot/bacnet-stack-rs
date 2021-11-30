use bacnet::whois::WhoIs;

fn main() {
    pretty_env_logger::init();
    let devices = WhoIs::new().timeout(std::time::Duration::from_secs(1)).execute();
}
