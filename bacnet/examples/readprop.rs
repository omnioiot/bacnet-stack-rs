extern crate bacnet;
extern crate structopt;

use bacnet::BACnetDevice;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
#[structopt(name = "readprop")]
struct Opt {
    #[structopt(long, default_value = "0")]
    device_id: u32,
    #[structopt(long, default_value = "192.168.10.96")]
    ip: std::net::Ipv4Addr,
    #[structopt(long, default_value = "0")]
    dnet: u16,
    #[structopt(long, default_value = "0")]
    dadr: u8,
    #[structopt(long, default_value = "47808")]
    port: u16,
}

fn main() {
    pretty_env_logger::init();
    let opt = Opt::from_args();
    let mut dev = BACnetDevice::builder()
        .device_id(opt.device_id)
        .ip(opt.ip)
        .dnet(opt.dnet)
        .dadr(opt.dadr)
        .port(opt.port)
        .build();

    println!("{:?}", dev);
    match dev.connect() {
        Ok(()) => {
            let r =
                dev.read_prop_present_value(bacnet_sys::BACNET_OBJECT_TYPE_OBJECT_ANALOG_VALUE, 22);
            match r {
                Ok(_) => println!("ok"),
                Err(err) => eprintln!("failed to read property: {}", err),
            }
        }
        Err(err) => {
            eprintln!("failed to connect to device... {}", err);
        }
    }
}
