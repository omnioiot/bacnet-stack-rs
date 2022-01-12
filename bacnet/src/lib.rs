#![allow(unused_variables, unused_mut)] // XXX Remove
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
#[macro_use]
extern crate failure;

use std::cmp::min;
use std::collections::HashMap;
use std::ffi::CStr;
use std::net::Ipv4Addr;
use std::sync::{Mutex, Once};

use failure::Fallible;

use epics::Epics;
use value::BACnetValue;

mod epics;
pub mod value;
pub mod whois;

static BACNET_STACK_INIT: Once = Once::new();

type RequestInvokeId = u8;
type DeviceId = u32;

// We need a global structure here for collecting "target addresses"
lazy_static! {
    /// Global tracking struct for target addresses. These are devices that we consider ourselves
    /// connected to and communicating with.
    static ref TARGET_ADDRESSES: Mutex<HashMap<DeviceId, TargetDevice>> = Mutex::new(HashMap::new());
}

//// Epics property list
//lazy_static! {
//    static ref PROPERTY_LIST: Mutex<
//}
//
//struct PropertyList {
//    length: u32,
//    index: u32,
//    list: [130; i32],
//}

// Status of a request
enum RequestStatus {
    Ongoing,      // No reply has been received yet
    Done,         // Successfully completed
    Rejected(u8), // Rejected with the given reason code
    Aborted(u8),  // Aborted with given reason code
}

// A structure for tracking
//
// FIXME(tj): This is a really poor hand-off mechanism. When making a request, we set the
// request_invoke_id so the response can be matched properly, then we set the decoded value inside
// an Option and read_prop() fishes it out. This means that read_prop() needs to acquire the mutex
// twice for each data extraction, which seems like a really poor design.
struct TargetDevice {
    addr: bacnet_sys::BACNET_ADDRESS,
    request: Option<(RequestInvokeId, RequestStatus)>, // For tracking on-going an ongoing request
    value: Option<Fallible<BACnetValue>>,              // TODO Build this into the 'request status'
}

// As I understand the BACnet stack, it works by acting as another BACnet device on the network.
//
// This means that there's not really a
//
// To "connect" to a device, we call address_bind_request(device_id, ..) which adds the device (if
// possible) to the internal address cache. [sidenote: The MAX_ADDRESS_CACHE = 255, which I take to
// mean that we can connect to at most 255 devices].

#[derive(Debug)]
pub struct BACnetDevice {
    pub device_id: u32,
    max_apdu: u32,
    addr: bacnet_sys::BACNET_ADDRESS,
}

pub type ObjectType = bacnet_sys::BACNET_OBJECT_TYPE;
pub type ObjectPropertyId = bacnet_sys::BACNET_PROPERTY_ID;

impl BACnetDevice {
    pub fn builder() -> BACnetDeviceBuilder {
        BACnetDeviceBuilder::default()
    }

    pub fn connect(&mut self) -> Fallible<()> {
        BACNET_STACK_INIT.call_once(|| unsafe {
            init_service_handlers();
            bacnet_sys::dlenv_init();
        });
        // Add address
        unsafe {
            bacnet_sys::address_add(self.device_id, bacnet_sys::MAX_APDU, &mut self.addr);
        }
        let mut target_addr = bacnet_sys::BACNET_ADDRESS::default();
        // FIXME(tj): Wait until device is bound, or timeout
        let found = unsafe {
            bacnet_sys::address_bind_request(self.device_id, &mut self.max_apdu, &mut target_addr)
        };
        debug!("found = {}", found);
        if found {
            let mut lock = TARGET_ADDRESSES.lock().unwrap();
            lock.insert(
                self.device_id,
                TargetDevice {
                    addr: target_addr,
                    request: None,
                    value: None,
                },
            );
            Ok(())
        } else {
            Err(format_err!("failed to bind to the device"))
        }
    }

    // Read_Property
    //
    // Only reads the present value (property 85)
    pub fn read_prop_present_value(
        &self,
        object_type: ObjectType,
        object_instance: u32,
    ) -> Fallible<BACnetValue> {
        self.read_prop(
            object_type,
            object_instance,
            bacnet_sys::BACNET_PROPERTY_ID_PROP_PRESENT_VALUE,
        )
    }

