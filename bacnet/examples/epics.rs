extern crate bacnet;

use bacnet::BACnetDevice;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Opt {
    #[arg(long, default_value_t = 0)]
    device_id: u32,
    #[arg(long, default_value_t = std::net::Ipv4Addr::new(192, 168, 10, 96))]
    ip: std::net::Ipv4Addr,
    #[arg(long, default_value_t = 0)]
    dnet: u16,
    #[arg(long, default_value_t = 0)]
    dadr: u8,
    #[arg(long, default_value_t = 47808)]
    port: u16,
}

fn main() {
    pretty_env_logger::init();
    let opt = Opt::parse();
    let mut dev = BACnetDevice::builder()
        .device_id(opt.device_id)
        .ip(opt.ip)
        .dnet(opt.dnet)
        .dadr(opt.dadr)
        .port(opt.port)
        .build();

    println!("{:?}", dev);
    match dev.connect() {
        Ok(()) => match dev.epics() {
            Ok(epics) => {
                println!("Got epics {:#?}", epics);
            }
            Err(err) => eprintln!("failed to read property: {}", err),
        },
        Err(err) => {
            eprintln!("failed to connect to device... {}", err);
        }
    }
}
