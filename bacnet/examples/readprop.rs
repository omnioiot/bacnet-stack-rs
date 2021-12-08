extern crate bacnet;

use bacnet::BACnetDevice;

fn main() {
    let mut dev = BACnetDevice::builder()
        .ip([192, 168, 10, 96].into())
        .dnet(5)
        .dadr(14)
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