    /// Read a property
    ///
    /// We call Send_Read_Property_Request, and wait for a result.
    pub fn read_prop(
        &self,
        object_type: ObjectType,
        object_instance: u32,
        property_id: ObjectPropertyId,
    ) -> Fallible<BACnetValue> {
        self.read_prop_at(
            object_type,
            object_instance,
            property_id,
            bacnet_sys::BACNET_ARRAY_ALL,
        )
    }

    pub fn read_prop_at(
        &self,
        object_type: ObjectType,
        object_instance: u32,
        property_id: ObjectPropertyId,
        index: u32,
    ) -> Fallible<BACnetValue> {
        let init = std::time::Instant::now();
        const TIMEOUT: u32 = 100;
        let request_invoke_id =
            if let Some(h) = TARGET_ADDRESSES.lock().unwrap().get_mut(&self.device_id) {
                let request_invoke_id = unsafe {
                    bacnet_sys::Send_Read_Property_Request(
                        self.device_id,
                        object_type,
                        object_instance,
                        property_id,
                        index,
                    )
                };
                h.request = Some((request_invoke_id, RequestStatus::Ongoing));
                request_invoke_id
            } else {
                bail!("Not connected to device {}", self.device_id)
            };

        let mut src = bacnet_sys::BACNET_ADDRESS::default();
        let mut rx_buf = [0u8; bacnet_sys::MAX_MPDU as usize];
        let start = std::time::Instant::now();
        loop {
            // TODO(tj): Consider pulling the "driving forward the internal state machine" stuff
            // into an inner method here. We need it for EPICS as well.
            let pdu_len = unsafe {
                bacnet_sys::bip_receive(
                    &mut src,
                    &mut rx_buf as *mut _,
                    bacnet_sys::MAX_MPDU as u16,
                    TIMEOUT,
                )
            };
            if pdu_len > 0 {
                unsafe { bacnet_sys::npdu_handler(&mut src, &mut rx_buf as *mut _, pdu_len) }
            }

            // FIXME(tj): Need to do tsm_invoke_id_free() and tsm_invoke_id_failed() in this loop
            // as well.
            if unsafe { bacnet_sys::tsm_invoke_id_free(request_invoke_id) } {
                break;
            }
            if unsafe { bacnet_sys::tsm_invoke_id_failed(request_invoke_id) } {
                bail!("TSM timeout");
            }

            if start.elapsed().as_secs() > 3 {
                // FIXME(tj): A better timeout here...
                bail!("APDU timeout");
            }
        }

        let ret = {
            let mut lock = TARGET_ADDRESSES.lock().unwrap();
            let h = lock.get_mut(&self.device_id).unwrap();
            //h.request_invoke_id = 0;
            h.value
                .take()
                .unwrap_or_else(|| Err(format_err!("No value was extracted")))
        };

        debug!("read_prop() finished in {:?}", init.elapsed());
        ret
    }

    /// Read all required properties for a given object-type and object-instance
    ///
    /// The BACnet stack internally has a list of required properties for a given object-type, and
    /// this function will simply walk over every single one and call `read_prop()` on it.
    pub fn read_properties(
        &self,
        object_type: bacnet_sys::BACNET_OBJECT_TYPE,
        object_instance: u32,
    ) -> HashMap<u32, BACnetValue> {
        let mut special_property_list = bacnet_sys::special_property_list_t::default();

        // Fetch all the properties that are known to be required here.
        unsafe {
            bacnet_sys::property_list_special(object_type, &mut special_property_list);
        }

        let len = min(special_property_list.Required.count, 130);
        let mut ret = HashMap::with_capacity(len as usize);
        for i in 0..len {
            let prop = unsafe { *special_property_list.Required.pList.offset(i as isize) } as u32;

            if log_enabled!(log::Level::Debug) {
                let prop_name = unsafe { CStr::from_ptr(bacnet_sys::bactext_property_name(prop)) }
                    .to_string_lossy()
                    .into_owned();
                debug!("Required property {} ({})", prop_name, prop);
            }
            if prop == bacnet_sys::BACNET_PROPERTY_ID_PROP_OBJECT_LIST {
                // This particular property we will not try to read in one go, instead we'll resort
                // to reading it an item at a time.
                continue;
            }
            match self.read_prop(object_type, object_instance, prop) {
                Ok(v) => {
                    debug!("OK. Got value {:?}", v);
                    ret.insert(prop, v);
                }
                Err(err) => {
                    error!("Failed to get property {}", err);
                }
            }
        }

        ret
    }

