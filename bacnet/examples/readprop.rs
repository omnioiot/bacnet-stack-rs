extern crate bacnet;

use bacnet::BACnetDevice;

fn main() {
    let dev = BACnetDevice::builder()
        .ip([192, 168, 10, 96].into())
        .dnet(5)
        .dadr(14)
        .build();

    // dev.connect()?;
    //
    // dev.read_prop(...)?;
    //
    // dev.disconnect()?;
    println!("{:?}", dev);
}
