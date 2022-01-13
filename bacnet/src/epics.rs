use crate::value::BACnetValue;
use std::collections::HashMap;

/// A full device report, including device vendor and product information along with structured
/// data for every single object instance available on the device.
#[derive(Debug, Default)]
pub struct Epics {
    pub object_name: String,
    pub system_status: Option<()>,
    pub vendor_identifier: u64,
    pub vendor_name: String,
    pub model_name: String,
    pub firmware_revision: String,
    pub application_software_version: String,
    pub protocol_version: u64,
    pub protocol_revision: u64,
    pub objects: Vec<Object>,
}

#[derive(Debug, Default)]
pub struct Object {
    name: String,
    instance: usize,
    type_: String,     // Actually an enum
    present_value: (), // Option<BACnetValue>,
    unit: String,
}

pub struct SimpleEpics {
    pub device: HashMap<String, BACnetValue>,
    pub object_list: Vec<HashMap<String, BACnetValue>>,
}