    pub fn simple_epics(&self) -> Fallible<Epics> {
        let device_props =
            self.read_properties(bacnet_sys::BACNET_OBJECT_TYPE_OBJECT_DEVICE, self.device_id);

        println!("{:#?}", device_props);
        bail!("Not yet implemented");
    }

    pub fn epics(&self) -> Fallible<Epics> {
        let init = std::time::Instant::now();
        let mut src = bacnet_sys::BACNET_ADDRESS::default();

        // FIXME Set different callback methods to handle incoming data

        // Getting EPICS relies on a different kind of data processing compared to readprop().
        //
        // Initial -> GetHeadingInfo -> GetHeadingResponse -> PrintHeading
        //
        // Next, we try the "Get All Request" falling back to getting objects one at a time.
        //
        // -> GetAllRequest -> GetAllResponse
        //

        let mut rx_buf = [0u8; bacnet_sys::MAX_PDU as usize];
        let mut my_object = bacnet_sys::BACNET_OBJECT_ID::default();
        my_object.type_ = bacnet_sys::BACNET_OBJECT_TYPE_OBJECT_DEVICE;
        my_object.instance = self.device_id;
        let mut rpm_object = bacnet_sys::BACNET_READ_ACCESS_DATA::default();

        // aka StartNextObject(rpm_object, BACNET_OBJECT_ID pNewObject)
        // Error_Detected = false;
        // Property_List_Index = 0;
        // Property_List_Length = 0;
        rpm_object.object_type = my_object.type_;
        rpm_object.object_instance = my_object.instance;

        let device_props = [
            bacnet_sys::BACNET_PROPERTY_ID_PROP_VENDOR_NAME,
            bacnet_sys::BACNET_PROPERTY_ID_PROP_MODEL_NAME,
            bacnet_sys::BACNET_PROPERTY_ID_PROP_MAX_APDU_LENGTH_ACCEPTED,
            bacnet_sys::BACNET_PROPERTY_ID_PROP_PROTOCOL_SERVICES_SUPPORTED,
            bacnet_sys::BACNET_PROPERTY_ID_PROP_PROTOCOL_OBJECT_TYPES_SUPPORTED,
            bacnet_sys::BACNET_PROPERTY_ID_PROP_DESCRIPTION,
        ];
        // Build a linked list of BACNET_PROPERTY_REFERENCE
        let mut list_head = None;
        for prop in IntoIterator::into_iter(device_props).rev() {
            let mut new_entry = bacnet_sys::BACNET_PROPERTY_REFERENCE::default();
            new_entry.propertyIdentifier = prop;
            new_entry.propertyArrayIndex = bacnet_sys::BACNET_ARRAY_ALL;
            if let Some(list_head) = list_head {
                new_entry.next = Box::into_raw(Box::new(list_head));
            }
            list_head = Some(new_entry);
        }
        let mut rpm_property = bacnet_sys::BACNET_PROPERTY_REFERENCE::default();
        rpm_object.listOfProperties = Box::into_raw(Box::new(dbg!(list_head.unwrap())));

        let request_invoke_id = unsafe {
            bacnet_sys::Send_Read_Property_Multiple_Request(
                &mut rx_buf as *mut _,
                bacnet_sys::MAX_APDU.into(),
                self.device_id,
                &mut rpm_object,
            )
        };
        {
            let mut lock = TARGET_ADDRESSES.lock().unwrap();
            if let Some(target) = lock.get_mut(&self.device_id) {
                target.request = Some((request_invoke_id, RequestStatus::Ongoing));
            }
        }
        debug!("request_invoke_id = {}", request_invoke_id);

        recv(&mut src, &mut rx_buf, request_invoke_id)?;

        debug!("epics() finished in {:?}", init.elapsed());
        let epics = Epics::default();
        Ok(epics)
    }

