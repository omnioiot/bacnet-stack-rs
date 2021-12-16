/// A full device report, including device vendor and product information along with structured
/// data for every single object instance available on the device.
#[derive(Debug, Default)]
pub struct Epics {
    objects: Vec<Object>,
}

#[derive(Debug, Default)]
pub struct Object {
    name: String,
    instance: usize,
    type_: String, // Actually an enum
    present_value: (), // Option<BACnetValue>,
    unit: String,
}
