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

// List of properties to fetch for a device
pub(crate) const DEVICE_PROPERTIES: [bacnet_sys::BACNET_PROPERTY_ID; 9] = [
    bacnet_sys::BACNET_PROPERTY_ID_PROP_OBJECT_NAME,
    bacnet_sys::BACNET_PROPERTY_ID_PROP_SYSTEM_STATUS,
    bacnet_sys::BACNET_PROPERTY_ID_PROP_VENDOR_NAME,
    bacnet_sys::BACNET_PROPERTY_ID_PROP_VENDOR_IDENTIFIER,
    bacnet_sys::BACNET_PROPERTY_ID_PROP_MODEL_NAME,
    bacnet_sys::BACNET_PROPERTY_ID_PROP_FIRMWARE_REVISION,
    bacnet_sys::BACNET_PROPERTY_ID_PROP_APPLICATION_SOFTWARE_VERSION,
    bacnet_sys::BACNET_PROPERTY_ID_PROP_PROTOCOL_VERSION,
    bacnet_sys::BACNET_PROPERTY_ID_PROP_PROTOCOL_REVISION,
];

// List of properties to fetch for a profile
pub(crate) const PROFILE_PROPERTIES: [bacnet_sys::BACNET_PROPERTY_ID; 7] = [
    bacnet_sys::BACNET_PROPERTY_ID_PROP_OBJECT_NAME,
    bacnet_sys::BACNET_PROPERTY_ID_PROP_OBJECT_TYPE,
    bacnet_sys::BACNET_PROPERTY_ID_PROP_PRESENT_VALUE,
    bacnet_sys::BACNET_PROPERTY_ID_PROP_STATUS_FLAGS,
    bacnet_sys::BACNET_PROPERTY_ID_PROP_EVENT_STATE,
    bacnet_sys::BACNET_PROPERTY_ID_PROP_OUT_OF_SERVICE,
    bacnet_sys::BACNET_PROPERTY_ID_PROP_UNITS,
];

// Don't forget these:
//
//  {
//    ...
//    priority-array: { Null,Null,Null,Null,Null,Null,Null,Null,Null,Null,Null,Null,Null,Null,Null,Null }
//    relinquish-default: 0.000000
//    current-command-priority:     -- unknown property
//  },
//
//