    pub fn disconnect(&self) {
        unsafe {
            bacnet_sys::address_remove_device(self.device_id);
        }
    }
}
// Run the inner bip_receive() function and return when data has been provided for the given
fn recv(
    src: &mut bacnet_sys::BACNET_ADDRESS,
    rx_buf: &mut [u8],
    request_invoke_id: u8,
) -> Fallible<()> {
    const TIMEOUT: u32 = 100; // ms
    let start = std::time::Instant::now();
    loop {
        let pdu_len = unsafe {
            bacnet_sys::bip_receive(
                src,
                rx_buf.as_mut_ptr(),
                bacnet_sys::MAX_MPDU as u16,
                TIMEOUT,
            )
        };
        if pdu_len > 0 {
            unsafe { bacnet_sys::npdu_handler(src, rx_buf.as_mut_ptr(), pdu_len) }
        }
        if unsafe { bacnet_sys::tsm_invoke_id_free(request_invoke_id) } {
            break; // This means we processed the request successfully!
        }
        if unsafe { bacnet_sys::tsm_invoke_id_failed(request_invoke_id) } {
            // An error! Return
            bail!("TSM timeout");
        }

        if start.elapsed().as_secs() > 3 {
            // FIXME(tj): A better timeout here...
            bail!("APDU timeout");
        }
    }
    Ok(())
}

impl Drop for BACnetDevice {
    fn drop(&mut self) {
        // FIXME(tj): Call address_remove()
        info!("disconnecting");
    }
}

// ./bacrp 1025 analog-value 22 present-value --mac 192.168.10.96 --dnet 5 --dadr 14
#[derive(Debug)]
pub struct BACnetDeviceBuilder {
    ip: Ipv4Addr,
    dnet: u16,
    dadr: u8,
    port: u16,
    device_id: u32,
}

impl Default for BACnetDeviceBuilder {
    fn default() -> Self {
        Self {
            ip: Ipv4Addr::LOCALHOST,
            dnet: 0,
            dadr: 0,
            port: 0xBAC0,
            device_id: 0,
        }
    }
}

impl BACnetDeviceBuilder {
    pub fn ip(mut self, ip: Ipv4Addr) -> Self {
        self.ip = ip;
        self
    }

    pub fn dnet(mut self, dnet: u16) -> Self {
        self.dnet = dnet;
        self
    }

    pub fn dadr(mut self, dadr: u8) -> Self {
        self.dadr = dadr;
        self
    }

    pub fn port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn device_id(mut self, device_id: u32) -> Self {
        self.device_id = device_id;
        self
    }

    pub fn build(self) -> BACnetDevice {
        let BACnetDeviceBuilder {
            ip,
            dnet,
            dadr,
            port,
            device_id,
        } = self;
        let mut addr = bacnet_sys::BACNET_ADDRESS::default();
        addr.mac[..4].copy_from_slice(&ip.octets());
        addr.mac[4] = (port >> 8) as u8;
        addr.mac[5] = (port & 0xff) as u8;
        addr.mac_len = 6;
        addr.net = dnet;
        addr.adr[0] = dadr;
        addr.len = 1;

        BACnetDevice {
            device_id,
            max_apdu: 0,
            addr,
        }
    }
}

#[no_mangle]
extern "C" fn my_readprop_ack_handler(
    service_request: *mut u8,
    service_len: u16,
    src: *mut bacnet_sys::BACNET_ADDRESS,
    service_data: *mut bacnet_sys::BACNET_CONFIRMED_SERVICE_ACK_DATA,
) {
    let mut data: bacnet_sys::BACNET_READ_PROPERTY_DATA =
        bacnet_sys::BACNET_READ_PROPERTY_DATA::default();

    let invoke_id = unsafe { (*service_data).invoke_id };
    let mut lock = TARGET_ADDRESSES.lock().unwrap();
    if let Some(target) = find_matching_device(&mut lock, src, invoke_id) {
        // Decode the data
        let len = unsafe {
            bacnet_sys::rp_ack_decode_service_request(
                service_request,
                service_len.into(),
                &mut data as *mut _,
            )
        };
        if len >= 0 {
            // XXX Consider moving data decoding out. We should probably just stick to getting
            // the raw data, putting it somewhere and let someone else decode it.
            let decoded = decode_data(data);
            target.value = Some(decoded);
        } else {
            error!("<decode failed>");
            target.value = Some(Err(format_err!("failed to decode data")));
        }
        target.request = Some((invoke_id, RequestStatus::Done));
    }
}

