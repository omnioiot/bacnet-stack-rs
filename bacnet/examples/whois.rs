use bacnet::whois::WhoIs;

fn main() {
    pretty_env_logger::init();
    let devices = WhoIs::new().timeout(std::time::Duration::from_secs(1)).execute().unwrap();

    println!("Got {} devices", devices.len());
    for dev in devices {
        println!("Device ID = {}", dev.device_id);
    }
}
