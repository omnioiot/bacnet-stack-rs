#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
#[macro_use]
extern crate failure;

use std::collections::HashMap;
use std::ffi::CStr;
use std::net::Ipv4Addr;
use std::sync::{Mutex, Once};

use failure::Fallible;

pub mod whois;

static BACNET_STACK_INIT: Once = Once::new();

// We need a global structure here for collecting "target addresses"
lazy_static! {
    /// Global tracking struct for target addresses. These are devices that we consider ourselves
    /// connected to and communicating with.
    static ref TARGET_ADDRESSES: Mutex<HashMap<u32, TargetDevice>> = Mutex::new(HashMap::new());
}

// A structure for tracking
struct TargetDevice {
    addr: bacnet_sys::BACNET_ADDRESS,
    request_invoke_id: u8,
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

#[derive(Debug)]
pub enum BACnetValue {
    Null, // Yes!
    Bool(bool),
    Uint(u64),
    Int(i32),
    Real(f32),
    Double(f64),
    String(String), // BACNET_CHARACTER_STRING
    Bytes(Vec<u8>), // BACNET_OCTET_STRING
    Enum(u32),
}

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
                    request_invoke_id: 0,
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
                        bacnet_sys::BACNET_ARRAY_ALL,
                    )
                };
                h.request_invoke_id = request_invoke_id;
                request_invoke_id
            } else {
                bail!("Not connected to device {}", self.device_id)
            };

        let mut src = bacnet_sys::BACNET_ADDRESS::default();
        let mut rx_buf = [0u8; bacnet_sys::MAX_MPDU as usize];
        let start = std::time::Instant::now();
        loop {
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

        debug!("read_prop() finished in {:?}", init.elapsed());
        Ok(BACnetValue::Bool(true))
    }

    pub fn disconnect(&self) {
        todo!()
    }
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

    let mut lock = TARGET_ADDRESSES.lock().unwrap();

    for target in lock.values_mut() {
        let is_addr_match = unsafe { bacnet_sys::address_match(&mut target.addr, src) };
        let is_request_invoke_id = unsafe { (*service_data).invoke_id } == target.request_invoke_id;
        if is_addr_match && is_request_invoke_id {
            // Decode the data
            let len = unsafe {
                bacnet_sys::rp_ack_decode_service_request(
                    service_request,
                    service_len.into(),
                    &mut data as *mut _,
                )
            };
            if len >= 0 {
                //unsafe {
                //    bacnet_sys::rp_ack_print_data(&mut data);
                //}
                let decoded = decode_data(data);
                println!("{:?}", decoded);
            } else {
                println!("<decode failed>");
            }
            target.request_invoke_id = 0;
            return;
        }
    }
    error!("device wasn't matched! {:?}", src);
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
    println!("decoded {:?}", value.tag);

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
            BACnetValue::Enum(unsafe { value.type_.Enumerated })
        }
        _ => bail!("unhandled type tag {:?}", value.tag),
    })
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
    println!(
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
    println!(
        "aborted invoke_id = {} abort_reason = {}",
        invoke_id, abort_reason
    );
}

#[no_mangle]
extern "C" fn my_reject_handler(
    src: *mut bacnet_sys::BACNET_ADDRESS,
    invoke_id: u8,
    reject_reason: u8,
) {
    let _ = src;
    println!(
        "rejected invoke_id = {} reject_reason = {}",
        invoke_id, reject_reason
    );
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