fn decode_data(data: bacnet_sys::BACNET_READ_PROPERTY_DATA) -> Fallible<BACnetValue> {
    let mut value = bacnet_sys::BACNET_APPLICATION_DATA_VALUE::default();
    let appdata = data.application_data;
    let appdata_len = data.application_data_len;

    let len = unsafe {
        bacnet_sys::bacapp_decode_application_data(appdata, appdata_len as u32, &mut value)
    };

    if len == bacnet_sys::BACNET_STATUS_ERROR {
        bail!("decoding error");
    }

    Ok(match value.tag as u32 {
        bacnet_sys::BACNET_APPLICATION_TAG_BACNET_APPLICATION_TAG_NULL => BACnetValue::Null,
        bacnet_sys::BACNET_APPLICATION_TAG_BACNET_APPLICATION_TAG_BOOLEAN => {
            BACnetValue::Bool(unsafe { value.type_.Boolean })
        }
        bacnet_sys::BACNET_APPLICATION_TAG_BACNET_APPLICATION_TAG_SIGNED_INT => {
            BACnetValue::Int(unsafe { value.type_.Signed_Int })
        }
        bacnet_sys::BACNET_APPLICATION_TAG_BACNET_APPLICATION_TAG_UNSIGNED_INT => {
            BACnetValue::Uint(unsafe { value.type_.Unsigned_Int })
        }
        bacnet_sys::BACNET_APPLICATION_TAG_BACNET_APPLICATION_TAG_REAL => {
            BACnetValue::Real(unsafe { value.type_.Real })
        }
        bacnet_sys::BACNET_APPLICATION_TAG_BACNET_APPLICATION_TAG_DOUBLE => {
            BACnetValue::Double(unsafe { value.type_.Double })
        }
        bacnet_sys::BACNET_APPLICATION_TAG_BACNET_APPLICATION_TAG_CHARACTER_STRING => {
            // BACnet string has the following structure
            // size_t length, uint8_t encoding, char value[MAX_CHARACTER_STRING_BYTES]
            // For now just assume UTF-8 bytes, but we really should respect encodings...
            //
            // FIXME(tj): Look at value.type_.Character_String.encoding
            let s = unsafe {
                CStr::from_ptr(
                    value.type_.Character_String.value
                        [0..value.type_.Character_String.length as usize]
                        .as_ptr(),
                )
                .to_string_lossy()
                .into_owned()
            };
            BACnetValue::String(s)
        }
        bacnet_sys::BACNET_APPLICATION_TAG_BACNET_APPLICATION_TAG_ENUMERATED => {
            // FIXME(tj): Find the string representation of the enum (if possible).
            // See bacapp.c:1200
            // See bactext.c:1266 - bactext_binary_present_value_name()
            // Try calling:
            //
            // int bacapp_snprintf_value(char *str, size_t str_len, BACNET_OBJECT_PROPERTY_VALUE *object_value)
            //
            // It should return the numbers of characters written so we can permute it to a String
            let s = None;
            BACnetValue::Enum(unsafe { value.type_.Enumerated }, s)
        }
        _ => bail!("unhandled type tag {:?}", value.tag),
    })
}

#[no_mangle]
extern "C" fn my_readpropmultiple_ack_handler(
    service_request: u16,
    src: *mut bacnet_sys::BACNET_ADDRESS,
    service_data: *mut bacnet_sys::BACNET_CONFIRMED_SERVICE_ACK_DATA,
) {
    let mut data = bacnet_sys::BACNET_READ_ACCESS_DATA::default();
}

#[no_mangle]
extern "C" fn my_error_handler(
    src: *mut bacnet_sys::BACNET_ADDRESS,
    invoke_id: u8,
    error_class: bacnet_sys::BACNET_ERROR_CLASS,
    error_code: bacnet_sys::BACNET_ERROR_CODE,
) {
    // TODO(tj): address_match(&Target_Address, src) && invoke_id == Request_Invoke_ID
    let error_class_str =
        unsafe { CStr::from_ptr(bacnet_sys::bactext_error_class_name(error_class)) }
            .to_string_lossy()
            .into_owned();
    let error_code_str = unsafe { CStr::from_ptr(bacnet_sys::bactext_error_code_name(error_code)) }
        .to_string_lossy()
        .into_owned();
    error!(
        "BACnet error: error_class={} ({}) error_code={} ({})",
        error_class, error_class_str, error_code, error_code_str,
    );
}

