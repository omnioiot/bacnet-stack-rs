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

    #[arg(short = 't', long, default_value_t = 2, value_parser = parse_object_type)]
    object_type: bacnet_sys::BACNET_OBJECT_TYPE,
    #[arg(short = 'i', long, default_value_t = 22)]
    object_instance: u32,
    #[arg(short = 'p', long, default_value_t = 85, value_parser = parse_property)]
    property: u32,
    #[arg(short = 'I', long, default_value_t = 4294967295)]
    index: u32,

    #[arg(short = 'n', long, default_value_t = 1)]
    number_of_reads: usize,
}

fn parse_object_type(src: &str) -> Result<bacnet_sys::BACNET_OBJECT_TYPE, String> {
    if let Ok(t) = src.parse() {
        Ok(t)
    } else {
        let mut found_index = 0;
        if unsafe {
            bacnet_sys::bactext_object_type_strtol(
                src.as_ptr() as *const ::std::os::raw::c_char,
                &mut found_index,
            )
        } {
            Ok(found_index)
        } else {
            Err(format!("Couldn't parse input '{}' as object-type", src))
        }
    }
}

fn parse_property(src: &str) -> Result<bacnet_sys::BACNET_PROPERTY_ID, String> {
    if let Ok(t) = src.parse() {
        Ok(t)
    } else {
        let mut found_index = 0;
        if unsafe {
            bacnet_sys::bactext_property_strtol(
                src.as_ptr() as *const ::std::os::raw::c_char,
                &mut found_index,
            )
        } {
            Ok(found_index)
        } else {
            Err(format!("Couldn't parse input '{}' as property", src))
        }
    }
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
        Ok(()) => {
            for _ in 0..opt.number_of_reads {
                let r = dev.read_prop_at(
                    opt.object_type,
                    opt.object_instance,
                    opt.property,
                    opt.index,
                );
                match r {
                    Ok(_) => println!("result {:?}", r),
                    Err(err) => eprintln!("failed to read property: {}", err),
                }
            }
        }
        Err(err) => {
            eprintln!("failed to connect to device... {}", err);
        }
    }
}