#[no_mangle]
extern "C" fn my_abort_handler(
    src: *mut bacnet_sys::BACNET_ADDRESS,
    invoke_id: u8,
    abort_reason: u8,
    server: bool,
) {
    let _ = server;
    let _ = src;
    let mut lock = TARGET_ADDRESSES.lock().unwrap();
    if let Some(target) = find_matching_device(&mut lock, src, invoke_id) {
        let abort_text =
            unsafe { CStr::from_ptr(bacnet_sys::bactext_abort_reason_name(abort_reason as u32)) }
                .to_string_lossy()
                .into_owned();
        target.request = Some((invoke_id, RequestStatus::Aborted(abort_reason)));
        error!(
            "aborted invoke_id = {} abort_reason = {} ({})",
            invoke_id, abort_text, abort_reason
        );
    }
}

#[no_mangle]
extern "C" fn my_reject_handler(
    src: *mut bacnet_sys::BACNET_ADDRESS,
    invoke_id: u8,
    reject_reason: u8,
) {
    let _ = src;

    let mut lock = TARGET_ADDRESSES.lock().unwrap();
    if let Some(target) = find_matching_device(&mut lock, src, invoke_id) {
        target.request = Some((invoke_id, RequestStatus::Rejected(reject_reason)));
    }
}

// Holding the lock on the global map of devices, find a device that matches `src` and the given
// RequestInvokeId.
//
// This function _should_ return something.
fn find_matching_device<'a>(
    guard: &'a mut std::sync::MutexGuard<'_, HashMap<u32, TargetDevice>>,
    src: *mut bacnet_sys::BACNET_ADDRESS,
    invoke_id: RequestInvokeId,
) -> Option<&'a mut TargetDevice> {
    for target in guard.values_mut() {
        let is_addr_match = unsafe { bacnet_sys::address_match(&mut target.addr, src) };
        if let Some((request_invoke_id, _)) = &target.request {
            let is_request_invoke_id = invoke_id == *request_invoke_id;
            if is_addr_match && is_request_invoke_id {
                return Some(target);
            }
        }
    }
    error!("device wasn't matched! {:?}", src);
    return None;
}

unsafe fn init_service_handlers() {
    bacnet_sys::Device_Init(std::ptr::null_mut());
    bacnet_sys::apdu_set_unconfirmed_handler(
        bacnet_sys::BACNET_UNCONFIRMED_SERVICE_SERVICE_UNCONFIRMED_WHO_IS,
        Some(bacnet_sys::handler_who_is),
    );
    bacnet_sys::apdu_set_unconfirmed_handler(
        bacnet_sys::BACNET_UNCONFIRMED_SERVICE_SERVICE_UNCONFIRMED_I_AM,
        Some(bacnet_sys::handler_i_am_bind),
    );
    bacnet_sys::apdu_set_unrecognized_service_handler_handler(Some(
        bacnet_sys::handler_unrecognized_service,
    ));
    bacnet_sys::apdu_set_confirmed_handler(
        bacnet_sys::BACNET_CONFIRMED_SERVICE_SERVICE_CONFIRMED_READ_PROPERTY,
        Some(bacnet_sys::handler_read_property),
    );
    bacnet_sys::apdu_set_confirmed_ack_handler(
        bacnet_sys::BACNET_CONFIRMED_SERVICE_SERVICE_CONFIRMED_READ_PROPERTY,
        Some(my_readprop_ack_handler),
    );

    bacnet_sys::apdu_set_error_handler(
        bacnet_sys::BACNET_CONFIRMED_SERVICE_SERVICE_CONFIRMED_READ_PROPERTY,
        Some(my_error_handler),
    );
    bacnet_sys::apdu_set_abort_handler(Some(my_abort_handler));
    bacnet_sys::apdu_set_reject_handler(Some(my_reject_handler));
}
